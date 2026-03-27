use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
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
            message: "正在连接 API...".to_string(),
        })
        .ok();

    let client = TranslationClient::new(config);
    // map_err converts Box<dyn Error> (non-Send) to String before any await
    let connect_result = client.connect().await.map_err(|e| e.to_string());
    let (audio_tx, mut event_rx) = match connect_result {
        Ok(v) => v,
        Err(err_msg) => {
            *state.stop_tx.lock().await = None;
            return Err(err_msg);
        }
    };

    // Wait for SessionStarted with timeout to prevent hanging forever
    match timeout(Duration::from_secs(10), event_rx.recv()).await {
        Err(_) => {
            *state.stop_tx.lock().await = None;
            return Err("API 连接超时 (10秒未响应)".to_string());
        }
        Ok(response) => match response {
            Some(TranslationEvent::SessionStarted) => {
                on_subtitle
                    .send(SubtitleEvent::Status {
                        message: if enable_tts {
                            "API 已连接 (语音+字幕)".to_string()
                        } else {
                            "API 已连接 (字幕)".to_string()
                        },
                    })
                    .ok();
            }
            Some(TranslationEvent::SessionFailed { message }) => {
                *state.stop_tx.lock().await = None;
                return Err(format!("API 连接失败: {}", message));
            }
            _ => {
                *state.stop_tx.lock().await = None;
                return Err("意外的 API 响应".to_string());
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
            message: "开始同传".to_string(),
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
                        message: "语音播放初始化失败，仅显示字幕".to_string(),
                    })
                    .ok();
                None
            }
        }
    } else {
        None
    };

    // Spawn audio pipeline: capture → resample → send to API
    let audio_sender = audio_tx.clone();
    let pipeline_on_sub = on_subtitle.clone();
    tokio::spawn(async move {
        // Defer resampler init until first frame so we can read actual sample_rate/channels
        let mut resampler: Option<AudioResampler> = None;
        let mut channels: u16;
        let mut chunk_size_interleaved: usize = 0;
        let mut audio_buffer: Vec<f32> = Vec::new();

        // Silence detection with hysteresis to prevent repeated speech during pauses.
        // macOS ScreenCaptureKit delivers near-zero silence, but Windows WASAPI
        // loopback has a higher noise floor (~0.002-0.01), so the threshold must
        // be high enough for both platforms.
        const SILENCE_RMS_THRESHOLD: f32 = 0.01;
        // After sustained silence, require a higher RMS to "wake up" (hysteresis
        // prevents chattering when noise hovers near the threshold).
        const SILENCE_WAKE_THRESHOLD: f32 = 0.02;
        const SILENCE_SUSTAIN_FRAMES: u64 = 30;
        let mut silence_frames: u64 = 0;
        let mut in_silence = false;

        while let Some(frame) = audio_rx.recv().await {
            // Initialize resampler on first frame using actual capture format
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
                        pipeline_on_sub
                            .send(SubtitleEvent::Error {
                                message: format!("音频重采样初始化失败: {}", e),
                            })
                            .ok();
                        return;
                    }
                }
            }

            // Silence detection: skip frames that are effectively silent
            let rms: f32 = if frame.samples.is_empty() {
                0.0
            } else {
                (frame.samples.iter().map(|s| s * s).sum::<f32>() / frame.samples.len() as f32).sqrt()
            };

            // Use hysteresis: once in sustained silence, require higher energy to resume
            let effective_threshold = if in_silence { SILENCE_WAKE_THRESHOLD } else { SILENCE_RMS_THRESHOLD };

            if rms < effective_threshold {
                silence_frames += 1;
                if silence_frames == SILENCE_SUSTAIN_FRAMES {
                    in_silence = true;
                    debug!("Audio silence detected (sustained), suppressing frames");
                }
                continue;
            }

            if silence_frames > 0 {
                if in_silence {
                    debug!("Audio resumed after {} silent frames (rms={:.4})", silence_frames, rms);
                }
                silence_frames = 0;
                in_silence = false;
            }

            let resampler = resampler.as_mut().unwrap();
            audio_buffer.extend_from_slice(&frame.samples);

            while audio_buffer.len() >= chunk_size_interleaved {
                let chunk: Vec<f32> = audio_buffer.drain(..chunk_size_interleaved).collect();
                match resampler.process(&chunk) {
                    Ok(pcm16) => {
                        let mut offset = 0;
                        while offset < pcm16.len() {
                            let end = (offset + 1280).min(pcm16.len());
                            if audio_sender.send(pcm16[offset..end].to_vec()).await.is_err() {
                                return;
                            }
                            offset = end;
                        }
                    }
                    Err(e) => warn!("Resample: {}", e),
                }
            }
        }

        // Flush residual audio samples
        if !audio_buffer.is_empty() {
            if let Some(resampler) = resampler.as_mut() {
                match resampler.process(&audio_buffer) {
                    Ok(pcm16) => {
                        if !pcm16.is_empty() {
                            let _ = audio_sender.send(pcm16).await;
                        }
                    }
                    Err(e) => debug!("Flush resample: {}", e),
                }
            }
        }
    });

    // Spawn event loop: forward translation events to frontend + play TTS
    let on_sub = on_subtitle.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        // Drop guard ensures stop_tx is cleared even if this task panics
        let _cleanup = CleanupGuard {
            app_handle: app_handle.clone(),
        };

        // Text-based dedup: track recent finalized texts to filter repeats
        let mut recent_sources: VecDeque<String> = VecDeque::with_capacity(16);
        let mut recent_translations: VecDeque<String> = VecDeque::with_capacity(16);
        const DEDUP_WINDOW: usize = 15;

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(TranslationEvent::SourceSubtitle { text, is_final, .. }) => {
                            if !text.is_empty() {
                                if is_final {
                                    if recent_sources.contains(&text) {
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
                                    if recent_translations.contains(&text) {
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
                            } else {
                                debug!("TTS audio received but no player");
                            }
                        }
                        Some(TranslationEvent::SessionFailed { message }) => {
                            on_sub.send(SubtitleEvent::Error { message }).ok();
                            break;
                        }
                        Some(TranslationEvent::SessionFinished) => {
                            on_sub.send(SubtitleEvent::Status { message: "会话结束".to_string() }).ok();
                            break;
                        }
                        None => {
                            // All event senders dropped — connection lost
                            on_sub.send(SubtitleEvent::Error {
                                message: "连接已断开".to_string(),
                            }).ok();
                            break;
                        }
                        _ => {}
                    }
                }
                _ = stop_rx.recv() => {
                    info!("Stop signal received");
                    on_sub.send(SubtitleEvent::Status { message: "已停止".to_string() }).ok();
                    break;
                }
            }
        }

        // Cleanup: restore system volume if changed
        if let Some(vol) = original_volume {
            playback::set_system_volume(vol);
        }
        drop(tts_player);
        drop(audio_tx);
        capture_handle.stop().ok();
        // Explicit cleanup (also done by CleanupGuard as safety net)
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
