//! Tauri commands for the remote STT settings UI and status badge.

use crate::remote_stt::{self, HealthSnapshot, RemoteHealthMonitor};
use crate::settings::{
    get_settings, write_settings, AppSettings, PostProcessApiFormat, PostProcessProvider,
};
use tauri::{AppHandle, Manager, State};

fn validate_remote_provider_exists(
    settings: &AppSettings,
    provider_id: &str,
) -> Result<(), String> {
    if settings
        .remote_transcription_providers
        .iter()
        .any(|p| p.id == provider_id)
    {
        Ok(())
    } else {
        Err(format!("Remote provider '{}' not found", provider_id))
    }
}

fn nudge_monitor(app: &AppHandle) {
    if let Some(m) = app.try_state::<RemoteHealthMonitor>() {
        m.nudge();
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.remote_transcription_enabled = enabled;
    write_settings(&app, settings);
    nudge_monitor(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_streaming_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.remote_transcription_streaming_enabled = enabled;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_remote_transcription_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    validate_remote_provider_exists(&settings, &provider_id)?;
    settings.remote_transcription_provider_id = provider_id;
    write_settings(&app, settings);
    nudge_monitor(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_base_url_setting(
    app: AppHandle,
    provider_id: String,
    base_url: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    let label = settings
        .remote_transcription_provider(&provider_id)
        .map(|p| p.label.clone())
        .ok_or_else(|| format!("Remote provider '{}' not found", provider_id))?;
    let provider = settings
        .remote_transcription_provider_mut(&provider_id)
        .expect("Provider looked up above must exist");
    if !provider.allow_base_url_edit {
        return Err(format!(
            "Provider '{}' does not allow editing the base URL",
            label
        ));
    }
    provider.base_url = base_url;
    write_settings(&app, settings);
    nudge_monitor(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_api_key_setting(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    validate_remote_provider_exists(&settings, &provider_id)?;
    settings
        .remote_transcription_api_keys
        .insert(provider_id, api_key);
    write_settings(&app, settings);
    nudge_monitor(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_health_endpoint_setting(
    app: AppHandle,
    provider_id: String,
    health_endpoint: Option<String>,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    let provider = settings
        .remote_transcription_provider_mut(&provider_id)
        .ok_or_else(|| format!("Remote provider '{}' not found", provider_id))?;
    provider.health_endpoint = health_endpoint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    write_settings(&app, settings);
    nudge_monitor(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_remote_transcription_model_setting(
    app: AppHandle,
    provider_id: String,
    model: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    validate_remote_provider_exists(&settings, &provider_id)?;
    settings
        .remote_transcription_models
        .insert(provider_id, model);
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_remote_transcription_models(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let settings = get_settings(&app);
    let provider = settings
        .remote_transcription_provider(&provider_id)
        .cloned()
        .ok_or_else(|| format!("Remote provider '{}' not found", provider_id))?;
    let api_key = settings
        .remote_transcription_api_keys
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    // Reuse the post-processing models fetcher — same /models contract.
    crate::llm_client::fetch_models(
        &PostProcessProvider {
            id: provider.id,
            label: provider.label,
            base_url: provider.base_url,
            allow_base_url_edit: provider.allow_base_url_edit,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        },
        api_key,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn test_remote_transcription_connection(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    let settings = get_settings(&app);
    let provider = settings
        .remote_transcription_provider(&provider_id)
        .cloned()
        .ok_or_else(|| format!("Remote provider '{}' not found", provider_id))?;
    let api_key = settings
        .remote_transcription_api_keys
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    remote_stt::health_check(&provider, &api_key).await
}

#[tauri::command]
#[specta::specta]
pub fn get_remote_transcription_health(
    monitor: State<RemoteHealthMonitor>,
) -> Result<HealthSnapshot, String> {
    Ok(monitor.snapshot())
}
