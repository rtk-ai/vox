<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  Local voice for AI coding agents: text-to-speech and speech-to-text in one Rust binary.
</p>

<p align="center">
  No Python and no cloud service: synthesis and transcription run on your machine.
  Several TTS backends, Whisper speech-to-text, an MCP server with 14 tools,
  and one command that configures the AI tools installed on your machine
  (14 supported).
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
            +-----------------+------------------+
            |                                    |
       speak (TTS)                          hear (STT)
            |                                    |
   +--------+---------+----------+            Whisper
   |        |         |          |            (candle)
 pocket   piper   qwen-native   say           99 languages
(candle)  (ONNX)   (candle)   (macOS)         CPU, Metal or CUDA
   |        |         |                          |
   +---- rodio (playback) ----+             cpal (microphone)
```

`say` plays through the macOS `say` command. A fifth backend, `kokoro`, exists
only in a build compiled with `--features kokoro` (see below).

## Backends

| Backend | Engine | Languages | Voice cloning | GPU | Where |
|---------|--------|-----------|:---:|:---:|-------|
| `pocket` | Kyutai pocket-tts (100M) on candle | English | Yes¹ | No | All platforms. Default for English and when no language is given |
| `piper` | Piper on ONNX Runtime | 11² | No | No | All platforms. Default for every other language |
| `qwen-native` | Qwen3-TTS (0.6B) on candle | 10³ | Yes | In a Metal or CUDA build | All platforms |
| `say` | macOS `/usr/bin/say` | System voices | No | No | macOS only |
| `kokoro` | Kokoro on ONNX Runtime | See `--list-voices` | No | No | Only in a build compiled with `--features kokoro` |

> ¹ `pocket` has 8 predefined voices (`alba`, `marius`, `javert`, `jean`,
> `fantine`, `cosette`, `eponine`, `azelma`) that need no setup: the public
> weights (~226 MB) are downloaded on first use. Voice cloning from a reference
> WAV needs `HF_TOKEN` and the license of the gated
> [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts) checkpoint
> accepted. The bundled checkpoint is English-only (all 8 voices are English
> speakers); Kyutai's per-language checkpoints (fr/de/es/it/pt) need upstream
> support in the pocket-tts crate and are not wired up.
>
> ² `piper` has one default voice for each of en, fr, es, de, it, pt, zh, ko,
> ru, ar and nl, chosen by `-l`. `-v` with the name of another voice of
> [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices) selects
> that voice, for example `-v fr_FR-siwis-low`; with a value that is not a
> piper voice name, vox prints a note and uses the default voice of the
> language. A voice is downloaded the first time it is used (~60 MB for a
> `medium` voice). `piper` has no Japanese voice, so Japanese uses
> `qwen-native` by default; `-b piper -l ja` fails at once with a message
> that says so.
>
> ³ `qwen-native` accepts en, fr, es, de, it, pt, zh, ja, ko and ru. It loads
> the Qwen3-TTS Base model, which has no preset voices: it is the backend for
> voice clones. Without a clone, `-l` has no effect (the model infers the
> language from the text). The first use downloads the model and its tokenizer,
> about 2.5 GB.

`kokoro` is not in the release binaries: there, `vox -b kokoro` answers
`Unknown backend: kokoro`.

### Time to first sound

Time from launching `vox` to the first sound, for a sentence that lasts about
3 seconds. Measured in October 2026 on an Apple A18 Pro laptop (8 GB, low-power
mode) with the release Metal build:

| Backend | Without the daemon | With the daemon |
|---------|-------------------:|----------------:|
| `pocket` (English) | 0.18 to 0.55 s | not measured |
| `piper` (French) | about 0.75 s | 0.26 to 0.32 s |

A long English text (15 s of audio) starts after 0.81 s: `pocket` plays while
it is still generating. `piper` and `pocket` send their samples straight to the
audio device, which is opened while the model loads. The first use of a
backend also downloads its model.

`vox daemon start` keeps the models loaded between calls (`vox daemon status`,
`vox daemon stop`). While it runs, `vox "..."` calls that use `pocket`, `piper`,
`qwen-native` or `kokoro` go through it, except calls with `-o`. It is not
started automatically, and it stops by itself after 300 seconds without a
request (`vox daemon start --idle-timeout <seconds>`; `0` means no timeout).
The MCP server does not use the daemon: it keeps the models loaded in its own
process.

`VOX_TIMINGS=1` prints the time of each step of an utterance on stderr.

The other backends have not been measured again. The figures below are older.
They are end-to-end times for one sentence of about 50 characters (model load,
synthesis and playback) on an M2 Pro using the CPU:

| Backend | Time |
|---------|------|
| `say` | 3 s |
| `qwen-native` | 11 min 33 s for the first call; about 3 s with the model already loaded by the daemon |
| `kokoro` | under 1 s |

On a machine with an RTX 4070 Ti SUPER (16 GB), `qwen-native` took 48 s; that
figure was recorded as a CPU run.

## Install

### Pre-built binaries (recommended)

```bash
# Quick install (macOS Apple Silicon, Linux x86_64 and ARM64, WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Custom install dir (no sudo needed)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | VOX_INSTALL_DIR=~/.local/bin sh

