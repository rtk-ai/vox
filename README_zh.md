<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  跨平台 TTS 命令行工具，支持五种后端和 MCP 服务器，可与 AI 助手集成。
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
# 快速安装 (macOS / Linux / WSL)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# 从源码安装
cargo install --path .

# 启用 GPU 加速
cargo install --path . --features metal  # macOS Apple Silicon
cargo install --path . --features cuda   # Linux NVIDIA
```

## 快速开始

```bash
vox "Hello, world."                     # 使用默认后端朗读
vox -b voxtream "Zero-shot TTS."        # VoXtream2（最快）
vox -b kokoro -l zh "你好世界"           # Kokoro 中文
vox --volume 2.0 "大声点！"            # 2倍音量（范围：0.0–5.0）
echo "管道文本" | vox                    # 从标准输入读取
vox setup                               # 交互式配置（TUI）
```

## AI 助手集成

一条命令配置 **14 个 AI 工具**（Claude Code、Cursor、VS Code、Zed、Codex、Gemini、Amazon Q 等）：

```bash
vox init                # MCP 服务器（默认）— 所有工具
vox init -m cli         # CLAUDE.md + Stop 钩子
vox init -m all         # 所有模式
```

## 语音克隆

```bash
vox clone add myvoice --audio ~/voice.wav --text "转录文本"
vox clone record myvoice --duration 10
vox -v myvoice "用你的声音说话。"
```

## 守护进程（模型常驻内存）

```bash
vox daemon start        # 保持模型在内存中
vox daemon status       # 查看已加载的后端
vox daemon stop         # 停止
```

## 文档

| 文档 | 说明 |
|------|------|
| [架构](docs/ARCHITECTURE.md) | 技术架构、后端、数据库、MCP 协议 |
| [功能](docs/FEATURES.md) | 所有命令和功能文档 |
| [指南](docs/GUIDE.md) | 安装、快速开始、故障排除 |

## 许可证

[Apache-2.0](LICENSE)
