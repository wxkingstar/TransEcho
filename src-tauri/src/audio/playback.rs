use rodio::{OutputStream, Sink, Source};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// A streaming audio source with jitter buffer for smooth TTS playback.
///
/// Key design: next() is NEVER blocking. The audio callback thread must not
/// be starved — if no data is available, we immediately return silence.
/// A separate pre-buffering phase absorbs initial network jitter.
struct StreamingSource {
    rx: std_mpsc::Receiver<Vec<f32>>,
    buffer: VecDeque<Vec<f32>>,
    current_chunk: Vec<f32>,
    pos: usize,
    sample_rate: u32,
    initialized: bool,
    /// Shared flag: true while real (non-silence) audio is being played.
    /// Used by the capture pipeline to suppress loopback echo on Windows.
    is_playing: Arc<AtomicBool>,
}

impl StreamingSource {
    /// Drain all immediately available chunks into the internal buffer (non-blocking)
    fn drain_available(&mut self) {
        while let Ok(chunk) = self.rx.try_recv() {
            self.buffer.push_back(chunk);
        }
    }

    /// Pre-buffer audio data before starting playback.
    /// Waits up to 500ms to accumulate ~200ms worth of audio samples.
    /// This is the ONLY place where blocking is allowed (before playback starts).
    fn initialize(&mut self) {
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    self.buffer.push_back(chunk);
                    let total_samples: usize = self.buffer.iter().map(|c| c.len()).sum();
                    if total_samples >= (self.sample_rate as usize / 5) {
                        break;
                    }
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.initialized = true;
        debug!(
            "TTS jitter buffer initialized with {} chunks ({} samples)",
            self.buffer.len(),
            self.buffer.iter().map(|c| c.len()).sum::<usize>()
        );
    }
}

impl Iterator for StreamingSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // Pre-buffer on first call (blocking allowed here, before playback)
        if !self.initialized {
            self.initialize();
        }

        // Fast path: advance within current chunk
        if self.pos < self.current_chunk.len() {
            let sample = self.current_chunk[self.pos];
            self.pos += 1;
            return Some(sample);
        }

        // Current chunk exhausted — eagerly drain all available chunks (non-blocking)
        self.drain_available();

        // Get next chunk from internal buffer
        if let Some(chunk) = self.buffer.pop_front() {
            self.is_playing.store(true, Ordering::Relaxed);
            self.current_chunk = chunk;
            self.pos = 1;
            return Some(self.current_chunk[0]);
        }

        // Buffer empty — try once more (NON-BLOCKING, never stall the audio thread)
        match self.rx.try_recv() {
            Ok(chunk) => {
                self.is_playing.store(true, Ordering::Relaxed);
                self.drain_available();
                self.current_chunk = chunk;
                self.pos = 1;
                Some(self.current_chunk[0])
            }
            Err(std_mpsc::TryRecvError::Empty) => {
                // No data available — TTS is idle, clear the playing flag so
                // the capture pipeline resumes normal audio forwarding.
                self.is_playing.store(false, Ordering::Relaxed);
                let silence_samples = (self.sample_rate as usize * 10) / 1000;
                self.current_chunk = vec![0.0; silence_samples];
                self.pos = 1;
                Some(0.0)
            }
            Err(std_mpsc::TryRecvError::Disconnected) => {
                self.is_playing.store(false, Ordering::Relaxed);
                None
            }
        }
    }
}

impl Source for StreamingSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Handle to send TTS audio data. This is Send + Sync safe.
#[derive(Clone)]
pub struct TtsHandle {
    tx: std_mpsc::SyncSender<Vec<f32>>,
    /// Shared flag indicating TTS is actively playing audio.
    /// The capture pipeline checks this to suppress loopback echo on Windows.
    is_playing: Arc<AtomicBool>,
}

impl TtsHandle {
    /// Start a TTS player on a dedicated thread. Returns a Send-safe handle.
    ///
    /// Uses a bounded channel (capacity 50) to prevent unbounded memory growth.
    /// Waits for the audio thread to confirm device initialization before returning,
    /// ensuring the handle is truly ready to play audio.
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = std_mpsc::sync_channel::<Vec<f32>>(50);
        let is_playing = Arc::new(AtomicBool::new(false));
        let is_playing_clone = is_playing.clone();
        // Oneshot channel to confirm audio device initialization
        let (ready_tx, ready_rx) = std_mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::spawn(move || {
            let (_stream, stream_handle) = match OutputStream::try_default() {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("Failed to open audio output: {}", e)));
                    return;
                }
            };

            let sink = match Sink::try_new(&stream_handle) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("Failed to create audio sink: {}", e)));
                    return;
                }
            };

            // Signal ready AFTER both stream and sink are successfully created
            let _ = ready_tx.send(Ok(()));

            sink.set_volume(1.0);

            let source = StreamingSource {
                rx,
                buffer: VecDeque::new(),
                current_chunk: Vec::new(),
                pos: 0,
                sample_rate: 24000,
                initialized: false,
                is_playing: is_playing_clone,
            };

            sink.append(source);
            info!("TTS player thread started (24kHz mono f32 PCM, jitter buffer enabled)");
            sink.sleep_until_end();
            info!("TTS player thread ended");
        });

        // Wait for audio thread to confirm device initialization (up to 3 seconds)
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self { tx, is_playing }),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err("TTS 播放器初始化超时".into()),
        }
    }

    /// Returns a shared flag that is true while TTS is actively playing audio.
    /// Used by the capture pipeline for echo suppression on Windows.
    pub fn is_playing_flag(&self) -> Arc<AtomicBool> {
        self.is_playing.clone()
    }

    /// Feed raw PCM bytes (32-bit float LE, 24kHz mono) to the player.
    /// Non-blocking: if the buffer is full, the chunk is dropped to maintain liveness.
    pub fn play_pcm_bytes(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let samples: Vec<f32> = data
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        if self.tx.try_send(samples).is_err() {
            warn!("TTS buffer full, dropping audio chunk");
        }
    }
}

/// Lower system volume to the specified percentage (0-100)
#[cfg(target_os = "macos")]
pub fn set_system_volume(percent: u32) {
    let vol = percent.min(100);
    let _ = std::process::Command::new("osascript")
        .args(["-e", &format!("set volume output volume {}", vol)])
        .output();
    info!("System volume set to {}%", vol);
}

#[cfg(target_os = "windows")]
pub fn set_system_volume(_percent: u32) {
    // Windows volume control not implemented yet
}

/// Get current system volume (0-100)
#[cfg(target_os = "macos")]
pub fn get_system_volume() -> u32 {
    let output = std::process::Command::new("osascript")
        .args(["-e", "output volume of (get volume settings)"])
        .output();

    match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            s.parse::<u32>().unwrap_or(50)
        }
        Err(_) => 50,
    }
}

#[cfg(target_os = "windows")]
pub fn get_system_volume() -> u32 {
    50 // Default fallback on Windows
}