# Homebrew (macOS Apple Silicon, Linux)
brew install rtk-ai/tap/vox
```

The installer defaults to `/usr/local/bin` (with sudo if needed). When sudo is
not usable (CI, agents, no TTY), it falls back to `~/.local/bin` automatically.

#### Which build do I need?

Most users need nothing special. The two default voices, `pocket` (English) and
`piper` (other languages), run on the CPU in every build. A GPU build speeds up
only two things: Whisper transcription (`vox hear`) and the `qwen-native`
backend (voice cloning). When the GPU device cannot be opened at run time,
both fall back to the CPU.

| Platform | Build | How to get it |
|----------|-------|---------------|
| macOS, Apple Silicon | Metal (GPU) | Installer or Homebrew |
| macOS, Intel | none | Not supported, see below |
| Linux x86_64 | CPU, or CUDA with an NVIDIA card | Installer (it asks, see below). Homebrew, `.deb` and `.rpm` are CPU |
| Linux ARM64 | CPU | Installer, Homebrew, `.deb` or `.rpm` |
| Windows x86_64 | CPU | The `.zip` from GitHub Releases. With an NVIDIA card: WSL2 and the Linux installer |

To check which build is installed, run `vox config show`. Its `acceleration:`
line reads `Metal (GPU): used by Whisper and qwen-native`,
`CUDA (NVIDIA GPU): used by Whisper and qwen-native` or `CPU only`. It reports
how the binary was built, not which device is in use.

There is no Intel Mac build (`x86_64-apple-darwin`): the ONNX runtime that
`piper` depends on ships no prebuilt binary for Intel Macs. The last release
with an Intel Mac binary is v0.10.0. The installer stops there with a message
saying so, before downloading anything.

There is no Windows CUDA build either. With an NVIDIA card on Windows, run the
Linux installer inside WSL2.

The Linux binaries need glibc 2.39 or newer: Ubuntu 24.04, Debian 13, Fedora 40
and later. On Ubuntu 22.04 or Debian 12 the installer stops with a message
before downloading anything. Building from source does not help there: the
ONNX runtime used by the `piper` voices needs glibc 2.38 itself. They also need
ALSA (`libasound2t64`, or `alsa-lib` on Fedora) and OpenSSL 3 at run time.

#### GPU choice in the installer (`VOX_GPU`)

On Linux x86_64, when `nvidia-smi` runs successfully, the installer asks
whether to install the CUDA build. The question says what it changes (faster
voice cloning with `qwen-native` and faster transcription with Whisper; the
default voices run on the CPU either way) and what it needs. The default
answer is No.

`VOX_GPU` answers the question in advance:

| `VOX_GPU` | Effect on Linux x86_64 with an NVIDIA card |
|-----------|--------------------------------------------|
| `auto` (default) | Asks when a terminal is reachable. Otherwise installs the CPU build and prints how to get the CUDA one |
| `cuda` | Installs the CUDA build without asking |
| `cpu` | Installs the CPU build without asking |

```bash
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | VOX_GPU=cuda sh
```

Any other value is an error. `VOX_GPU=cuda` on a platform it does not apply to
(macOS, Linux ARM64) prints a warning and installs the normal build for that
platform. macOS always gets the Metal build, the only macOS build, so
`VOX_GPU=cpu` only prints a warning there too. On Linux x86_64 without a
working `nvidia-smi`, `VOX_GPU=cuda` prints a warning and still tries the CUDA
build.

The CUDA build needs, on the machine that runs it:

- the NVIDIA driver (it provides `libcuda.so.1`);
- the CUDA 12 runtime libraries cuBLAS (`libcublas.so.12`, `libcublasLt.so.12`)
  and cuRAND (`libcurand.so.10`). This is what `ldd` lists on a real CUDA build.

These libraries are linked dynamically: without them the CUDA binary does not
start at all. So the installer runs the downloaded binary (`vox --version`)
before installing it. If it does not start, the installer lists the missing
libraries and installs the CPU build instead. It also installs the CPU build,
with a warning, when the release has no CUDA binary or its download fails. Its
last lines say which
build was installed (CPU, CUDA or Metal) and, for a CPU build on a machine with
an NVIDIA card, how to switch.

The CUDA build is compiled for CUDA 12.6 and GPU compute capability 8.0, so it
needs an RTX 30 series card or newer (or a data-center card of the same
generations). Older cards are not supported: the GPU kernels do not compile
below 8.0.

What has been checked on real hardware, an RTX 4070 Ti SUPER under WSL2 with
CUDA 12.4: a CUDA build compiled there starts, the installer installs it and
falls back to the CPU build in a container without the CUDA libraries, and
Whisper transcribes 24.6 s of audio in about 1.4 s against about 26 s for the
CPU build on the same machine, with the same text. The binary the release
workflow produces has not been run yet: CI compiles it on runners that have no
GPU.

#### Release assets

| Platform | Binary | Build |
|----------|--------|-------|
| macOS (Apple Silicon) | `vox-aarch64-apple-darwin.tar.gz` | Metal |
| Linux x86_64 | `vox-x86_64-unknown-linux-gnu.tar.gz` | CPU |
| Linux x86_64, NVIDIA | `vox-x86_64-unknown-linux-gnu-cuda.tar.gz` | CUDA 12. Built by the release workflow, not in v0.16.0 or earlier. Installed with `VOX_GPU=cuda` |
| Linux ARM64 | `vox-aarch64-unknown-linux-gnu.tar.gz` | CPU |
| Linux (Debian/Ubuntu) | `vox-{x86_64,aarch64}-unknown-linux-gnu.deb` | CPU |
| Linux (Fedora/RHEL) | `vox-{x86_64,aarch64}-unknown-linux-gnu.rpm` | CPU |
| Windows x86_64 | `vox-x86_64-pc-windows-msvc.zip` | CPU |

Download from [GitHub Releases](https://github.com/rtk-ai/vox/releases). The
CUDA job of the release workflow is allowed to fail, so a release can go out
without the CUDA binary: check the asset list of the release you install.

### From source

```bash
cargo install --git https://github.com/rtk-ai/vox                   # CPU only
cargo install --git https://github.com/rtk-ai/vox --features metal  # macOS Apple Silicon (Metal)
cargo install --git https://github.com/rtk-ai/vox --features cuda   # Linux x86_64, NVIDIA (CUDA)

