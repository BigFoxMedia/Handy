//! HTTP client for OpenAI-compatible speech-to-text endpoints.

use crate::remote_stt::wav::encode_f32_to_wav_bytes;
use crate::settings::TranscriptionProvider;
use log::{debug, warn};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::time::Duration;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(10);
const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct RemoteTranscribeOptions {
    pub model: String,
    /// `None` or `"auto"` → omit the `language` field; let the server decide.
    pub language: Option<String>,
    /// Joined with `, ` and sent as the `prompt` field for vocabulary biasing.
    pub custom_words: Vec<String>,
    /// If true, hit `/audio/translations` instead of `/audio/transcriptions`.
    pub translate_to_english: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

fn build_headers(api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("Handy/1.0"));
    if !api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|e| format!("Invalid Authorization header: {e}"))?,
        );
    }
    Ok(headers)
}

fn build_client(api_key: &str, timeout: Duration) -> Result<reqwest::Client, String> {
    let headers = build_headers(api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

fn base(provider: &TranscriptionProvider) -> &str {
    provider.base_url.trim_end_matches('/')
}

fn sanitized_url_for_log(url: &str) -> &str {
    url.split_once('?').map(|(base, _)| base).unwrap_or(url)
}

fn health_url(provider: &TranscriptionProvider) -> String {
    let endpoint = provider.health_endpoint.as_deref().unwrap_or("/models");
    format!("{}{}", base(provider), endpoint)
}

fn transcribe_url(provider: &TranscriptionProvider, translate: bool) -> String {
    let path = if translate {
        "/audio/translations"
    } else {
        "/audio/transcriptions"
    };
    format!("{}{}", base(provider), path)
}

/// Cheap availability check against `{base_url}/models` (or a custom path).
/// Returns Ok(()) on 2xx, Err otherwise. Used by the health monitor and as the
/// pre-flight check before each transcription.
pub async fn health_check(provider: &TranscriptionProvider, api_key: &str) -> Result<(), String> {
    if provider.base_url.trim().is_empty() {
        return Err("Provider base URL is empty".to_string());
    }
    let url = health_url(provider);
    debug!(
        "Remote STT health check: GET {}",
        sanitized_url_for_log(&url)
    );
    let client = build_client(api_key, HEALTH_TIMEOUT)?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Health check request failed: {e}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("Health check returned HTTP {}", response.status()))
    }
}

/// Send PCM f32 audio to the remote endpoint and return the transcribed text.
pub async fn transcribe(
    provider: &TranscriptionProvider,
    api_key: &str,
    audio: &[f32],
    options: &RemoteTranscribeOptions,
) -> Result<String, String> {
    transcribe_with_timeout(provider, api_key, audio, options, TRANSCRIBE_TIMEOUT).await
}

/// Preview decodes use a shorter timeout so a slow endpoint cannot hold the
/// live overlay for the whole recording. Preview failures never fall back.
pub async fn transcribe_preview(
    provider: &TranscriptionProvider,
    api_key: &str,
    audio: &[f32],
    options: &RemoteTranscribeOptions,
) -> Result<String, String> {
    transcribe_with_timeout(provider, api_key, audio, options, PREVIEW_TIMEOUT).await
}

