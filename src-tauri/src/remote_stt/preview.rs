//! Chunked live preview for remote STT.
//!
//! The remote server has no streaming/SSE audio endpoint, so this worker
//! builds a bounded growing window and re-decodes it as speech arrives. Text
//! is emitted through the same `StreamTextEvent` the local streaming overlay
//! uses. The final text is still produced by the full-recording batch request.

use crate::managers::transcription::{finish_remote_text, StreamTextEvent};
use crate::remote_stt::{transcribe_preview, RemoteTranscribeOptions};
use crate::settings::{AppSettings, TranscriptionProvider};
use log::{debug, warn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
use tauri_specta::Event;

const MIN_SAMPLES: usize = 16_000; // ~1 s
const UPDATE_SAMPLES: usize = 24_000; // ~1.5 s of new audio between tentative decodes
const MAX_SAMPLES: usize = 12 * 16_000; // forced commit ceiling
const QUIET_RMS: f32 = 0.01;
const QUIET_FRAMES: usize = 3;
const TICK: Duration = Duration::from_millis(120);

enum PreviewCmd {
    Feed(Vec<f32>),
    Cancel,
}

#[derive(Clone)]
struct PreviewRoute {
    tx: mpsc::SyncSender<PreviewCmd>,
    cancel: Arc<AtomicBool>,
}

pub struct RemotePreviewRouter {
    route: Mutex<Option<PreviewRoute>>,
    open: Arc<AtomicBool>,
}

impl RemotePreviewRouter {
    fn new() -> Self {
        Self {
            route: Mutex::new(None),
            open: Arc::new(AtomicBool::new(false)),
        }
    }

    fn open(&self, tx: mpsc::SyncSender<PreviewCmd>, cancel: Arc<AtomicBool>) {
        self.open.store(true, Ordering::Release);
        *self.route.lock().unwrap() = Some(PreviewRoute { tx, cancel });
    }

    fn take(&self) -> Option<PreviewRoute> {
        self.open.store(false, Ordering::Release);
        self.route.lock().unwrap().take()
    }

    fn clear(&self) {
        self.open.store(false, Ordering::Release);
        *self.route.lock().unwrap() = None;
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    pub fn feed(&self, frame: &[f32]) {
        if !self.open.load(Ordering::Relaxed) {
            return;
        }
        if let Some(route) = self.route.lock().unwrap().as_ref() {
            // Bounded channel: a slow endpoint must not grow an unbounded
            // audio backlog. Dropping a preview frame is acceptable; the final
            // batch request still has the full recording.
            let _ = route.tx.try_send(PreviewCmd::Feed(frame.to_vec()));
        }
    }
}

struct WorkerHandle {
    alive: Arc<AtomicBool>,
}

pub struct RemotePreviewManager {
    router: Arc<RemotePreviewRouter>,
    worker: Mutex<Option<WorkerHandle>>,
    session: Arc<AtomicU64>,
}

impl RemotePreviewManager {
    pub fn new() -> Self {
        Self {
            router: Arc::new(RemotePreviewRouter::new()),
            worker: Mutex::new(None),
            session: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn router(&self) -> Arc<RemotePreviewRouter> {
        Arc::clone(&self.router)
    }

    pub fn is_active(&self) -> bool {
        let worker = self.worker.lock().unwrap();
        worker
            .as_ref()
            .map(|handle| handle.alive.load(Ordering::Acquire))
            .unwrap_or(false)
            && self.router.is_open()
    }

    /// True when a preview worker was created for this take, even if it has
    /// since stopped on an error. Used to keep the Live overlay transition
    /// coherent at stop.
    pub fn was_started(&self) -> bool {
        self.worker.lock().unwrap().is_some()
    }

    pub fn start(&self, app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
        // Invalidating first also kills a leaked worker from a previous take
        // when the new configuration is unusable.
        self.cancel();

        if !settings.remote_transcription_enabled {
            return Err("Remote transcription is not enabled".to_string());
        }
        if !settings.remote_transcription_streaming_enabled {
            return Err("Remote live preview is not enabled".to_string());
        }

        let provider = settings
            .remote_transcription_provider(&settings.remote_transcription_provider_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Remote provider '{}' not found",
                    settings.remote_transcription_provider_id
                )
            })?;
        if provider.base_url.trim().is_empty() {
            return Err("Remote provider base URL is empty".to_string());
        }
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
            return Err("Remote model is empty".to_string());
        }

        let options = RemoteTranscribeOptions {
            model,
            language: Some(settings.selected_language.clone()),
            custom_words: settings.custom_words.clone(),
            translate_to_english: settings.translate_to_english,
        };

        let session = self.session.fetch_add(1, Ordering::SeqCst) + 1;
        self.session.store(session, Ordering::SeqCst);

        let (tx, rx) = mpsc::sync_channel(512);
        let cancel = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        self.router.open(tx, Arc::clone(&cancel));

        let app_clone = app.clone();
        let router = Arc::clone(&self.router);
        let session_atomic = Arc::clone(&self.session);
        let worker_alive = Arc::clone(&alive);
        let worker_settings = settings.clone();
        std::thread::Builder::new()
            .name("remote-preview".into())
            .spawn(move || {
                run_preview_worker(
                    app_clone,
                    router,
                    session_atomic,
                    session,
                    worker_alive,
                    provider,
                    api_key,
                    worker_settings,
                    options,
                    rx,
                    cancel,
                );
            })
            .map_err(|e| {
                self.router.clear();
                self.session.fetch_add(1, Ordering::SeqCst);
                format!("Failed to start remote preview worker: {e}")
            })?;

        *self.worker.lock().unwrap() = Some(WorkerHandle {
            alive: Arc::clone(&alive),
        });

        debug!(
            "Remote preview started for provider {}",
            settings.remote_transcription_provider_id
        );
        Ok(())
    }

    /// Stop the preview without producing final text. The final full-recording
    /// batch request is what produces the authoritative transcript.
    pub fn cancel(&self) {
        // Invalidate the session before the worker can observe the cancel flag,
        // so a request completing in flight cannot emit into the next take.
        self.session.fetch_add(1, Ordering::SeqCst);
        if let Some(handle) = self.worker.lock().unwrap().take() {
            handle.alive.store(false, Ordering::Release);
        }
        if let Some(route) = self.router.take() {
            route.cancel.store(true, Ordering::Release);
            let _ = route.tx.try_send(PreviewCmd::Cancel);
        }
    }
}

fn run_preview_worker(
    app: AppHandle,
    router: Arc<RemotePreviewRouter>,
    session_atomic: Arc<AtomicU64>,
    session: u64,
    alive: Arc<AtomicBool>,
    provider: TranscriptionProvider,
    api_key: String,
    settings: AppSettings,
    options: RemoteTranscribeOptions,
    rx: mpsc::Receiver<PreviewCmd>,
    cancel: Arc<AtomicBool>,
) {
    let mut frames: Vec<Vec<f32>> = Vec::new();
    let mut total_samples = 0;
    let mut quiet_run = 0;
    let mut last_decoded = 0;
    let mut failures = 0;
    let mut committed = String::new();
    let language_hint =
        if settings.selected_language.is_empty() || settings.selected_language == "auto" {
            None
        } else {
            Some(settings.selected_language.clone())
        };

    loop {
        if cancel.load(Ordering::Acquire) || session_atomic.load(Ordering::SeqCst) != session {
            break;
        }

        match rx.recv_timeout(TICK) {
            Ok(PreviewCmd::Feed(frame)) => {
                if frame.is_empty() {
                    continue;
                }
                let rms = rms(&frame);
                if rms < QUIET_RMS {
                    quiet_run += 1;
                } else {
                    quiet_run = 0;
                }
                total_samples += frame.len();
                frames.push(frame);
            }
            Ok(PreviewCmd::Cancel) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if total_samples < MIN_SAMPLES {
            continue;
        }

        let pause = quiet_run >= QUIET_FRAMES;
        let forced = total_samples >= MAX_SAMPLES;
        let grown = total_samples.saturating_sub(last_decoded) >= UPDATE_SAMPLES;
        if !(pause || forced || grown) {
            continue;
        }

        if pause || forced {
            let cut = if forced && !pause {
                quietest_cut(&frames)
            } else {
                frames.len()
            };
            if cut == 0 {
                continue;
            }
            let prefix: Vec<Vec<f32>> = frames.drain(0..cut).collect();
            let samples = flatten(&prefix);
            if samples.is_empty() {
                continue;
            }
            match preview_transcribe(&provider, &api_key, &samples, &options) {
                Ok(raw) => {
                    failures = 0;
                    let text = finish_remote_text(raw, &settings, language_hint.clone());
                    if !text.is_empty() {
                        if !committed.is_empty() {
                            committed.push(' ');
                        }
                        committed.push_str(&text);
                    }
                }
                Err(e) => {
                    failures += 1;
                    warn!("Remote preview failed ({failures}/2): {e}");
                    last_decoded = total_samples;
                    if failures >= 2 {
                        break;
                    }
                    continue;
                }
            }

            total_samples = frames.iter().map(Vec::len).sum();
            quiet_run = trailing_quiet(&frames);
            last_decoded = total_samples;
            emit_stream_text(&app, &session_atomic, session, &committed, "");
        } else {
            let samples = flatten(&frames);
            match preview_transcribe(&provider, &api_key, &samples, &options) {
                Ok(raw) => {
                    failures = 0;
                    let tentative = finish_remote_text(raw, &settings, language_hint.clone());
                    emit_stream_text(&app, &session_atomic, session, &committed, &tentative);
                }
                Err(e) => {
                    failures += 1;
                    warn!("Remote preview failed ({failures}/2): {e}");
                    last_decoded = total_samples;
                    if failures >= 2 {
                        break;
                    }
                    continue;
                }
            }
            last_decoded = total_samples;
        }
    }

    alive.store(false, Ordering::Release);
    if session_atomic.load(Ordering::SeqCst) == session {
        router.clear();
    }
}

fn preview_transcribe(
    provider: &TranscriptionProvider,
    api_key: &str,
    samples: &[f32],
    options: &RemoteTranscribeOptions,
) -> Result<String, String> {
    tauri::async_runtime::block_on(transcribe_preview(provider, api_key, samples, options))
}

fn emit_stream_text(
    app: &AppHandle,
    session_atomic: &AtomicU64,
    session: u64,
    committed: &str,
    tentative: &str,
) {
    if session_atomic.load(Ordering::SeqCst) != session {
        return;
    }
    let _ = StreamTextEvent {
        committed: committed.to_string(),
        tentative: tentative.to_string(),
    }
    .emit(app);
}

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum = frame.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / frame.len() as f32).sqrt()
}