# From a clone of this repository
cargo install --path .                                              # same --features flags
```

Do not run `cargo install vox`: on crates.io that name belongs to an unrelated
crate (github.com/bearcove/vox).

Build prerequisites:

- Linux: `sudo apt install build-essential cmake pkg-config clang libclang-dev libssl-dev libasound2-dev`.
  The piper voices build espeak-ng with cmake and generate their bindings with
  libclang; model downloads link OpenSSL.
- `--features cuda`: the CUDA 12 toolkit with `nvcc` on the `PATH` (CI uses
  12.6.3; 12.4 is known to work). The GPU kernels are compiled for one compute
  capability, read from `nvidia-smi` on the build machine, and run on that
  generation and newer ones. Set `CUDA_COMPUTE_CAP` when building for another
  machine or where `nvidia-smi` is not on the `PATH` (under WSL it lives in
  `/usr/lib/wsl/lib`). The lowest value that compiles is `80` (8.0, RTX 30
  series), which CI and the release build use.

### Platform defaults

The default backend depends on the language, not on the platform:

| Language | Default backend | First use |
|----------|----------------|-----------|
| English, or no `-l` | `pocket` | Downloads the public weights (~226 MB) |
| Japanese (`-l ja`) | `qwen-native` | Downloads Qwen3-TTS (about 2.5 GB). piper has no Japanese voice. Slower than the other two: 14 s for a short sentence, model load included, on an Apple A18 Pro laptop with the Metal build |
| Any other language | `piper` | Downloads the voice of that language (~60 MB). The pocket checkpoint is English-only |

A stored backend preference (`vox config set backend ...`) replaces both
defaults.

## Quick start

```bash
vox "Hello, world."                     # Speak with the default backend (pocket)
vox -l fr "Bonjour"                     # French: piper
vox -b piper "Hello from piper."        # Choose a backend
vox -b qwen-native "Hello from Qwen3."  # Qwen3-TTS
vox --volume 2.0 "Louder!"              # 2x volume (range: 0.0-5.0)
echo "Piped text" | vox                 # Read from stdin
vox -o note.wav "Saved, not spoken."    # Write a WAV file instead of playing
vox --list-voices                       # Voices of the selected backend
vox setup                               # Interactive TUI configuration
```

## Interactive setup (TUI)

`vox setup` opens a terminal interface to choose the backend, voice, language,
style and volume, and to test the result:

```
┌ Backend ─────┐┌ Voice ────┐┌ Language ┐┌ Style ─────┐┌ Volume ┐┌ Config ──────────────────────┐
│> pocket      ││> alba     ││> en      ││> (default) ││  0.5   ││ Backend: pocket              │
│  piper       ││  marius   ││  fr      ││  calm      ││  0.75  ││ Voice:   alba                │
│  qwen-native ││  javert   ││  es      ││  energetic ││> 1.0   ││ Lang:    en                  │
│  say         ││  jean     ││  de      ││  warm      ││  1.25  ││ [T] Test  [S] Save  [Q] Quit │
└──────────────┘└───────────┘└──────────┘└────────────┘└────────┘└──────────────────────────────┘
```

The backend list holds the backends of the build (`say` on macOS only).
Up/Down or j/k move in a list, Tab, Left/Right or h/l change panel, T speaks a
test sentence, S saves, Q or Esc quits.

S saves the backend, the language, the voice and the style. The volume is used
for the test only: there is no volume preference, pass `--volume` on each call.

AI agents use the command-line flags instead: `vox -l fr "text"`.

## AI assistant integration

One command writes the vox MCP server into the configuration of the AI tools
found on your machine. It knows **14 AI tools**: Claude Code, Claude Desktop,
Cursor, Windsurf, VS Code / Copilot, Zed, Codex, OpenCode, Gemini, Amazon Q,
Cline, Roo Code, Kilo Code and Amp.

```bash
vox init                # MCP server (default), for the installed tools among the 14
vox init -m cli         # CLAUDE.md block + Stop hook, in the current directory
vox init -m skill       # /speak slash command for Claude Code
vox init -m all         # all of the above
```

| Mode | What it writes | What the agent gets |
|------|----------------|---------------------|
| `mcp` | A `vox serve` entry in the MCP configuration of each tool found on the machine, in your home directory. A tool counts as installed when its configuration file or its own directory exists; the others are reported as `not installed, skipped` and nothing is written for them | 14 tools: `vox_speak`, `vox_hear`, `vox_list_voices`, clones, preferences, statistics and sound packs |
| `cli` | A block in `CLAUDE.md` and a Stop hook in `.claude/settings.json`, both in the current directory | An instruction to run `vox "..."` after a significant task. The hook says a short phrase ("Done.") when Claude Code stops |
| `skill` | `~/.claude/commands/speak.md` | The `/speak` command |

Running `vox init` again is safe: what is already configured is left as it is.
After its report, `vox init` names the tools to restart, or says that nothing
changed.

The MCP server starts with `vox serve` (stdio); `vox init` writes that command
for you. With the MCP tools an agent can hold a voice conversation by itself:
`vox_hear`, think, `vox_speak`, with no API key.

The generated instructions and the Stop hook use your language. vox takes
`--lang`, then your `vox config set lang` preference, then the system locale.
With none of them it tells the agent to match the language you write in.

```bash
vox init -m cli --lang de   # agent summaries in German, the hook says "Fertig."
vox init -m cli --lang es   # Spanish, the hook says "Listo."
```

`--lang` accepts en, fr, es, de, it, pt, zh, ja, ko, ru, ar and nl. The hook
runs `vox -l <lang> "..."`, which uses `piper` for every language but English.

## Claude Code plugin: live voice visualizer

vox ships a [Claude Code](https://claude.com/product/claude-code) plugin that
draws the spectrum of the voice above the prompt while vox speaks:

```text
            ▂ ▃           ▃ ▄ ▃          vox · speaking
