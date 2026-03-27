# TransEcho - 实时同声传译

<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="TransEcho Logo">
</p>

<p align="center">
  <strong>捕获系统音频，实时同声传译，所听即所译</strong>
</p>

<p align="center">
  <a href="#功能特性">功能特性</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#使用场景">使用场景</a> ·
  <a href="#技术架构">技术架构</a> ·
  <a href="README_EN.md">English</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%20|%20Windows-blue" alt="Platform">
  <img src="https://img.shields.io/badge/built%20with-Tauri%202-yellow" alt="Tauri">
  <img src="https://img.shields.io/badge/lang-Rust%20%2B%20Svelte-orange" alt="Stack">
  <img src="https://img.shields.io/github/license/wxkingstar/TransEcho" alt="License">
</p>

---

> 看日语直播听不懂？看英文会议跟不上？TransEcho 直接捕获 macOS 系统音频，实时翻译成你的语言，字幕 + 语音同传，零延迟体验。

## 截图预览

<p align="center">
  <img src="docs/screenshot.png" width="420" alt="TransEcho 实时同声传译截图">
</p>

## 功能特性

- **系统音频捕获** - 基于 ScreenCaptureKit，捕获任意应用的音频输出，无需虚拟声卡
- **实时同声传译** - 基于豆包同声传译 2.0 大模型，延迟极低
- **语音同传** - 翻译结果可同步语音播报（TTS），真正的"同声传译"体验
- **8 语言互译** - 支持中/英/日/德/法/西/葡/印尼语
- **原生性能** - Rust 后端 + Svelte 前端，内存占用极低
- **免费起步** - 豆包 API 新用户赠送 100 万 token，足够长时间使用

## 使用场景

| 场景 | 说明 |
|------|------|
| 看海外直播 | YouTube / Twitch / 日本直播，实时字幕翻译 |
| 远程会议 | Zoom / Teams / Google Meet 跨语言会议同传 |
| 学习外语 | 听外语播客/视频，对照原文和翻译 |
| 看番追剧 | 无字幕生肉也能看懂 |

## 快速开始

### 前置条件

- macOS 14.0+
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) 1.70+
- 豆包同声传译 API 凭证（[免费申请](https://console.volcengine.com/speech/service/10030)）

### 安装运行

```bash
# 克隆仓库
git clone https://github.com/wxkingstar/TransEcho.git
cd TransEcho

# 安装依赖
npm install

# 开发模式运行
npm run tauri dev

# 构建发布版
npm run tauri build
```

首次运行时会弹出设置面板，填入从火山引擎获取的 **APP ID** 和 **Access Token** 即可。

> 首次启动需要授予「屏幕录制」权限（系统设置 → 隐私与安全 → 屏幕录制）。

## 技术架构

```
Frontend (Svelte 5 / SvelteKit)        Backend (Rust / Tokio)
┌─────────────────────────┐    IPC    ┌───────────────────────────┐
│ +page.svelte            │◄────────►│ commands.rs               │
│  实时字幕显示 / 设置面板  │  Channel  │  会话编排 / 事件去重       │
└─────────────────────────┘          ├───────────────────────────┤
                                      │ audio/                    │
                                      │  capture.rs  ScreenCaptureKit│
                                      │  resample.rs 48kHz→16kHz  │
                                      │  playback.rs TTS/Rodio    │
                                      ├───────────────────────────┤
                                      │ transport/                │
                                      │  client.rs   WebSocket    │
                                      │  codec.rs    Protobuf     │
                                      └───────────────────────────┘
                                                 │ wss://
                                      ┌──────────────────────┐
                                      │ 豆包同声传译 2.0 API  │
                                      └──────────────────────┘
```

**数据流**: 系统音频 (48kHz stereo f32) → 重采样 (16kHz mono i16) → WebSocket 发送 → 豆包 ASR → Protobuf 响应 → 字幕事件 → 前端展示 & TTS 播报

## 技术栈

| 层 | 技术 |
|----|------|
| 桌面框架 | [Tauri 2.x](https://tauri.app/) |
| 前端 | [Svelte 5](https://svelte.dev/) + [SvelteKit](https://kit.svelte.dev/) |
| 后端 | Rust + [Tokio](https://tokio.rs/) |
| 音频捕获 | [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit) |
| 音频重采样 | [Rubato](https://crates.io/crates/rubato) |
| TTS 播放 | [Rodio](https://crates.io/crates/rodio) |
| 通信协议 | WebSocket + [Protobuf](https://crates.io/crates/prost) |
| 翻译引擎 | [豆包同声传译 2.0](https://www.volcengine.com/docs/4/167875) |

## 项目结构

```
TransEcho/
├── src/                    # 前端 Svelte 代码
│   └── routes/+page.svelte # 单页应用 UI
├── src-tauri/
│   ├── src/
│   │   ├── commands.rs     # Tauri IPC 命令
│   │   ├── audio/
│   │   │   ├── capture.rs  # 系统音频捕获
│   │   │   ├── resample.rs # 音频重采样
│   │   │   └── playback.rs # TTS 播放
│   │   └── transport/
│   │       ├── client.rs   # WebSocket 客户端
│   │       └── codec.rs    # Protobuf 编解码
│   └── proto/              # Protobuf 定义文件
├── package.json
└── CLAUDE.md
```

## 常见问题

<details>
<summary><b>macOS 提示"无法验证开发者"？</b></summary>

首次打开时系统可能提示无法验证开发者。前往：系统设置 → 隐私与安全 → 向下滚动找到 TransEcho → 点击「仍要打开」。
</details>

<details>
<summary><b>没有声音输入？</b></summary>

确保已授予「屏幕录制」权限：系统设置 → 隐私与安全 → 屏幕录制 → 勾选 TransEcho。授权后需重启应用。
</details>

<details>
<summary><b>如何获取 API 凭证？</b></summary>

1. 访问 [火山引擎控制台](https://console.volcengine.com/speech/service/10030)
2. 开通「同声传译 2.0」服务（新用户免费 100 万 token）
3. 创建应用，获取 APP ID 和 Access Token
4. 在 TransEcho 设置中填入即可
</details>

<details>
<summary><b>支持 Windows / Linux 吗？</b></summary>

支持 macOS 14+ 和 Windows 10+。macOS 使用 ScreenCaptureKit 捕获系统音频，Windows 使用 WASAPI Loopback。Linux 暂不支持，欢迎 PR。
</details>

## 参与贡献

欢迎提交 Issue 和 Pull Request！

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing`)
3. 提交改动 (`git commit -m 'Add amazing feature'`)
4. 推送到远程 (`git push origin feature/amazing`)
5. 创建 Pull Request

## License

[MIT](LICENSE) - 自由使用，欢迎传播。

---

<p align="center">
  <sub>Made with Rust + Svelte by <a href="https://github.com/wxkingstar">wangxin</a></sub>
</p>
