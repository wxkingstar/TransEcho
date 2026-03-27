# 社区发帖文案

---

## V2EX（发到 /t/share 或 /t/macos 节点）

**标题**: TransEcho - 开源的 macOS 实时同声传译，看直播/开会再也不怕听不懂了

**正文**:

做了一个 macOS 桌面端的实时同声传译工具，开源免费，分享给大家。

**解决什么问题？**

看日语直播、参加英语会议、追生肉番... 听不懂的场景太多了。TransEcho 直接捕获 Mac 的系统音频，实时翻译成字幕，还能语音播报翻译结果，体验接近真人同传。

**核心特点**:
- 系统音频捕获：基于 ScreenCaptureKit，不需要装虚拟声卡，任意 App 的声音都能抓
- 实时翻译：基于豆包同声传译 2.0 大模型，延迟很低
- 语音同传：翻译结果可以同步语音播报
- 8 语言互译：中/英/日/德/法/西/葡/印尼语
- 原生性能：Rust 后端 + Svelte 前端，内存占用很低
- 免费起步：豆包 API 新用户送 100 万 token，够用很久

**技术栈**: Tauri 2 + Rust + Svelte 5 + ScreenCaptureKit + Protobuf

**截图**: [贴 screenshot.png]

GitHub: https://github.com/wxkingstar/TransEcho
下载: https://github.com/wxkingstar/TransEcho/releases/tag/v0.1.0

目前只支持 macOS 14+ (Apple Silicon)，欢迎试用和 PR！

---

## 掘金

**标题**: 开源 | TransEcho - 用 Rust + Tauri 2 打造 macOS 实时同声传译应用

**正文**:

## 前言

经常看日语直播和英文技术分享，一直想要一个能实时翻译系统音频的工具。市面上的方案要么需要装虚拟声卡，要么延迟太高，要么收费。于是自己动手做了一个——**TransEcho**，现在开源出来。

## 效果展示

[贴 screenshot.png]

日语音频 → 中文字幕，实时翻译，延迟极低。

## 核心功能

| 功能 | 说明 |
|------|------|
| 系统音频捕获 | 基于 ScreenCaptureKit，无需虚拟声卡 |
| 实时同传 | 豆包同声传译 2.0 大模型驱动 |
| 语音播报 | TTS 同步播报翻译结果 |
| 多语言 | 支持 8 种语言互译 |

## 技术架构

```
系统音频 (48kHz stereo f32)
    → Rubato 重采样 (16kHz mono i16)
    → WebSocket 发送
    → 豆包 ASR 识别 + 翻译
    → Protobuf 响应解析
    → Tauri Channel 推送前端
    → 实时字幕展示 + TTS 播报
```

**技术栈选择**:
- **Tauri 2** 而非 Electron：包体小、内存低、原生体验
- **Rust + Tokio**：异步音频管线，高性能低延迟
- **Svelte 5**：响应式 UI，编译后体积极小
- **ScreenCaptureKit**：macOS 原生 API，无需第三方驱动
- **Protobuf**：与火山引擎 API 高效通信

## 如何使用

1. 从 [Release](https://github.com/wxkingstar/TransEcho/releases) 下载 DMG
2. 在[火山引擎](https://console.volcengine.com/speech/service/10030)免费申请 API 凭证（送 100 万 token）
3. 打开 TransEcho，填入凭证，开始同传

## 开源地址

GitHub: https://github.com/wxkingstar/TransEcho

MIT 协议，欢迎 Star 和 PR！

---

## 少数派

**标题**: TransEcho：一款开源免费的 macOS 实时同声传译工具

**正文**:

我经常看日语直播和英文播客，一直希望有一个工具能把 Mac 上正在播放的音频实时翻译出来。试过一些方案，不是需要装虚拟声卡就是延迟太高。最终决定自己做一个，就有了 TransEcho。

### 它能做什么？

打开 TransEcho，点击「开始同传」，它会捕获你 Mac 上正在播放的任何音频（YouTube、Zoom、播客、Netflix...），实时翻译成你的语言，以字幕形式展示。你还可以开启语音同传，让它把翻译结果读出来。

### 实际体验

[贴 screenshot.png]

上图是我在看一个日语直播时的效果。上面灰色小字是日语原文识别，下面白色大字是中文翻译。翻译延迟大约在 1-2 秒，基本能跟上说话节奏。

### 几个亮点

- **不需要虚拟声卡**：直接用 macOS 的 ScreenCaptureKit 捕获系统音频
- **免费额度充足**：翻译引擎用的是豆包同声传译 2.0，新用户送 100 万 token
- **支持 8 种语言**：中英日德法西葡印尼语任意互译
- **体积小、性能好**：用 Rust 写的，不是 Electron 那种大块头

### 下载使用

- GitHub: https://github.com/wxkingstar/TransEcho
- 安装包: https://github.com/wxkingstar/TransEcho/releases

需要 macOS 14 以上，Apple Silicon Mac。

开源项目，MIT 协议，欢迎反馈。

---

## Reddit (r/rust)

**Title**: TransEcho - Open-source real-time simultaneous interpretation for macOS, built with Rust + Tauri 2

**Body**:

I built a macOS desktop app that captures system audio and translates it in real-time. It's useful for watching foreign livestreams, joining multilingual meetings, or consuming content in languages you don't speak fluently.

**How it works:**
- Captures system audio via ScreenCaptureKit (no virtual audio driver needed)
- Resamples with Rubato (48kHz → 16kHz)
- Streams via WebSocket to Doubao AST 2.0 LLM for real-time translation
- Displays subtitles + optional TTS playback via Rodio

**Tech stack:**
- Tauri 2 + Rust + Tokio (async audio pipeline)
- Svelte 5 frontend
- Protobuf (prost) for API communication
- ScreenCaptureKit for native macOS audio capture

Supports 8 languages (Chinese, English, Japanese, German, French, Spanish, Portuguese, Indonesian).

GitHub: https://github.com/wxkingstar/TransEcho

MIT licensed. Would love feedback on the Rust side — especially the audio pipeline and WebSocket handling.

---

## Twitter/X

TransEcho - 开源的 macOS 实时同声传译 🎙️

看日语直播、开英语会议，系统音频实时翻译成字幕 + 语音同传。

✅ 不需要虚拟声卡
✅ 豆包大模型驱动，延迟极低
✅ 8 语言互译
✅ Rust + Tauri 2，性能拉满
✅ 免费 100 万 token

GitHub → https://github.com/wxkingstar/TransEcho

[附截图]

#开源 #macOS #Rust #Tauri #同声传译 #AI