▃ ▅ ▄ ▂ ▄ ▃ █ █ ▅ ▁   ▂ ▅ █ █ █ ▆ ▃ ▁    Done. The tests pass.
```

The bars are the spectrum of the audio, not an animation. While it plays, vox
writes the spectrum of the sound to `now-playing.json` in its config directory
and removes the file afterwards; the plugin reads that file. It works with the
MCP tools, with `vox ...` run from the shell, and with the Stop hook. At the
default volume, the `say` backend plays outside vox and writes nothing, so the
plugin shows no spectrum for it.

In a Claude Code session (2.1.287 or later):

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
```

Then pick your colors, kept from one session to the next:

```text
/vox-wave                          # preview, no audio needed
/vox-wave color ocean              # sunset, ocean, forest, fire, violet, rainbow, mono
/vox-wave color #00ff00 #0000ff    # your own gradient, one to three stops
```

`vox init` tells the agent that the plugin exists (in the `CLAUDE.md` block and
in the MCP instructions), so you can also ask Claude how to get the visualizer.
Details in [plugins/vox](plugins/vox/README.md).

## Voice cloning

```bash
vox clone add patrick --audio ~/voice.wav --text "Transcription"
vox clone record myvoice --duration 10
vox -v patrick "This speaks with your voice."
vox clone list
vox clone remove patrick
```

