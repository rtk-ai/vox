<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  Cross-platform TTS CLI with seven backends and MCP server for AI assistants.
</p>

<p align="center">
  <a href="https://github.com/rtk-ai/vox/actions"><img src="https://github.com/rtk-ai/vox/workflows/CI/badge.svg" alt="CI"></a>
  <a href="https://github.com/rtk-ai/vox/releases"><img src="https://img.shields.io/github/v/release/rtk-ai/vox?color=purple" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache--2.0-blue.svg" alt="License"></a>
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

```
                              vox
                               |
       +--------+--------+----+----+--------+--------+--------+
       |        |        |         |        |        |        |
     say     piper    pocket   qwen-native kokoro  voxtream  qwen
   (macOS)  (Rust/ort) (Rust/candle) (Rust/candle) (ONNX) (zero-shot) (MLX/Py)
   native   CPU       CPU       CPU/Metal  opt-in  CUDA/MPS  Apple Si.
                                 /CUDA
                         |
                       rodio (audio playback)
```

## Backends

| Backend | Engine | Voice cloning | Latency (warm) | GPU | Platform |
|---------|--------|:---:|---:|:---:|----------|
| `say` | macOS native | No | **3s** | No | macOS |
| `piper` | ONNX (Rust) | No | **<1s** | No | All |
| `pocket` | Candle (Rust, 100M) | Yes* | **~2s** | No (CPU-first) | All |
| `qwen-native` | Candle (Rust) | Yes | **~3s** | Metal/CUDA | All |
| `kokoro` | ONNX (Rust, opt-in) | No | **<1s** | No | macOS only |
| `voxtream` | PyTorch 0.5B | Yes | **~8s** | CUDA/MPS | All |
| `qwen` | MLX-Audio (Python) | Yes | **~2s** | Apple Neural | macOS |

> \* `pocket` ships 8 predefined voices with zero setup (public weights). Voice cloning
> from a reference WAV needs `HF_TOKEN` and the gated
> [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts) license accepted.
> The bundled checkpoint is **English-only** (all 8 voices are English speakers);
> Kyutai's per-language checkpoints (fr/de/es/it/pt) need upstream support in the
> pocket-tts crate and are not wired up yet.

### Benchmark — single sentence (~50 chars)

All times measured end-to-end (model loading + inference + audio playback). Cold = first CLI call.

| Backend | M2 Pro (CPU) | RTX 4070 Ti SUPER | Voice cloning | Quality |
|---------|-------------:|-------------------------:|:---:|---------|
| **`say`** | **3s** | macOS only | No | System voices |
| **`piper`** | **<1s** | <1s | No | Good |
| **`pocket`** (Kyutai, 100M) | TBD | TBD | Yes | Very good (EN only) — faster than real-time on CPU¹ |
| **`kokoro`** | **<1s** | macOS only | No | Fair (EN only) |
| **`voxtream`** (VoXtream2, 0.5B) | **68s** / 40s warm | **23s** / **19s** warm | Yes (zero-shot) | Excellent |
| **`qwen-native`** (Qwen3-TTS, 0.6B) | **11m33s** / 3s warm | **48s** (CPU) | Yes | Excellent |
| **`qwen`** (MLX-Audio) | ~15s / 2s warm | macOS only | Yes | Excellent |

**With daemon** (`vox daemon start` — keeps model server warm):

| Backend | M2 Pro (CPU) | Notes |
|---------|-------------:|-------|
| **`voxtream`** | **32s** | Inference CPU-bound (~25s). On CUDA: paper reports 74ms first-packet |
| **`qwen-native`** | **~3s** | Model stays in RAM via global Mutex |

> ¹ `pocket` measured 7–14s end-to-end cold (model load + generation + full playback of 5–9s audio) on an i7-1065G7 laptop CPU — generation itself runs faster than real-time. M2 Pro / RTX numbers to be measured.
> All CUDA benchmarks measured on RTX 4070 Ti SUPER (16GB).
> For lowest latency: `say` (macOS) or `piper` (all platforms). For best quality + cloning: `voxtream` on CUDA with daemon.

## Install

### Pre-built binaries (recommended)

```bash
# Quick install (macOS ARM / Linux x86_64 & ARM64)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Custom install dir (no sudo needed)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | VOX_INSTALL_DIR=~/.local/bin sh

# Homebrew (macOS)
brew install rtk-ai/tap/vox
```

The installer defaults to `/usr/local/bin` (with sudo if needed). When sudo is
not usable (CI, agents, no TTY), it falls back to `~/.local/bin` automatically.

Pre-built binaries are available for each release:

| Platform | Binary | GPU |
|----------|--------|-----|
| macOS (Apple Silicon) | `vox-aarch64-apple-darwin.tar.gz` | Metal |
| Linux x86_64 | `vox-x86_64-unknown-linux-gnu.tar.gz` | CPU |
| Linux x86_64 + CUDA | `vox-x86_64-unknown-linux-gnu-cuda.tar.gz` | CUDA |
| Linux ARM64 | `vox-aarch64-unknown-linux-gnu.tar.gz` | CPU |
| Linux (Debian/Ubuntu) | `vox-{x86_64,aarch64}-unknown-linux-gnu.deb` | CPU |
| Linux (Fedora/RHEL) | `vox-{x86_64,aarch64}-unknown-linux-gnu.rpm` | CPU |
| Windows x86_64 | `vox-x86_64-pc-windows-msvc.zip` | CPU |
| Windows x86_64 + CUDA | `vox-x86_64-pc-windows-msvc-cuda.zip` | CUDA |

