//! Background monitor that periodically pings the configured remote STT
//! provider and caches the result. Drives the UI status badge.

use crate::settings::TranscriptionProvider;
use log::debug;
use serde::Serialize;
use specta::Type;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

const POLL_INTERVAL: Duration = Duration::from_secs(30);
pub const FRESH_CACHE_WINDOW: Duration = Duration::from_secs(10);

/// Public snapshot of the most recent health check. Cloneable; sent to the
/// frontend as the payload of `remote-stt-health-changed` events.
#[derive(Debug, Clone, Serialize, Type)]
pub struct HealthSnapshot {
    pub enabled: bool,
    pub healthy: bool,
    pub provider_id: String,
    pub provider_label: String,
    pub last_error: Option<String>,
    /// Milliseconds since UNIX epoch.
    pub checked_at_ms: u64,
}

impl HealthSnapshot {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            healthy: false,
            provider_id: String::new(),
            provider_label: String::new(),
            last_error: None,
            checked_at_ms: 0,
        }
    }
}

/// Internal record that also tracks an Instant (not serializable) for cache
/// freshness comparisons.
#[derive(Debug, Clone)]
struct Cached {
    snapshot: HealthSnapshot,
    measured_at: Option<Instant>,
}

impl Cached {
    fn disabled() -> Self {
        Self {
            snapshot: HealthSnapshot::disabled(),
            measured_at: None,
        }
    }
}

#[derive(Clone)]
pub struct RemoteHealthMonitor {
    state: Arc<Mutex<Cached>>,
    wakeup: Arc<Notify>,
}

impl RemoteHealthMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Cached::disabled())),
            wakeup: Arc::new(Notify::new()),
        }
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        self.state.lock().unwrap().snapshot.clone()
    }

    /// True iff the cached result is healthy, belongs to `provider_id`, and
    /// was measured within the fresh-cache window. Callers use this to skip
    /// a redundant network probe without trusting a stale provider's result.
    pub fn is_fresh_and_healthy_for(&self, provider_id: &str) -> bool {
        let guard = self.state.lock().unwrap();
        if guard.snapshot.provider_id != provider_id || !guard.snapshot.healthy {
            return false;
        }
        match guard.measured_at {
            Some(t) => t.elapsed() < FRESH_CACHE_WINDOW,
            None => false,
        }
    }

    /// Replace the cached snapshot from an external probe result (used by
    /// `try_remote` so the UI updates immediately on real transcription
    /// outcomes, not just on the 30s timer).
    pub fn record_result(
        &self,
        app: &AppHandle,
        provider: &TranscriptionProvider,
        result: Result<(), String>,
        enabled: bool,
    ) {
        let snapshot = HealthSnapshot {
            enabled,
            healthy: result.is_ok(),
            provider_id: provider.id.clone(),
            provider_label: provider.label.clone(),
            last_error: result.err(),
            checked_at_ms: now_ms(),
        };
        let prev = {
            let mut guard = self.state.lock().unwrap();
            let prev = guard.snapshot.clone();
            *guard = Cached {
                snapshot: snapshot.clone(),
                measured_at: Some(Instant::now()),
            };
            prev
        };
        emit_if_changed(app, &prev, &snapshot);
    }

    /// Notify the background task to re-probe immediately (call when relevant
    /// settings change).
    pub fn nudge(&self) {
        self.wakeup.notify_one();
    }

    /// Mark monitor as disabled. Idempotent.
    pub fn mark_disabled(&self, app: &AppHandle) {
        let prev = {
            let mut guard = self.state.lock().unwrap();
            let prev = guard.snapshot.clone();
            *guard = Cached::disabled();
            prev
        };
        emit_if_changed(app, &prev, &HealthSnapshot::disabled());
    }
}

// `provider_label` is intentionally omitted from the change-detection comparison because label
// changes without id changes are not user-visible state transitions worth re-emitting.
fn emit_if_changed(app: &AppHandle, prev: &HealthSnapshot, next: &HealthSnapshot) {
    if prev.enabled != next.enabled
        || prev.healthy != next.healthy
        || prev.provider_id != next.provider_id
        || prev.last_error != next.last_error
    {
        let _ = app.emit("remote-stt-health-changed", next);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Spawn the background task. Holds clones of the monitor and reads settings
/// on each tick (so changes propagate without restart).
pub fn spawn(app: AppHandle, monitor: RemoteHealthMonitor) {
    tauri::async_runtime::spawn(async move {
        loop {
            let settings = crate::settings::get_settings(&app);
            if !settings.remote_transcription_enabled {
                monitor.mark_disabled(&app);
                // Sleep until a setting change wakes us up, or 30s as a heartbeat.
                tokio::select! {
                    _ = tokio::time::sleep(POLL_INTERVAL) => {},
                    _ = monitor.wakeup.notified() => {},
                }
                continue;
            }

            let provider_id = settings.remote_transcription_provider_id.clone();
            let provider = settings
                .remote_transcription_providers
                .iter()
                .find(|p| p.id == provider_id)
                .cloned();
            match provider {
                Some(provider) => {
                    let api_key = settings
                        .remote_transcription_api_keys
                        .get(&provider.id)
                        .cloned()
                        .unwrap_or_default();
                    debug!("Health monitor probing provider {}", provider.id);
                    let result = crate::remote_stt::client::health_check(&provider, &api_key).await;
                    monitor.record_result(&app, &provider, result, true);
                }
                None => {
                    // Misconfiguration: enabled is true but the selected provider_id
                    // doesn't match any known provider. Surface as an unhealthy snapshot
                    // so the UI badge reflects reality instead of stale state.
                    let synthetic = TranscriptionProvider {
                        id: provider_id.clone(),
                        label: provider_id.clone(),
                        base_url: String::new(),
                        allow_base_url_edit: false,
                        health_endpoint: None,
                    };
                    monitor.record_result(
                        &app,
                        &synthetic,
                        Err(format!("Configured provider '{provider_id}' not found")),
                        true,
                    );
                }
            }

            tokio::select! {
                _ = tokio::time::sleep(POLL_INTERVAL) => {},
                _ = monitor.wakeup.notified() => {},
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_cache_is_initially_false() {
        let m = RemoteHealthMonitor::new();
        assert!(!m.is_fresh_and_healthy_for("test"));
    }

    #[test]
    fn disabled_snapshot_has_enabled_false() {
        let s = HealthSnapshot::disabled();
        assert!(!s.enabled);
        assert!(!s.healthy);
    }
}