Cloning works with `qwen-native`, with no setup, and with `pocket`, which needs
`HF_TOKEN` (see [Backends](#backends)). A 3-second reference clip is enough.

Without `-b`, `vox -v <clone>` uses `pocket` when `pocket` is the selected
backend and `HF_TOKEN` is set, and `qwen-native` in every other case. A `-b` on
the command line is respected: `-b pocket` without `HF_TOKEN` stops with an
error that asks for the token, and a backend that cannot clone (`piper` or
`say`, for example) prints a note and ignores the clone.

`clone add` reads a WAV, MP3, FLAC or Ogg file, converts it to WAV and keeps
that copy in the `clones/` folder of the config directory: the original file
can be moved or deleted afterwards. A `.m4a` file is refused. So is a name that
is already taken, whatever its case: remove that clone first. `clone record`
saves its recording, a WAV, in the same folder.

## Preferences

```bash
vox config show
vox config set backend qwen-native
vox config set lang fr
vox config set voice alba
vox config set stt_model openai/whisper-base
vox config reset
```

The keys are `backend`, `voice`, `lang`, `rate`, `gender`, `style`, `model`,
`stt_model` and `pack`. A flag on the command line wins over a preference.

- `lang` accepts en, fr, es, de, it, pt, zh, ja, ko, ru, ar and nl.
- `rate` (words per minute) applies to `say` only, `model` to `qwen-native` only.
- `gender` and `style` are accepted and stored, but no backend of this version
  uses them.

`vox config show` ends with an `acceleration:` line that says how the binary
was built (Metal, CUDA or CPU).

## Sound packs

```bash
vox pack list                      # Installed packs, and a few names to install
vox pack install peon              # Download a pack
vox pack set peon                  # Make it the active pack
vox pack play greeting             # Play a random sound of a category
vox pack remove peon               # Delete it
```

`vox pack install` takes the name of a pack of the peon-ping registry (browse
them at https://openpeon.com/packs); `vox pack list` suggests a few of them.
Packs are published in the CESP format (`openpeon.json`), which vox converts
when it installs them. The categories are `greeting`, `acknowledge`,
`complete`, `error`, `permission`, `resource_limit` and `annoyed`. A pack can
carry other categories, which keep their CESP name (`session.end`,
`task.progress`).

## Save to a file instead of speaking

```bash
vox -o note.wav "Text to save"                      # default backend
vox -l fr -o note.wav "Texte a enregistrer"         # piper
vox -b qwen-native -v myvoice -o note.wav "..."     # with a voice clone
ffmpeg -i note.wav -c:a libopus -b:a 32k note.ogg   # convert it afterwards, here to Opus
```

`-o` writes a WAV file instead of playing, on every backend and every platform.
A call with `-o` does not go through the daemon.

`--output` exists on the command line only. The MCP `vox_speak` tool has no
such parameter, so an agent cannot use it to write to a path of its choice.

## Speech-to-text (all platforms)

Whisper on candle, 99 languages. The model is downloaded from Hugging Face on
first use. It runs on the GPU in a Metal or CUDA build. The MCP server keeps it
loaded between two `vox_hear` calls; `vox hear` from the shell loads it at each
call.

```bash
vox hear                                   # Listen, auto-detect language, print text
vox hear -l fr -t 60 -s 3.0                # French, max 60s, stop after 3s of silence
vox hear -m openai/whisper-large-v3-turbo  # Larger model (GPU + 16 GB RAM recommended)
vox hear -f recording.wav                  # Transcribe a WAV file instead of the mic
```

A short beep marks the start of the recording and a lower one its end
(`VOX_CUES=0` turns them off).

Model size is a tradeoff. Time and RAM were measured warm on 11.85 s of French
speech, on an Apple M2 (`tiny`, `base`, `small`). The size on disk is that of
the files in the Hugging Face cache:

| Model | Time | RAM | On disk | Quality |
|-------|------|-----|---------|---------|
| `openai/whisper-tiny` | 0.93 s | 350 MB | 154 MB | roughest |
| `openai/whisper-base` | 1.52 s | 631 MB | 293 MB | the default |
| `openai/whisper-small` | 5.07 s | 1.98 GB | 970 MB | one fewer mistake in 30 words |
| `openai/whisper-large-v3-turbo` | not measured | ~3.5 GB | not measured | not measured, GPU recommended |

`base` is the default because, in that test, it was 3x faster and 3x lighter
than `small` for one more mistake on a 30-word sentence.

What a GPU build changes: on a 12-core x86_64 Linux machine (WSL2) with an RTX
4070 Ti SUPER, `base` transcribes 24.6 s of audio in about 26 s with the CPU
build and in about 1.4 s with the CUDA build, with the same text.

To use another model, in order of precedence:

```bash
vox hear -m openai/whisper-small                # 1. this call only
export VOX_STT_MODEL=openai/whisper-small       # 2. this shell
vox config set stt_model openai/whisper-small   # 3. stored preference
# 4. [whisper] model_id in models.toml, in the config directory
```

| Env var | Description |
|---------|-------------|
| `VOX_STT_MODEL` | Whisper repo. Wins over the stored preference and `models.toml` (default `openai/whisper-base`, ~630 MB RAM; `openai/whisper-large-v3-turbo` ~3.5 GB) |
| `VOX_VAD_THRESHOLD` | Minimum RMS speech threshold, between 0 and 1 (default 0.0125; adapts to ambient noise) |
| `VOX_VAD_DEBUG` | Set to `1` to print RMS levels and the chosen threshold |
| `VOX_CUES` | Set to `0` to turn off the beeps at the start and the end of a recording |

No external tool is needed: the microphone is read with cpal.

## Voice conversation

There are two ways to talk with an assistant.

Through the MCP server, on every platform and with no API key: the agent calls
`vox_hear`, thinks, and answers with `vox_speak`. Ask it to "chat" or "talk".

With `vox chat`, on macOS only. vox calls the Claude API itself, so it needs
`ANTHROPIC_API_KEY`:

```bash
export ANTHROPIC_API_KEY=sk-...
vox chat -l fr                     # Talk with Claude
```

`vox chat` records until you press Enter, transcribes with Whisper, sends the
text to the Claude API and speaks the answer. Its prompts and its system prompt
follow `-l`, or the stored `lang` preference: English by default, French for
`fr`. For another language they stay in English and Claude is asked to answer
in that language. The model is `claude-haiku-4-5`; `VOX_CHAT_MODEL` names
another one.

## Data

What you say and what vox reads aloud stay on your machine, except with
`vox chat`, which sends the transcript to the Claude API. Models and voices are
downloaded from Hugging Face on first use; `vox pack install` downloads a sound
pack from GitHub.

```
~/.config/vox/           # Linux; ~/Library/Application Support/vox/ on macOS
  vox.db                 # SQLite: preferences, voice clones, usage log
  clones/                # Reference audio of the voice clones (WAV)
  packs/                 # Installed sound packs
  piper/                 # Piper voices
  pocket/                # Configuration of the pocket model
  models.toml            # Optional: your own model ids
  now-playing.json       # Only while vox plays: the spectrum for the visualizer
  daemon.pid             # Only while the daemon runs
  daemon.log             # Output of the daemon, emptied at each start
```

The Whisper, pocket and Qwen3-TTS weights are in the Hugging Face cache
(`~/.cache/huggingface/hub`), not in this directory.

| Env var | Description |
|---------|-------------|
| `VOX_CONFIG_DIR` | Override config directory |
| `VOX_DB_PATH` | Override database path |
| `VOX_TIMINGS` | Set to `1` to print, on stderr, when each step of an utterance finished (model load, synthesis, device open, first sample) |
| `VOX_DAEMON_PORT` | Port of the daemon on 127.0.0.1 (default 19876) |
| `HF_TOKEN` | Hugging Face token, needed only for voice cloning with `pocket` |
| `ANTHROPIC_API_KEY` | Needed only by `vox chat` |
| `VOX_CHAT_MODEL` | Claude model used by `vox chat` (default `claude-haiku-4-5`) |

## Documentation

These documents are written in French.

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | Technical architecture, backends, DB schema, MCP protocol, security |
| [Features](docs/FEATURES.md) | All commands and features documented |
| [Guide](docs/GUIDE.md) | Installation, quick start, troubleshooting |

## License

[Apache-2.0](LICENSE)