Download from [GitHub Releases](https://github.com/rtk-ai/vox/releases).

### From source

```bash
cargo install --path .                   # CPU only
cargo install --path . --features metal  # macOS Apple Silicon (GPU)
cargo install --path . --features cuda   # Linux/Windows NVIDIA (GPU)
```

Linux requires `sudo apt install libasound2-dev`.

### Platform defaults

| Platform | Default backend | Notes |
|----------|----------------|-------|
| All (English or no `-l`) | `pocket` | Public weights auto-download on first use (~226 MB) |
| All (other languages) | `piper` | The pocket checkpoint is English-only; piper has per-language voices |

### VoXtream backend (optional)

```bash
brew install espeak-ng                              # macOS (or apt install espeak-ng on Linux)
uv venv ~/.local/venvs/voxtream --python 3.11
uv pip install --python ~/.local/venvs/voxtream/bin/python "voxtream>=0.2"
# Copy config files
git clone --depth 1 https://github.com/herimor/voxtream.git /tmp/voxtream-repo
mkdir -p "$(vox config show 2>/dev/null | grep dir | awk '{print $2}' || echo ~/.config/vox)/voxtream"
cp /tmp/voxtream-repo/configs/*.json "$(vox config show 2>/dev/null | grep dir | awk '{print $2}' || echo ~/.config/vox)/voxtream/"
```

## Quick start

```bash
vox "Hello, world."                     # Speak with default backend
vox -b qwen-native "Neural TTS."        # Qwen3 (best quality)
vox -b piper "Fast TTS."                # Piper (fastest)
vox --volume 2.0 "Louder!"             # 2x volume (range: 0.0-5.0)
vox -l fr "Bonjour"                     # French
echo "Piped text" | vox                 # Read from stdin
vox --list-voices                       # List available voices
vox setup                               # Interactive TUI configuration
```

## Interactive setup (TUI)

For humans — choose backend, voice, language, style, and volume interactively:

```bash
vox setup
```

```
┌ Backend ──┐┌ Voice ─────┐┌ Lang ┐┌ Style ────┐┌ Volume ┐┌ Config ──────┐
│> say      ││> Samantha  ││> en  ││> (default)││  0.5   ││ Backend: say │
│  piper    ││  Thomas    ││  fr  ││  calm     ││> 1.0   ││ Voice: ...   │
│  qwen-nat ││  Amelie    ││  es  ││  warm     ││  1.5   ││ Lang:  en    │
│  voxtream ││           ││  de  ││  cheerful ││  2.0   ││ Volume: 1.0x │
│  qwen     ││           ││  ja  ││          ││  3.0   ││ [T]est [S]ave│
└───────────┘└────────────┘└──────┘└──────────┘└────────┘└──────────────┘
```

Navigate with arrow keys / hjkl, Tab to switch panel, T to test, S to save, Q to quit.

AI agents use CLI flags instead: `vox -b qwen-native -l fr "text"`

## AI assistant integration

One command configures **14 AI tools** (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q, and more):

```bash
vox init                # MCP server (default) — all AI tools
vox init -m cli         # CLAUDE.md + Stop hook (recommended)
vox init -m skill       # /speak slash command
vox init -m all         # all of the above
```

Running `vox init` again is safe — it skips files that are already configured.

### CLI mode vs MCP mode

**CLI mode is recommended** for AI coding agents. Benchmarks show CLI tools are [10-32x cheaper and 100% reliable vs 72% for MCP](https://mariozechner.at/posts/2025-08-15-mcp-vs-cli/) due to MCP's TCP timeout overhead and JSON schema cost per call.

| Mode | Reliability | Token cost | Best for |
|------|------------|------------|----------|
| **CLI** (`vox init -m cli`) | 100% | Low (Bash call) | Claude Code, Codex, terminal agents |
| **MCP** (`vox init`) | ~72% | Higher (JSON schema) | Cursor, VS Code, GUI-based tools |

## Voice cloning

```bash
vox clone add patrick --audio ~/voice.wav --text "Transcription"
vox clone record myvoice --duration 10
vox -v patrick "This speaks with your voice."
vox clone list
vox clone remove patrick
```

Works with `qwen`, `qwen-native`, and `voxtream` backends. VoXtream2 uses zero-shot cloning (3-10s audio prompt, no training needed).

## Preferences

```bash
vox config show
vox config set backend voxtream
vox config set lang fr
vox config set voice Chelsie
vox config set gender feminine
vox config set style warm
vox config reset
```

## Sound packs

```bash
vox pack install peon              # Install a pack
vox pack set peon                  # Activate it
vox pack play greeting             # Play a sound
vox pack list                      # List available packs
```

## Voice conversation (macOS)

```bash
export ANTHROPIC_API_KEY=sk-...
vox chat -l fr                     # Talk with Claude
vox hear -l fr                     # Speech-to-text only
```

## Data

All state is stored locally — no data sent to external servers (except `vox chat` which uses Claude API).

```
~/.config/vox/           # or ~/Library/Application Support/vox/ on macOS
  vox.db                 # SQLite: preferences, voice clones, usage logs
  clones/                # Audio files for voice clones
  packs/                 # Installed sound packs
  voxtream/              # VoXtream2 config files
```

| Env var | Description |
|---------|-------------|
| `VOX_CONFIG_DIR` | Override config directory |
| `VOX_DB_PATH` | Override database path |

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | Technical architecture, backends, DB schema, MCP protocol, security |
| [Features](docs/FEATURES.md) | All commands and features documented |
| [Guide](docs/GUIDE.md) | Installation, quick start, troubleshooting |

## License

[Apache-2.0](LICENSE)
