<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  Cross-platform TTS CLI with five backends and MCP server for AI assistants.
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
       +--------+--------+----+----+--------+-----------+
       |        |        |         |        |           |
     say      qwen    qwen-native kokoro  voxtream    (TUI)
   (macOS)  (MLX/Py)  (Rust/candle) (ONNX) (zero-shot) vox setup
   native   Apple Si.  CPU/Metal  CPU/GPU  CUDA/MPS
                        /CUDA
                          |
                        rodio (audio playback)
```

## Backends

| Backend | Engine | Voice cloning | Latency (cold) | Latency (warm) | GPU | Platform |
|---------|--------|:---:|---:|---:|:---:|----------|
| `say` | macOS native | No | **3s** | **3s** | No | macOS |
| `kokoro` | ONNX via Python | No | **10s** | **10s** | No | All |
| `qwen-native` | Candle (Rust) | Yes | **11m33s** | ~3s | Metal/CUDA | All |
| `voxtream` | PyTorch 0.5B | Yes | **68s** | ~8s | CUDA/MPS | All |
| `qwen` | MLX-Audio (Python) | Yes | ~15s | ~2s | Apple Neural | macOS |

### Benchmark — single sentence (~50 chars)

All times measured end-to-end (model loading + inference + audio playback). Cold = first CLI call.

| Backend | M2 Pro (CPU) | RTX 4070 Ti SUPER | Voice cloning | Quality |
|---------|-------------:|-------------------------:|:---:|---------|
| **`say`** | **3s** | macOS only | No | System voices |
| **`kokoro`** | **10s** | ~10s | No | Good |
| **`voxtream`** (VoXtream2, 0.5B) | **68s** / 40s warm | **23s** / **19s** warm | Yes (zero-shot) | Excellent |
| **`qwen-native`** (Qwen3-TTS, 0.6B) | **11m33s** / 3s warm | **48s** (CPU) | Yes | Excellent |
| **`qwen`** (MLX-Audio) | ~15s / 2s warm | macOS only | Yes | Excellent |

**With daemon** (`vox daemon start` — keeps model server warm):

| Backend | M2 Pro (CPU) | Notes |
|---------|-------------:|-------|
| **`voxtream`** | **32s** | Inference CPU-bound (~25s). On CUDA: paper reports 74ms first-packet |
| **`qwen-native`** | **~3s** | Model stays in RAM via global Mutex |

> All CUDA benchmarks measured on RTX 4070 Ti SUPER (16GB). qwen-native CUDA not yet supported (requires cudarc update for CUDA 13.2).
> For lowest latency: `say` (macOS) or `kokoro`. For best quality + cloning: `voxtream` on CUDA with daemon.

## Install

```bash
# Quick install (macOS / Linux / WSL)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# From source
cargo install --path .

# With GPU acceleration
cargo install --path . --features metal  # macOS Apple Silicon
cargo install --path . --features cuda   # Linux NVIDIA
```

### VoXtream backend (optional)

```bash
brew install espeak-ng                              # macOS (or apt install espeak-ng on Linux)
uv venv ~/.local/venvs/voxtream --python 3.11
uv pip install --python ~/.local/venvs/voxtream/bin/python "voxtream>=0.2"
# Copy config files
git clone --depth 1 https://github.com/herimor/voxtream.git /tmp/voxtream-repo
cp /tmp/voxtream-repo/configs/*.json "$(vox config show 2>/dev/null | head -1 | grep -v backend || echo ~/.config/vox)/voxtream/"
```

| Platform | Default backend | GPU |
|----------|----------------|-----|
| macOS | `say` | `--features metal` |
| Linux / WSL | `kokoro` | `--features cuda` |

Linux requires `sudo apt install libasound2-dev`.

## Quick start

```bash
vox "Hello, world."                     # Speak with default backend
vox -b voxtream "Zero-shot TTS."        # VoXtream2 (fastest neural)
vox -b kokoro -l fr "Bonjour"           # Kokoro with language
vox --volume 2.0 "Louder!"             # 2x volume (range: 0.0–5.0)
echo "Piped text" | vox                 # Read from stdin
vox --list-voices                       # List available voices
vox setup                               # Interactive TUI configuration
```

## Interactive setup (TUI)

For humans — choose backend, voice, language, and style interactively:

```bash
vox setup
```

```
┌ Backend ──┐┌ Voice ─────┐┌ Lang ┐┌ Style ────┐┌ Config ──────┐
│> say      ││> Samantha  ││> en  ││> (default)││ Backend: say │
│  kokoro   ││  Thomas    ││  fr  ││  calm     ││ Voice: ...   │
│  qwen-nat ││  Amelie    ││  es  ││  warm     ││ Lang:  en    │
│  voxtream ││           ││  de  ││  cheerful ││              │
│  qwen     ││           ││  ja  ││          ││ [T]est [S]ave│
└───────────┘└────────────┘└──────┘└──────────┘└──────────────┘
```

Navigate with arrow keys / hjkl, Tab to switch panel, T to test, S to save, Q to quit.

AI agents use CLI flags instead: `vox -b voxtream -l fr "text"`

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
