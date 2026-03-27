# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TransEcho is a cross-platform (macOS + Windows) desktop real-time simultaneous interpretation (同声传译) application built with Tauri 2.x. It captures system audio, sends it to Volcengine's AST API over WebSocket, and displays source/translation subtitles in real-time with optional TTS playback.

## Build & Development Commands

```bash
# Development (starts both frontend dev server and Rust backend)
npm run tauri dev

# Production build (current platform)
npm run tauri build

# Frontend only
npm run dev          # Vite dev server on port 1420
npm run build        # Build frontend
npm run check        # svelte-check type checking

# Rust backend only (from src-tauri/)
cargo build
cargo check
cargo test           # Run codec tests
```

Protobuf compilation requires `protoc` installed (`brew install protobuf` on macOS).

## Architecture

```
Frontend (Svelte 5 / SvelteKit)     Backend (Rust / Tokio)
┌─────────────────────┐    IPC     ┌──────────────────────────┐
│ src/routes/          │◄─────────►│ commands.rs              │
│  +page.svelte (SPA)  │  Channel  │  start/stop_interpretation│
└─────────────────────┘           ├──────────────────────────┤
                                   │ audio/                   │
                                   │  capture_macos.rs (SCK)  │
                                   │  capture_windows.rs(WASAPI)│
                                   │  resample.rs (48k→16k)   │
                                   │  playback.rs (TTS/Rodio) │
                                   ├──────────────────────────┤
                                   │ transport/               │
                                   │  client.rs  (WebSocket)  │
                                   │  codec.rs   (Protobuf)   │
                                   └──────────────────────────┘
                                              │
                                   Volcengine AST API (wss://)
```

**Data flow**: System audio (48kHz stereo f32) → silence detection (RMS hysteresis) → echo suppression (TTS timestamp cooldown) → resample (16kHz mono i16) → WebSocket → Volcengine AST → protobuf response → SubtitleEvent via Tauri Channel → frontend display. TTS audio (24kHz f32) from API is played via Rodio with jitter buffer.

**Key design decisions**:
- Single-page Svelte app with SSR disabled (`adapter-static`, `ssr = false`)
- Protobuf definitions compiled at build time via `prost-build` in `build.rs`
- Audio pipeline uses `tokio::sync::mpsc` channels between capture/resample/transport stages
- Deduplication: backend tracks last 15 finalized texts (exact + concatenation match), frontend tracks last 10 subtitle pairs
- Two API modes: `s2t` (text only) and `s2s` (text + TTS audio at 24kHz)

## Key Modules

| Module | Role |
|--------|------|
| `src-tauri/src/commands.rs` | Tauri IPC commands, session orchestration, silence/echo/dedup logic |
| `src-tauri/src/audio/capture_macos.rs` | macOS ScreenCaptureKit audio capture |
| `src-tauri/src/audio/capture_windows.rs` | Windows WASAPI loopback audio capture (cpal) |
| `src-tauri/src/audio/resample.rs` | Rubato FFT resampler (48kHz stereo → 16kHz mono) |
| `src-tauri/src/audio/playback.rs` | Rodio streaming TTS playback with jitter buffer, echo suppression timestamp |
| `src-tauri/src/transport/client.rs` | WebSocket client to Volcengine AST API (wss://openspeech.bytedance.com) |
| `src-tauri/src/transport/codec.rs` | Protobuf encode/decode, SessionConfig, TranslationEvent |
| `src-tauri/proto/` | Protobuf definitions for Volcengine AST protocol |
| `src/routes/+page.svelte` | Entire frontend UI (settings, subtitles, controls) |

## Platform-Specific Details

- **macOS**: ScreenCaptureKit for audio capture (macOS 14.0+), requires screen recording permission. Audio capture excludes current process audio via `.with_excludes_current_process_audio(true)`.
- **Windows**: WASAPI loopback via cpal. Captures ALL system audio including app's own TTS output, hence the echo suppression mechanism.
- Audio capture module is selected at compile time via `#[cfg(target_os)]` in `audio/mod.rs`.
- Platform string sent to API is mapped: `std::env::consts::OS` "macos" → "macOS" for API compatibility.

## Audio Pipeline Internals

- **Silence detection**: RMS threshold 0.01 (normal) / 0.02 (wake from silence, hysteresis). Sustained silence after 30 frames.
- **Echo suppression**: `AtomicI64` timestamp shared between TTS playback thread and capture pipeline. 150ms cooldown covers WASAPI device buffer latency.
- **Auto-pause**: After 60s sustained silence, auto-disconnects API to save tokens. Reconnects automatically when speech resumes (~1-2s delay). Short pauses (<60s) send zero frames for instant resume.
- **Channel buffer sizes**: capture→pipeline: 50 frames, pipeline→API: 100 frames, TTS audio: 50 chunks.
- **Unified task**: Audio pipeline and event loop are merged into a single tokio task using `tokio::select!`, enabling session lifecycle management (connect/disconnect/reconnect).

## CI/CD & Release

- GitHub Actions (`.github/workflows/build.yml`) triggers on `v*` tags
- CI builds Windows (msi + nsis exe) artifacts only; macOS CI build fails (no signing certificate)
- **macOS must be built locally** because it requires Developer ID signing + Apple notarization (Developer ID: INAGORA CO., LTD.), which needs local keychain access

### Release workflow

```bash
# 1. Bump version in all 3 files
#    - src-tauri/Cargo.toml, src-tauri/tauri.conf.json, package.json

# 2. Commit, tag, push
git commit -am "Bump version to v0.1.x"
git push origin main
git tag v0.1.x && git push origin v0.1.x   # triggers Windows CI

# 3. Build macOS locally (requires signing identity in keychain)
npm run tauri build

# 4. Wait for Windows CI, download artifacts
gh run download <run-id> -n windows-installer -D /tmp/win

# 5. Create GitHub Release with all artifacts
gh release create v0.1.x \
  src-tauri/target/release/bundle/dmg/TransEcho_0.1.x_aarch64.dmg \
  /tmp/win/msi/TransEcho_0.1.x_x64_en-US.msi \
  /tmp/win/nsis/TransEcho_0.1.x_x64-setup.exe \
  --title "v0.1.x" --notes "..."
```

Steps 3 and 4 can run in parallel (local macOS build + CI Windows build).
