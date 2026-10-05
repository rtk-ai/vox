<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  面向 AI 编程助手的本地语音工具：语音合成和语音识别集成在一个 Rust 二进制文件中，提供多个 TTS 后端、Whisper 和 MCP 服务器。
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README_fr.md">Fran&ccedil;ais</a> &bull;
  <a href="README_zh.md">中文</a> &bull;
  <a href="README_ja.md">日本語</a> &bull;
  <a href="README_ko.md">한국어</a> &bull;
  <a href="README_es.md">Espa&ntilde;ol</a>
</p>

---

## 安装

```bash
# 快速安装 (macOS Apple Silicon、Linux x86_64 和 ARM64、WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Homebrew (macOS Apple Silicon、Linux)
brew install rtk-ai/tap/vox
```

GPU 构建（Metal、CUDA）、从源码编译以及系统要求，请参阅[英文 README](README.md#install)。不要运行 `cargo install vox`：在 crates.io 上这个名字属于另一个项目。

## 后端

| 后端 | 语言 | 语音克隆 | 可用范围 |
|------|------|----------|----------|
| `pocket` | 英语 | 支持（需要 `HF_TOKEN`） | 所有平台。英语以及未指定语言时的默认后端 |
| `piper` | 11 种语言，包括中文 | 不支持 | 所有平台。其他语言的默认后端 |
| `qwen-native` | 10 种语言 | 支持 | 所有平台。在 Metal 或 CUDA 构建中使用 GPU |
| `say` | 系统语音 | 不支持 | 仅 macOS |
| `kokoro` | — | 不支持 | 仅存在于使用 `--features kokoro` 编译的构建中。发布的二进制文件会返回 `Unknown backend: kokoro` |

任何环节都不使用 Python。语音识别使用 Whisper，在所有平台上可用（`vox hear`）。

## 快速开始

```bash
vox "Hello, world."                     # 默认后端（pocket，英语）
vox -l zh "你好，世界"                   # 中文：piper 后端
vox -l zh --volume 2.0 "大声点！"        # 2倍音量（范围：0.0–5.0）
echo "管道文本" | vox -l zh              # 从标准输入读取
vox setup                               # 交互式配置（TUI）
```

## AI 助手集成

一条命令在 **14 个 AI 工具**中配置本机已安装的那些（Claude Code、Cursor、VS Code、Zed、Codex、Gemini、Amazon Q 等）：

```bash
vox init                # MCP 服务器（默认）— 已安装的工具
vox init -m cli         # CLAUDE.md + Stop 钩子
vox init -m all         # 所有模式
```

## Claude Code 插件：语音可视化

vox 附带一个 Claude Code 插件，在 vox 说话时，它会在提示符上方显示真实的语音频谱。在 Claude Code（2.1.287 或更高版本）会话中执行：

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
/vox-wave                          # 预览并设置颜色
```

详情见 [plugins/vox](plugins/vox/README.md)（英文）。

## 语音克隆

```bash
vox clone add myvoice --audio ~/voice.wav --text "转录文本"      # 从音频文件
vox clone record myvoice2 --duration 10                         # 或用麦克风录制
vox -b qwen-native -l zh -v myvoice "用你的声音说话。"
```

语音克隆使用 `qwen-native`。使用 `pocket` 克隆需要 `HF_TOKEN`。

## 守护进程（模型常驻内存）

```bash
vox daemon start        # 保持模型在内存中
vox daemon status       # 查看已加载的后端
vox daemon stop         # 停止
```

守护进程不会自动启动。

## 文档

以下文档以法语撰写。

| 文档 | 说明 |
|------|------|
| [架构](docs/ARCHITECTURE.md) | 技术架构、后端、数据库、MCP 协议 |
| [功能](docs/FEATURES.md) | 所有命令和功能文档 |
| [指南](docs/GUIDE.md) | 安装、快速开始、故障排除 |

## 许可证

[Apache-2.0](LICENSE)