fn flatten(frames: &[Vec<f32>]) -> Vec<f32> {
    frames
        .iter()
        .flat_map(|frame| frame.iter().copied())
        .collect()
}

fn trailing_quiet(frames: &[Vec<f32>]) -> usize {
    frames
        .iter()
        .rev()
        .take_while(|frame| rms(frame) < QUIET_RMS)
        .count()
}

fn quietest_cut(frames: &[Vec<f32>]) -> usize {
    if frames.is_empty() {
        return 0;
    }
    let mut start = frames.len();
    let mut samples = 0;
    while start > 0 && samples < 16_000 {
        start -= 1;
        samples += frames[start].len();
    }

    let mut best_index = frames.len() - 1;
    let mut best_rms = f32::MAX;
    for index in start..frames.len() {
        let current = rms(&frames[index]);
        if current < best_rms {
            best_rms = current;
            best_index = index;
        }
    }
    best_index + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quietest_cut_finds_the_quietest_frame_in_the_last_second() {
        let loud = vec![0.2f32; 512];
        let quiet = vec![0.0f32; 512];
        let frames = vec![loud.clone(), loud.clone(), quiet.clone(), loud.clone()];
        assert_eq!(quietest_cut(&frames), 3);
    }

    #[test]
    fn trailing_quiet_counts_only_the_tail() {
        let loud = vec![0.2f32; 512];
        let quiet = vec![0.0f32; 512];
        let frames = vec![loud.clone(), quiet.clone(), quiet.clone()];
        assert_eq!(trailing_quiet(&frames), 2);
    }

    #[test]
    fn router_feed_is_a_noop_when_closed() {
        let router = RemotePreviewRouter::new();
        router.feed(&[0.1, 0.2]);
        assert!(!router.is_open());
    }

    #[test]
    fn flatten_preserves_order_and_length() {
        let frames = vec![vec![1.0, 2.0], vec![3.0]];
        assert_eq!(flatten(&frames), vec![1.0, 2.0, 3.0]);
    }
}
