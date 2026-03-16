# vox

Cross-platform TTS CLI with four backends and MCP server for AI assistants.

```
                           vox
                            |
          +--------+--------+--------+--------+
          |        |                 |        |
        say      qwen          qwen-native  kokoro
     (macOS)  (MLX/Python)    (pure Rust)  (pure Rust)
      native  Apple Silicon   CPU/Metal    CPU/GPU
                               /CUDA
                        |
                      rodio
                  (audio playback)
```

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

| Platform | Default backend | GPU |
|----------|----------------|-----|
| macOS | `say` | `--features metal` |
| Linux / WSL | `kokoro` | `--features cuda` |

Linux requires `sudo apt install libasound2-dev`.

## Quick start

```bash
vox "Hello, world."                     # Speak with default backend
vox -b kokoro -l fr "Bonjour"           # Specific backend + language
echo "Piped text" | vox                 # Read from stdin
vox --list-voices                       # List available voices
```

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

With CLI mode, the agent calls vox directly via Bash — no server, no protocol overhead:

```bash
# Agent just runs this after completing a task
vox "Fix applied and tests passing."
```

MCP mode remains useful for tools that don't have shell access (Cursor, VS Code extensions) or when you need structured tool discovery.

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

## Preferences

```bash
vox config show
vox config set backend kokoro
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

All state is stored locally in `~/.config/vox/`:

```
~/.config/vox/
  vox.db          # SQLite: preferences, voice clones, usage logs
  clones/         # Audio files for voice clones
  packs/          # Installed sound packs
```

| Env var | Description |
|---------|-------------|
| `VOX_CONFIG_DIR` | Override config directory |
| `VOX_DB_PATH` | Override database path |

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | Architecture technique, backends, DB schema, protocole MCP, securite |
| [Features](docs/FEATURES.md) | Documentation fonctionnelle de toutes les commandes et fonctionnalites |
| [Guide](docs/GUIDE.md) | Guide utilisateur, installation, demarrage rapide, depannage |

## License

[Source-Available](LICENSE) — Free for individuals and teams up to 20 people. Enterprise license required for larger organizations. Contact: license@rtk.ai
