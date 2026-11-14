//! Remote OpenAI-compatible speech-to-text client and health monitor.

use crate::settings::AppSettings;
use tauri::{AppHandle, Manager};

pub mod client;
pub mod health;
pub mod wav;

pub use client::{health_check, transcribe, RemoteTranscribeOptions};
pub use health::{HealthSnapshot, RemoteHealthMonitor};

/// Resolve the configured provider and transcribe `audio` remotely.
/// Returns the raw text or the error that sends the caller to local fallback.
pub async fn transcribe_with_settings(
    app: &AppHandle,
    settings: &AppSettings,
    audio: &[f32],
) -> Result<String, String> {
    let provider_id = settings.remote_transcription_provider_id.clone();
    let provider = settings
        .remote_transcription_provider(&provider_id)
        .cloned()
        .ok_or_else(|| format!("Remote provider '{provider_id}' not found"))?;
    let api_key = settings
        .remote_transcription_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();
    let model = settings
        .remote_transcription_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();
    if model.trim().is_empty() {
        return Err("Remote transcription model is empty".to_string());
    }

    let monitor = app
        .try_state::<RemoteHealthMonitor>()
        .map(|state| (*state).clone());

    if let Some(monitor) = &monitor {
        if !monitor.is_fresh_and_healthy_for(&provider.id) {
            // Best-effort pre-flight: a missing /models endpoint must not
            // block a server that can transcribe. The transcription request
            // below decides whether the endpoint actually works.
            match health_check(&provider, &api_key).await {
                Ok(()) => monitor.record_result(app, &provider, Ok(()), true),
                Err(e) => {
                    monitor.record_result(app, &provider, Err(e.clone()), true);
                    log::warn!("Remote STT pre-flight check failed; trying anyway: {e}");
                }
            }
        }
    }

    let options = RemoteTranscribeOptions {
        model,
        language: Some(settings.selected_language.clone()),
        custom_words: settings.custom_words.clone(),
        translate_to_english: settings.translate_to_english,
    };

    let result = transcribe(&provider, &api_key, audio, &options).await;
    if let Some(monitor) = &monitor {
        monitor.record_result(
            app,
            &provider,
            result.as_ref().map(|_| ()).map_err(|e| e.clone()),
            true,
        );
    }
    result
}

/// Emitted after a transcription to say where its text came from.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TranscriptionSourceEvent {
    /// "remote", "local-fallback", or "local".
    pub source: String,
    pub provider_id: Option<String>,
    pub error: Option<String>,
}
