<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  Voix locale pour assistants IA : synthese et reconnaissance vocales dans un seul binaire Rust, avec plusieurs backends TTS, Whisper et un serveur MCP.
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

## Installation

```bash
# Installation rapide (macOS Apple Silicon, Linux x86_64 et ARM64, WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Homebrew (macOS Apple Silicon, Linux)
brew install rtk-ai/tap/vox

# Depuis les sources
cargo install --git https://github.com/rtk-ai/vox                   # CPU
cargo install --git https://github.com/rtk-ai/vox --features metal  # macOS Apple Silicon (Metal)
cargo install --git https://github.com/rtk-ai/vox --features cuda   # Linux x86_64, NVIDIA (CUDA)
```

Ne lancez pas `cargo install vox` : sur crates.io ce nom appartient a un autre
projet (github.com/bearcove/vox).

### Quel build choisir ?

La plupart des utilisateurs n'ont rien a faire. Les deux voix par defaut,
`pocket` (anglais) et `piper` (autres langues), tournent sur le CPU dans tous
les builds. Le GPU n'accelere que deux choses : la transcription Whisper
(`vox hear`) et le backend `qwen-native` (clonage de voix).

| Plateforme | Build | Comment l'obtenir |
|------------|-------|-------------------|
| macOS Apple Silicon | Metal (GPU) | Installeur ou Homebrew |
| macOS Intel | aucun | Non supporte : le runtime ONNX utilise par `piper` n'a pas de binaire pour Mac Intel |
| Linux x86_64 | CPU, ou CUDA avec une carte NVIDIA | Installeur (il pose la question). Homebrew, `.deb` et `.rpm` : CPU |
| Linux ARM64 | CPU | Installeur, Homebrew, `.deb` ou `.rpm` |
| Windows x86_64 | CPU | Le `.zip` des GitHub Releases. Avec une carte NVIDIA : WSL2 et l'installeur Linux |

`vox config show` affiche une ligne `acceleration:` qui dit quel build est
installe (Metal, CUDA ou CPU).

### Choix du GPU a l'installation (`VOX_GPU`)

Sur Linux x86_64, quand `nvidia-smi` fonctionne, l'installeur demande s'il faut
installer le build CUDA (reponse par defaut : non). `VOX_GPU` repond a
l'avance :

- `auto` (defaut) : pose la question si un terminal est accessible ; sinon
  installe le build CPU et indique comment obtenir le build CUDA.
- `cuda` : installe le build CUDA sans poser de question.
- `cpu` : installe le build CPU sans poser de question.

```bash
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | VOX_GPU=cuda sh
```

Le build CUDA a besoin du pilote NVIDIA et des bibliotheques runtime CUDA 12
(cuBLAS et cuRAND). Sans elles il ne demarre pas : l'installeur lance donc
le binaire telecharge avant de l'installer et, s'il ne demarre pas, liste les
bibliotheques manquantes et installe le build CPU a la place. Meme repli quand
la release ne contient pas de binaire CUDA : c'est le cas de v0.16.0 et des
versions precedentes.

Les binaires Linux demandent glibc 2.39 ou plus recent (Ubuntu 24.04, Debian
13, Fedora 40) : sur Ubuntu 22.04 ou Debian 12, l'installeur s'arrete avec un
message avant tout telechargement, et compiler depuis les sources n'y change
rien (le runtime ONNX des voix `piper` demande glibc 2.38).

Compiler avec `--features cuda` demande le toolkit CUDA 12 avec `nvcc` ;
definissez `CUDA_COMPUTE_CAP` (au minimum `80`, RTX serie 30) si vous compilez
pour une autre machine. Sur Linux, installez `build-essential cmake pkg-config
clang libclang-dev libssl-dev libasound2-dev`. Details dans le
[README anglais](README.md#install).

## Backends

| Backend | Langues | Clonage de voix | Disponibilite |
|---------|---------|-----------------|---------------|
| `pocket` | anglais | oui, avec `HF_TOKEN` | Toutes plateformes. Defaut pour l'anglais et quand aucune langue n'est donnee |
| `piper` | 11 langues, dont le francais | non | Toutes plateformes. Defaut pour les autres langues |
| `qwen-native` | 10 langues | oui | Toutes plateformes. Utilise le GPU dans un build Metal ou CUDA |
| `say` | voix du systeme | non | macOS uniquement |
| `kokoro` | — | non | Uniquement dans un build compile avec `--features kokoro`. Les binaires publies repondent `Unknown backend: kokoro` |

Il n'y a de Python nulle part. La transcription utilise Whisper et fonctionne
sur toutes les plateformes (`vox hear`).

## Demarrage rapide

```bash
vox "Hello, world."                     # Backend par defaut (pocket, anglais)
vox -l fr "Bonjour le monde"            # Francais : backend piper
vox -l fr --volume 2.0 "Plus fort !"    # Volume 2x (plage : 0.0–5.0)
echo "Texte pipe" | vox -l fr           # Lire depuis stdin
vox setup                               # Configuration interactive (TUI)
```

## Integration IA

Une commande configure, parmi **14 outils IA**, ceux qui sont installes sur la machine (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q, etc.) :

```bash
vox init                # Serveur MCP (defaut) — les outils installes
vox init -m cli         # CLAUDE.md + hook Stop
vox init -m all         # Tous les modes
```

## Plugin Claude Code : visualiseur de voix

vox fournit un plugin [Claude Code](https://claude.com/product/claude-code) qui
affiche le spectre reel de la voix au-dessus du prompt pendant que vox parle.

Dans une session Claude Code (2.1.287 ou plus recent) :

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
/vox-wave color ocean              # couleurs : sunset, ocean, forest, fire, violet, rainbow, mono
/vox-wave color #00ff00 #0000ff    # ou votre propre degrade
```

Details dans [plugins/vox](plugins/vox/README.md).

## Clonage de voix

```bash
vox clone add patrick --audio ~/voix.wav --text "Transcription"   # depuis un fichier
vox clone record mavoix --duration 10                            # ou en enregistrant au micro
vox -b qwen-native -l fr -v patrick "Ceci parle avec votre voix."
```

Le clonage utilise `qwen-native`. Avec `pocket`, il faut `HF_TOKEN`.

## Daemon (modeles chauds)

```bash
vox daemon start        # Garde les modeles en memoire
vox daemon status       # Voir les backends charges
vox daemon stop         # Arreter
```

Le daemon n'est pas lance automatiquement.

## Transcription et conversation vocale

```bash
vox hear -l fr                  # Transcription au micro (Whisper, toutes plateformes)
vox hear -f enregistrement.wav  # Transcrire un fichier WAV

export ANTHROPIC_API_KEY=sk-...
vox chat -l fr                  # Discuter avec Claude (macOS uniquement)
```

`vox chat` appelle lui-meme l'API Claude et demande `ANTHROPIC_API_KEY`. Par le
serveur MCP, un assistant peut tenir une conversation vocale sans cle : il
ecoute avec `vox_hear` et repond avec `vox_speak`.

## Documentation

Ces documents sont en francais.

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | Architecture technique, backends, schema DB, protocole MCP |
| [Fonctionnalites](docs/FEATURES.md) | Documentation de toutes les commandes |
| [Guide](docs/GUIDE.md) | Installation, demarrage rapide, depannage |

## Licence

[Apache-2.0](LICENSE)
