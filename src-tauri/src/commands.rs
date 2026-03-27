use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

use crate::audio::capture;
use crate::audio::playback::{self, TtsHandle};
use crate::audio::resample::AudioResampler;
use crate::transport::client::TranslationClient;
use crate::transport::codec::{SessionConfig, TranslationEvent};

/// Subtitle event sent to the frontend via Tauri Channel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SubtitleEvent {
    #[serde(rename = "source")]
    Source { text: String, is_final: bool },
    #[serde(rename = "translation")]
    Translation { text: String, is_final: bool },
    #[serde(rename = "status")]
    Status { message: String },
    #[serde(rename = "error")]
    Error { message: String },
}

/// App state to track running session
pub struct AppState {
    pub stop_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            stop_tx: Mutex::new(None),
        }
    }
}

/// Drop guard that ensures stop_tx is cleared even if the event loop task panics.
/// Without this, a panic would leave stop_tx set, causing "Already running" forever.
struct CleanupGuard {
    app_handle: AppHandle,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let app = self.app_handle.clone();
        // Schedule async cleanup on the tokio runtime.
        // try_current() may fail during app shutdown — that's OK since
        // the app is exiting anyway.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                *app.state::<AppState>().stop_tx.lock().await = None;
            });
        }
    }
}

#[tauri::command]
pub async fn start_interpretation(
    app: AppHandle,
    on_subtitle: Channel<SubtitleEvent>,
    app_key: String,
    access_key: String,
    source_language: String,
    target_language: String,
    enable_tts: bool,
    speaker_id: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Atomic check-and-set: single lock acquisition prevents TOCTOU race
    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
    {
        let mut guard = state.stop_tx.lock().await;
        if guard.is_some() {
            return Err("Already running".to_string());
        }
        *guard = Some(stop_tx);
    }

    let mode = if enable_tts { "s2s" } else { "s2t" };
    info!("Starting interpretation: mode={}", mode);

    // Save config params for potential reconnection during auto-pause
    let cfg_app_key = app_key.clone();
    let cfg_access_key = access_key.clone();
    let cfg_source_lang = source_language.clone();
    let cfg_target_lang = target_language.clone();
    let cfg_speaker_id = speaker_id.clone();
    let cfg_mode = mode.to_string();

    let session_id = uuid::Uuid::new_v4().to_string();
    let connection_id = uuid::Uuid::new_v4().to_string();

    let config = SessionConfig {
        app_key,
        access_key,
        resource_id: "volc.service_type.10053".to_string(),
        connection_id,
        session_id,
        mode: mode.to_string(),
        source_language,
        target_language,
        speaker_id,
    };

    // Connect to API
    on_subtitle
        .send(SubtitleEvent::Status {
            message: "connecting".to_string(),
        })
        .ok();

    let client = TranslationClient::new(config);
    // map_err converts Box<dyn Error> (non-Send) to String before any await
    let connect_result = client.connect().await.map_err(|e| e.to_string());
    let (audio_tx, event_rx) = match connect_result {
        Ok(v) => v,
        Err(err_msg) => {
            *state.stop_tx.lock().await = None;
            return Err(err_msg);
        }
    };

    // Wait for SessionStarted with timeout to prevent hanging forever
    let mut event_rx = event_rx;
    match timeout(Duration::from_secs(10), event_rx.recv()).await {
        Err(_) => {
            *state.stop_tx.lock().await = None;
            return Err("timeout".to_string());
        }
        Ok(response) => match response {
            Some(TranslationEvent::SessionStarted) => {
                on_subtitle
                    .send(SubtitleEvent::Status {
                        message: if enable_tts {
                            "connected_tts".to_string()
                        } else {
                            "connected_text".to_string()
                        },
                    })
                    .ok();
            }
            Some(TranslationEvent::SessionFailed { message }) => {
                *state.stop_tx.lock().await = None;
                return Err(format!("api_failed:{}", message));
            }
            _ => {
                *state.stop_tx.lock().await = None;
                return Err("unexpected_response".to_string());
            }
        },
    }

    // Start audio capture
    let capture_result = capture::start_capture(50).await.map_err(|e| e.to_string());
    let (mut audio_rx, capture_handle) = match capture_result {
        Ok(v) => v,
        Err(err_msg) => {
            *state.stop_tx.lock().await = None;
            return Err(err_msg);
        }
    };

    on_subtitle
        .send(SubtitleEvent::Status {
            message: "started".to_string(),
        })
        .ok();

    // Initialize TTS player if enabled
    let original_volume: Option<u32> = None;

    let tts_player = if enable_tts {
        match TtsHandle::new() {
            Ok(player) => Some(player),
            Err(e) => {
                warn!("TTS player init failed: {}, continuing without voice", e);
                on_subtitle
                    .send(SubtitleEvent::Status {
                        message: "tts_init_failed".to_string(),
                    })
                    .ok();
                None
            }
        }
    } else {
        None
    };

    // Echo suppression: share TTS playback timestamp with audio pipeline.
    let tts_last_played: Arc<AtomicI64> = tts_player
        .as_ref()
        .map(|p| p.last_played_ms())
        .unwrap_or_else(|| Arc::new(AtomicI64::new(0)));
    let echo_suppression = tts_last_played.clone();

    // Spawn unified session lifecycle task.
    // Merges audio pipeline + event loop into one task to support
    // auto-pause (disconnect API after 60s silence) and auto-reconnect
    // (reconnect when speech resumes), saving API tokens during idle periods.
    let on_sub = on_subtitle.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        let _cleanup = CleanupGuard {
            app_handle: app_handle.clone(),
        };

        // API connection state (Option allows disconnect/reconnect)
        let mut audio_tx: Option<mpsc::Sender<Vec<i16>>> = Some(audio_tx);
        let mut event_rx: Option<mpsc::Receiver<TranslationEvent>> = Some(event_rx);
        let mut api_connected = true;

        // Audio processing state
        let mut resampler: Option<AudioResampler> = None;
        let mut channels: u16;
        let mut chunk_size_interleaved: usize = 0;
        let mut audio_buffer: Vec<f32> = Vec::new();

        // Silence detection with hysteresis to prevent repeated speech during pauses.
        const SILENCE_RMS_THRESHOLD: f32 = 0.01;
        const SILENCE_WAKE_THRESHOLD: f32 = 0.02;
        const SILENCE_SUSTAIN_FRAMES: u64 = 30;
        // Auto-pause: disconnect API after 60s of sustained silence to save tokens.
        // 60s × 50fps = 3000 frames. Reconnects automatically when speech resumes.
        const AUTO_PAUSE_FRAMES: u64 = 3000;
        const ECHO_COOLDOWN_MS: i64 = 150;
        let mut silence_frames: u64 = 0;
        let mut in_silence = false;

        // Text-based dedup: track recent finalized texts to filter repeats.
        let mut recent_sources: VecDeque<String> = VecDeque::with_capacity(16);
        let mut recent_translations: VecDeque<String> = VecDeque::with_capacity(16);
        const DEDUP_WINDOW: usize = 15;

        let is_duplicate = |text: &str, recent: &VecDeque<String>| -> bool {
            if recent.iter().any(|r| r == text) {
                return true;
            }
            if recent.len() >= 2 {
                for start in 0..recent.len() {
                    let mut concat = String::new();
                    for i in start..recent.len() {
                        concat.push_str(&recent[i]);
                        if concat.len() > text.len() {
                            break;
                        }
                        if concat == text && i > start {
                            return true;
                        }
                    }
                }
            }
            false
        };

        loop {
            if api_connected {
                // === Connected mode: process audio + handle API events ===
                let mut need_auto_pause = false;
                let mut need_break = false;
                let mut send_failed = false;

                {
                    // Borrow event_rx for the select block; released after the block
                    let erx = event_rx.as_mut().unwrap();

                    tokio::select! {
                        frame_opt = audio_rx.recv() => {
                            if let Some(frame) = frame_opt {
                                // Initialize resampler on first frame
                                if resampler.is_none() {
                                    channels = frame.channels;
                                    match AudioResampler::new(frame.sample_rate, frame.channels) {
                                        Ok(r) => {
                                            chunk_size_interleaved = r.input_frames_next() * channels as usize;
                                            audio_buffer.reserve(chunk_size_interleaved * 2);
                                            info!(
                                                "Resampler initialized: {}Hz {}ch, chunk_size={}",
                                                frame.sample_rate, channels, chunk_size_interleaved
                                            );
                                            resampler = Some(r);
                                        }
                                        Err(e) => {
                                            error!("Resampler error: {}", e);
                                            on_sub.send(SubtitleEvent::Error {
                                                message: format!("resample_failed:{}", e),
                                            }).ok();
                                            need_break = true;
                                        }
                                    }
                                }

                                if !need_break {
                                    // Silence detection
                                    let rms: f32 = if frame.samples.is_empty() {
                                        0.0
                                    } else {
                                        (frame.samples.iter().map(|s| s * s).sum::<f32>() / frame.samples.len() as f32).sqrt()
                                    };

                                    let effective_threshold = if in_silence { SILENCE_WAKE_THRESHOLD } else { SILENCE_RMS_THRESHOLD };

                                    if rms < effective_threshold {
                                        silence_frames += 1;
                                        if silence_frames == SILENCE_SUSTAIN_FRAMES {
                                            in_silence = true;
                                            debug!("Audio silence detected (sustained), suppressing frames");
                                        }
                                    } else {
                                        if in_silence {
                                            debug!("Audio resumed after {} silent frames (rms={:.4})", silence_frames, rms);
                                        }
                                        silence_frames = 0;
                                        in_silence = false;
                                    }

                                    // Auto-pause: disconnect API after 60s sustained silence
                                    if in_silence && silence_frames >= AUTO_PAUSE_FRAMES {
                                        need_auto_pause = true;
                                    } else {
                                        // Echo suppression
                                        let last_tts = echo_suppression.load(Ordering::Relaxed);
                                        let now = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis() as i64;
                                        let tts_active = last_tts > 0 && (now - last_tts) < ECHO_COOLDOWN_MS;

                                        let resampler = resampler.as_mut().unwrap();

                                        if tts_active || in_silence {
                                            audio_buffer.extend(std::iter::repeat(0.0f32).take(frame.samples.len()));
                                        } else {
                                            audio_buffer.extend_from_slice(&frame.samples);
                                        }

                                        // Resample and send to API
                                        while audio_buffer.len() >= chunk_size_interleaved {
                                            let chunk: Vec<f32> = audio_buffer.drain(..chunk_size_interleaved).collect();
                                            match resampler.process(&chunk) {
                                                Ok(pcm16) => {
                                                    if let Some(ref tx) = audio_tx {
                                                        let mut offset = 0;
                                                        while offset < pcm16.len() {
                                                            let end = (offset + 1280).min(pcm16.len());
                                                            if tx.send(pcm16[offset..end].to_vec()).await.is_err() {
                                                                send_failed = true;
                                                                break;
                                                            }
                                                            offset = end;
                                                        }
                                                    }
                                                }
                                                Err(e) => warn!("Resample: {}", e),
                                            }
                                            if send_failed { break; }
                                        }
                                    }
                                }
                            } else {
                                // Audio capture ended
                                need_break = true;
                            }
                        }
                        event = erx.recv() => {
                            match event {
                                Some(TranslationEvent::SourceSubtitle { text, is_final, .. }) => {
                                    if !text.is_empty() {
                                        if is_final {
                                            if is_duplicate(&text, &recent_sources) {
                                                debug!("Skipping duplicate source: {}", text);
                                            } else {
                                                recent_sources.push_back(text.clone());
                                                if recent_sources.len() > DEDUP_WINDOW {
                                                    recent_sources.pop_front();
                                                }
                                                on_sub.send(SubtitleEvent::Source { text, is_final }).ok();
                                            }
                                        } else {
                                            on_sub.send(SubtitleEvent::Source { text, is_final }).ok();
                                        }
                                    }
                                }
                                Some(TranslationEvent::TranslationSubtitle { text, is_final, .. }) => {
                                    if !text.is_empty() {
                                        if is_final {
                                            if is_duplicate(&text, &recent_translations) {
                                                debug!("Skipping duplicate translation: {}", text);
                                            } else {
                                                recent_translations.push_back(text.clone());
                                                if recent_translations.len() > DEDUP_WINDOW {
                                                    recent_translations.pop_front();
                                                }
                                                on_sub.send(SubtitleEvent::Translation { text, is_final }).ok();
                                            }
                                        } else {
                                            on_sub.send(SubtitleEvent::Translation { text, is_final }).ok();
                                        }
                                    }
                                }
                                Some(TranslationEvent::TtsAudio { data }) => {
                                    if let Some(ref player) = tts_player {
                                        debug!("TTS audio chunk: {} bytes", data.len());
                                        player.play_pcm_bytes(&data);
                                    }
                                }
                                Some(TranslationEvent::SessionFailed { message }) => {
                                    on_sub.send(SubtitleEvent::Error { message }).ok();
                                    need_break = true;
                                }
                                Some(TranslationEvent::SessionFinished) => {
                                    on_sub.send(SubtitleEvent::Status { message: "session_ended".to_string() }).ok();
                                    need_break = true;
                                }
                                None => {
                                    on_sub.send(SubtitleEvent::Error {
                                        message: "disconnected".to_string(),
                                    }).ok();
                                    need_break = true;
                                }
                                _ => {}
                            }
                        }
                        _ = stop_rx.recv() => {
                            info!("Stop signal received");
                            on_sub.send(SubtitleEvent::Status { message: "stopped".to_string() }).ok();
                            need_break = true;
                        }
                    }
                } // event_rx borrow released here

                if need_break {
                    break;
                }

                if need_auto_pause || send_failed {
                    // Disconnect API, enter saving mode
                    audio_tx = None;
                    event_rx = None;
                    api_connected = false;
                    audio_buffer.clear();
                    if need_auto_pause {
                        info!("Auto-pause: 60s sustained silence, disconnecting API to save tokens");
                        on_sub.send(SubtitleEvent::Status {
                            message: "auto_paused".to_string(),
                        }).ok();
                    } else {
                        info!("API send failed, entering disconnected mode");
                    }
                }
            } else {
                // === Disconnected mode: monitor audio, wait for speech to reconnect ===
                tokio::select! {
                    frame = audio_rx.recv() => {
                        let Some(frame) = frame else { break; };

                        let rms: f32 = if frame.samples.is_empty() {
                            0.0
                        } else {
                            (frame.samples.iter().map(|s| s * s).sum::<f32>() / frame.samples.len() as f32).sqrt()
                        };

                        if rms >= SILENCE_WAKE_THRESHOLD {
                            // Speech detected — reconnect API
                            info!("Speech detected after auto-pause (rms={:.4}), reconnecting...", rms);
                            on_sub.send(SubtitleEvent::Status {
                                message: "reconnecting".to_string(),
                            }).ok();

                            let new_config = SessionConfig {
                                app_key: cfg_app_key.clone(),
                                access_key: cfg_access_key.clone(),
                                resource_id: "volc.service_type.10053".to_string(),
                                connection_id: uuid::Uuid::new_v4().to_string(),
                                session_id: uuid::Uuid::new_v4().to_string(),
                                mode: cfg_mode.clone(),
                                source_language: cfg_source_lang.clone(),
                                target_language: cfg_target_lang.clone(),
                                speaker_id: cfg_speaker_id.clone(),
                            };

                            let new_client = TranslationClient::new(new_config);
                            match new_client.connect().await.map_err(|e| e.to_string()) {
                                Ok((new_tx, mut new_rx)) => {
                                    match timeout(Duration::from_secs(10), new_rx.recv()).await {
                                        Ok(Some(TranslationEvent::SessionStarted)) => {
                                            audio_tx = Some(new_tx);
                                            event_rx = Some(new_rx);
                                            api_connected = true;
                                            silence_frames = 0;
                                            in_silence = false;
                                            audio_buffer.clear();
                                            info!("Auto-reconnect successful");
                                            on_sub.send(SubtitleEvent::Status {
                                                message: if enable_tts {
                                                    "reconnected_tts".to_string()
                                                } else {
                                                    "reconnected_text".to_string()
                                                },
                                            }).ok();
                                        }
                                        Ok(Some(TranslationEvent::SessionFailed { message })) => {
                                            warn!("Reconnect failed: {}", message);
                                            on_sub.send(SubtitleEvent::Error {
                                                message: format!("reconnect_failed:{}", message),
                                            }).ok();
                                            break;
                                        }
                                        _ => {
                                            warn!("Reconnect timeout");
                                            on_sub.send(SubtitleEvent::Error {
                                                message: "reconnect_timeout".to_string(),
                                            }).ok();
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Reconnect error: {}", e);
                                    on_sub.send(SubtitleEvent::Error {
                                        message: format!("reconnect_failed:{}", e),
                                    }).ok();
                                    break;
                                }
                            }
                        }
                    }
                    _ = stop_rx.recv() => {
                        info!("Stop signal received");
                        on_sub.send(SubtitleEvent::Status { message: "stopped".to_string() }).ok();
                        break;
                    }
                }
            }
        }

        // Cleanup
        if let Some(vol) = original_volume {
            playback::set_system_volume(vol);
        }
        drop(tts_player);
        drop(audio_tx);
        capture_handle.stop().ok();
        *app_handle.state::<AppState>().stop_tx.lock().await = None;
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_interpretation(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut stop_tx = state.stop_tx.lock().await;
    if let Some(tx) = stop_tx.take() {
        tx.send(()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