async fn transcribe_with_timeout(
    provider: &TranscriptionProvider,
    api_key: &str,
    audio: &[f32],
    options: &RemoteTranscribeOptions,
    timeout: Duration,
) -> Result<String, String> {
    if provider.base_url.trim().is_empty() {
        return Err("Provider base URL is empty".to_string());
    }
    if options.model.trim().is_empty() {
        return Err("Remote model name is empty".to_string());
    }

    let wav = encode_f32_to_wav_bytes(audio, SAMPLE_RATE)?;
    let url = transcribe_url(provider, options.translate_to_english);
    debug!(
        "Remote STT POST {} (model={}, audio_bytes={}, translate={})",
        sanitized_url_for_log(&url),
        options.model,
        wav.len(),
        options.translate_to_english
    );

    let file_part = Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to set MIME: {e}"))?;

    let mut form = Form::new()
        .text("model", options.model.clone())
        .text("response_format", "json")
        .part("file", file_part);

    if let Some(lang) = &options.language {
        if !lang.is_empty() && lang != "auto" {
            let normalized = match lang.as_str() {
                "zh-Hans" | "zh-Hant" => "zh".to_string(),
                other => other.to_string(),
            };
            form = form.text("language", normalized);
        }
    }

    if !options.custom_words.is_empty() {
        form = form.text("prompt", options.custom_words.join(", "));
    }

    let client = build_client(api_key, timeout)?;
    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Remote STT request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        warn!("Remote STT non-success {status}: {body}");
        return Err(format!("Remote STT HTTP {status}: {body}"));
    }

    let parsed: TranscriptionResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse remote STT response: {e}"))?;
    Ok(parsed.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base_url: &str) -> TranscriptionProvider {
        TranscriptionProvider {
            id: "test".to_string(),
            label: "Test".to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            health_endpoint: None,
        }
    }

    fn opts(translate: bool) -> RemoteTranscribeOptions {
        RemoteTranscribeOptions {
            model: "whisper-1".to_string(),
            language: None,
            custom_words: vec![],
            translate_to_english: translate,
        }
    }

    #[test]
    fn url_helpers_trim_trailing_slash() {
        let p = provider("http://example.com/v1/");
        assert_eq!(
            transcribe_url(&p, false),
            "http://example.com/v1/audio/transcriptions"
        );
        assert_eq!(
            transcribe_url(&p, true),
            "http://example.com/v1/audio/translations"
        );
        assert_eq!(health_url(&p), "http://example.com/v1/models");
    }

    #[test]
    fn health_url_respects_custom_endpoint() {
        let mut p = provider("http://example.com/v1");
        p.health_endpoint = Some("/health".to_string());
        assert_eq!(health_url(&p), "http://example.com/v1/health");
    }

    #[test]
    fn build_headers_sets_bearer_when_key_present() {
        let headers = build_headers("secret").unwrap();
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap().to_str().unwrap(),
            "Bearer secret"
        );
    }

    #[test]
    fn build_headers_omits_auth_when_key_empty() {
        let headers = build_headers("").unwrap();
        assert!(headers.get(AUTHORIZATION).is_none());
    }

    #[tokio::test]
    async fn health_check_returns_ok_on_200() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/models")
            .with_status(200)
            .with_body(r#"{"data":[]}"#)
            .create_async()
            .await;
        let p = provider(&server.url());
        health_check(&p, "k").await.expect("expected Ok");
    }

    #[tokio::test]
    async fn health_check_errors_on_5xx() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/models")
            .with_status(503)
            .create_async()
            .await;
        let p = provider(&server.url());
        assert!(health_check(&p, "k").await.is_err());
    }

    #[tokio::test]
    async fn health_check_errors_on_empty_base_url() {
        let p = provider("");
        assert!(health_check(&p, "k").await.is_err());
    }

    #[tokio::test]
    async fn transcribe_happy_path_returns_text() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/audio/transcriptions")
            .match_header("authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"text":"hello world"}"#)
            .create_async()
            .await;
        let p = provider(&server.url());
        let audio = vec![0.1f32; 1600]; // 100ms of silence-ish
        let text = transcribe(&p, "test-key", &audio, &opts(false))
            .await
            .unwrap();
        assert_eq!(text, "hello world");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn transcribe_preview_returns_text() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/audio/transcriptions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"text":"live preview"}"#)
            .create_async()
            .await;
        let p = provider(&server.url());
        let text = transcribe_preview(&p, "k", &[0.0; 1600], &opts(false))
            .await
            .unwrap();
        assert_eq!(text, "live preview");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn transcribe_uses_translations_endpoint_when_translating() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/audio/translations")
            .with_status(200)
            .with_body(r#"{"text":"translated"}"#)
            .create_async()
            .await;
        let p = provider(&server.url());
        let text = transcribe(&p, "k", &[0.0; 16], &opts(true)).await.unwrap();
        assert_eq!(text, "translated");
    }

    #[tokio::test]
    async fn transcribe_propagates_401_as_error() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/audio/transcriptions")
            .with_status(401)
            .with_body(r#"{"error":"bad key"}"#)
            .create_async()
            .await;
        let p = provider(&server.url());
        let err = transcribe(&p, "k", &[0.0; 16], &opts(false))
            .await
            .unwrap_err();
        assert!(err.contains("401"));
    }

    #[tokio::test]
    async fn transcribe_errors_on_malformed_json() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/audio/transcriptions")
            .with_status(200)
            .with_body("not json")
            .create_async()
            .await;
        let p = provider(&server.url());
        assert!(transcribe(&p, "k", &[0.0; 16], &opts(false)).await.is_err());
    }

    #[tokio::test]
    async fn transcribe_errors_on_empty_model() {
        let server = mockito::Server::new_async().await;
        let p = provider(&server.url());
        let mut o = opts(false);
        o.model = String::new();
        assert!(transcribe(&p, "k", &[0.0; 16], &o).await.is_err());
    }
}
