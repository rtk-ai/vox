# Architecture technique

## Vue d'ensemble

vox est un CLI TTS cross-platform ecrit en Rust. Il transforme du texte en parole via quatre backends interchangeables et expose ses fonctionnalites comme serveur MCP (Model Context Protocol) pour l'integration avec les assistants IA.

```
                          vox (Rust)
                              |
              +---------------+---------------+
              |                               |
          speak (TTS)                     hear (STT)
              |                               |
   +------+---+---+--------+-------+       Whisper
   |      |       |        |       |      (candle)
  say   piper  pocket  qwen-native kokoro  99 langues
 (macOS) (ONNX) (candle) (candle)  (ONNX)  CPU/Metal/CUDA
 natif    CPU    CPU    CPU/Metal   opt-in
                         /CUDA
              |                               |
            rodio (lecture)            cpal (capture)
```

## Modules source

```
src/
  main.rs         CLI (clap) — parsing args, dispatch subcommands
  lib.rs          Exports publics des modules
  mcp.rs          Serveur MCP JSON-RPC stdio (14 tools)
  backend/
    mod.rs        Trait TtsBackend + dispatch get_backend()
    say.rs        Backend macOS natif (NSSpeechSynthesizer via /usr/bin/say)
    piper.rs      Backend Piper (ONNX via ort, 50+ langues, espeak-ng embarque)
    pocket.rs     Backend Kyutai pocket-tts (candle, 100M, CPU temps reel)
    qwen_native.rs Backend candle/Rust (Qwen3-TTS, cross-platform, clonage)
    kokoro.rs     Backend Kokoro-TTS (ONNX Rust, opt-in via feature)
  config.rs       Chemins config, constantes, enums (Gender, IntonationStyle)
  db.rs           SQLite (rusqlite) — preferences, clones, usage logs, stats
  init.rs         Auto-configuration pour 14 outils IA
  input.rs        Lecture texte (args, stdin, pipe)
  lang.rs         Resolution de langue (drapeau > preference > locale systeme)
  clone.rs        Voice cloning — validation audio, enregistrement micro
  pack.rs         Sound packs (peon-ping compatible)
  audio.rs        Playback audio via rodio
  mic.rs          Capture micro via cpal, VAD energie, resampling 16 kHz
  stt/            Speech-to-text Whisper sur candle (Rust pur, toutes plateformes)
    mod.rs        Cache modele process-wide, API transcribe / transcribe_samples
    whisper.rs    Inference (mel, encodeur, decodeur greedy + fallback temperature)
  chat/           Mode conversation vocale (macOS only)
```

## Backends TTS

| Backend | Plateforme | Dependance | Latence | Voice cloning |
|---------|-----------|------------|---------|---------------|
| `say` | macOS uniquement | Aucune (systeme) | ~100ms | Non |
| `piper` | Toutes | Aucune (Rust pur) | <1s | Non |
| `pocket` | Toutes | Aucune (Rust pur) | ~2s | Oui (HF_TOKEN) |
| `qwen-native` | Toutes | Aucune (Rust pur) | ~3s a chaud | Oui |
| `kokoro` | macOS (opt-in) | Aucune (Rust pur) | ~2-5s | Non |

### Trait TtsBackend

```rust
pub trait TtsBackend {
    fn name(&self) -> &str;
    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()>;
    fn list_voices(&self) -> Result<Vec<String>>;
    fn is_available(&self) -> bool;
}
```

Chaque backend implemente ce trait. Le dispatch se fait via `get_backend(name)` dans `backend/mod.rs`.

### Defaut par plateforme

- **macOS**: `say` (zero latence, voix systeme)
- **Linux / Windows**: `kokoro` (Rust pur, pas de dependance Python)

## SpeakOptions

Structure centrale passee a chaque backend via `TtsBackend::speak()`. Tous les champs sont optionnels — les backends ignorent ceux qu'ils ne supportent pas.

```rust
pub struct SpeakOptions {
    pub voice: Option<String>,      // Nom de voix (ex: "Chelsie", "af_heart")
    pub lang: Option<String>,       // Code langue ISO (ex: "fr", "en")
    pub rate: Option<u32>,          // Debit en mots/min (say uniquement)
    pub gender: Option<String>,     // "feminine" | "masculine"
    pub style: Option<String>,      // "calm" | "energetic" | "warm" | ...
    pub ref_audio: Option<String>,  // Chemin audio pour voice cloning
    pub ref_text: Option<String>,   // Transcription de l'audio de reference
    pub model: Option<String>,      // Model ID (ex: Qwen/Qwen3-TTS-12Hz-0.6B-Base)
}
```

**Resolution de priorite** : flags CLI / params MCP > preferences DB > valeurs par defaut du backend.

## Resolution du voice cloning

Quand un utilisateur demande `-v patrick` (ou `voice: "patrick"` via MCP), le systeme :

1. Cherche un clone nomme `patrick` dans la table `voice_clones` via `clone::resolve_voice()`
2. Si trouve : extrait `ref_audio` et `ref_text` du clone
3. Verifie le backend courant — si c'est `say` ou `kokoro` (qui ne supportent pas le cloning), **bascule automatiquement** :
   - **Linux / Windows** : vers `qwen-native` (Rust pur)
4. Met `voice = None` (ne pas passer le nom du clone comme voix au backend)
5. Passe `ref_audio` + `ref_text` dans `SpeakOptions`

Ce mecanisme est identique dans le CLI (`main.rs::handle_speak`) et le serveur MCP (`mcp.rs::tool_speak`).

## Playback audio asynchrone (PlayHandle)

Le module `audio.rs` fournit deux modes de lecture via `rodio` :

- **`play_audio_blocking(path)`** — bloque le thread jusqu'a la fin de la lecture. Supporte WAV, MP3, OGG, FLAC.
- **`play_wav_async(path)`** — lance la lecture dans un thread et retourne un `PlayHandle`.

```rust
pub struct PlayHandle {
    join: Option<thread::JoinHandle<Result<()>>>,
}

impl PlayHandle {
    pub fn wait(self) -> Result<()>;  // Bloque jusqu'a la fin
}

impl Drop for PlayHandle {
    fn drop(&mut self);  // Attend la fin du thread au drop
}
```

Le pattern `PlayHandle` est utilise par le backend `qwen` pour le pipeline de chunking : pendant que le chunk N est joue, le chunk N+1 est genere en parallele.

## Pipeline de decoupage par phrases (qwen backend)

Le backend `qwen` decoupe le texte long en phrases pour reduire la latence percue :

1. **Split** : decoupe sur `.` `!` `?` `;`
2. **Merge** : fusionne les petites phrases consecutives tant que `len < MIN_CHUNK_CHARS` (120 caracteres) — pour reduire le nombre d'appels subprocess Python
3. **Pipeline** :
   - Si 1 seul chunk : appel direct avec `--play --stream` (latence optimale)
   - Si N chunks : pipeline chevauche (overlap generation + playback) :
     - Genere chunk 0 → joue chunk 0 (async) + genere chunk 1 en parallele
     - Quand chunk 1 genere → attend fin chunk 0 → joue chunk 1 + genere chunk 2...
     - Resultat : la latence inter-chunks est masquee

## Base de donnees

SQLite via `rusqlite` avec WAL mode. Fichier: `~/.config/vox/vox.db`.

### Schema DDL complet

```sql
CREATE TABLE IF NOT EXISTS preferences (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    backend TEXT,
    voice   TEXT,
    lang    TEXT,
    rate    INTEGER,
    gender  TEXT,
    style   TEXT,
    model   TEXT
);
-- Migration ajoutee dynamiquement pour les bases existantes :
-- ALTER TABLE preferences ADD COLUMN pack TEXT;

CREATE TABLE IF NOT EXISTS usage_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now')),
    backend     TEXT NOT NULL,
    voice       TEXT,
    lang        TEXT,
    text_len    INTEGER NOT NULL,
    duration_ms INTEGER
);

CREATE TABLE IF NOT EXISTS voice_clones (
    name       TEXT PRIMARY KEY,
    ref_audio  TEXT NOT NULL,
    ref_text   TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S','now'))
);
```

**Migration** : la colonne `pack` sur `preferences` est ajoutee via `ALTER TABLE` si absente (detection par `SELECT pack FROM preferences LIMIT 0`). Cela permet la compatibilite avec les bases creees avant cette fonctionnalite.

**UPSERT** : `set_preference()` insere d'abord une ligne vide avec `ON CONFLICT(id) DO NOTHING`, puis fait `UPDATE`. La contrainte `CHECK (id = 1)` garantit une seule ligne.

### Requetes d'agregation

| Fonction | Requete | Retour |
|----------|---------|--------|
| `get_usage_summary()` | `SELECT COUNT(*), SUM(text_len)` | `(u64, u64)` — total calls + total chars |
| `get_backend_stats()` | `GROUP BY backend` + COUNT/SUM | `Vec<BackendStats>` — calls, chars, duration par backend |
| `get_lang_stats()` | `GROUP BY lang` + COUNT | `Vec<LangStats>` — calls par langue |
| `get_total_duration_ms()` | `SUM(duration_ms)` | `u64` — temps de parole cumule en ms |
| `get_usage_stats()` | `ORDER BY id DESC LIMIT 50` | `Vec<UsageEntry>` — 50 dernieres entrees |

## Strategie de securite

| Vecteur | Protection | Implementation |
|---------|-----------|----------------|
| SQL injection | Parametres lies (`?1`, `?2`, ...) | `rusqlite::params![]` partout, jamais d'interpolation de valeurs utilisateur |
| Cles de preferences invalides | Whitelist validee | `set_preference()` valide `key` contre `["backend", "voice", "lang", "rate", "gender", "style", "model", "pack"]` |
| Valeurs de preferences invalides | Validation par type/enum | `gender` → `Gender::parse()`, `style` → `IntonationStyle::parse()`, `rate` → `parse::<u32>()`, `lang` → `SUPPORTED_LANGS.contains()`, `backend` → whitelist plateforme |
| Path traversal (audio) | Extensions validees | `validate_audio()` verifie existence + extension dans `[wav, mp3, flac, ogg, m4a]` |
| Injection shell | Pas de `sh -c` | Toutes les commandes externes via `std::process::Command` avec args separes |
| Backends invalides | Enum par plateforme | macOS: `[kokoro, say, qwen, qwen-native]`, autres: `[kokoro, qwen-native]` |

## Latence par backend : cold start vs warm start

| Backend | Cold start | Warm start | Notes |
|---------|-----------|------------|-------|
| `say` | ~100ms | ~100ms | Pas de modele a charger, appel systeme direct |
| `kokoro` | ~5-8s | ~2-5s | Chargement ONNX + voices.bin via Python. Pas de cache persistent |
| `qwen` | ~5-15s | ~1-2s | Cold = telechargement modele (~1.2 GB) + chargement Python. Warm = Python startup seul |
| `qwen-native` | ~10-30s | ~2-5s | Cold = telechargement HuggingFace + chargement candle. Warm = modele en memoire (`static Mutex<Option<Qwen3TTS>>`) |

**Note** : `qwen-native` garde le modele en memoire via un `Mutex` global — les appels suivants au meme processus (ex: serveur MCP) sont donc en warm start. Les appels CLI individuels sont toujours en cold start.

## Protocole MCP

Serveur JSON-RPC 2.0 sur stdio. Compatible MCP spec `2024-11-05`.

### Lifecycle

1. Client envoie `initialize` → serveur repond avec capabilities + tools
2. Client envoie `initialized` (notification)
3. Client appelle `tools/call` avec `name` et `arguments`
4. Serveur repond avec `content[{type: "text", text: "..."}]`

### 14 outils MCP exposes

| Tool | Description |
|------|-------------|
| `vox_speak` | Synthetise et joue du texte (params: text, voice, lang, backend) |
| `vox_list_voices` | Liste les voix disponibles pour un backend |
| `vox_clone_list` | Liste les voice clones enregistres |
| `vox_clone_add` | Ajoute un voice clone (name, audio_path, ref_text) |
| `vox_clone_remove` | Supprime un voice clone |
| `vox_config_show` | Affiche les preferences courantes |
| `vox_config_set` | Modifie une preference (key, value) |
| `vox_stats` | Statistiques d'utilisation |
| `vox_pack_list` | Liste les sound packs installes/disponibles |
| `vox_pack_install` | Installe un sound pack |
| `vox_pack_set` | Active un sound pack |
| `vox_pack_play` | Joue un son d'un pack (category) |
| `vox_pack_remove` | Supprime un sound pack |
| `vox_hear` | Enregistre et transcrit (STT Whisper local, toutes plateformes) |

## Compilation conditionnelle

```rust
#[cfg(target_os = "macos")]   // say, qwen, stt, chat
#[cfg(not(target_os = "macos"))] // kokoro comme defaut
```

Feature flags Cargo:
- `metal` — GPU Apple Silicon (Metal + Accelerate) pour qwen-native
- `cuda` — GPU NVIDIA pour qwen-native

## Securite

- **SQL injection**: Toutes les requetes utilisent des parametres lies (`?`). Les cles de preference sont validees par whitelist.
- **Path traversal**: Extensions audio validees (wav, mp3, flac, ogg, m4a). Fichiers verifies existants.
- **Input validation**: Backends, langues, gender, style valides par enum/whitelist.
- **Pas de shell**: Les commandes externes utilisent `std::process::Command` (pas de `sh -c`).

## CI/CD

- GitHub Actions: matrix macOS / Ubuntu / Windows
- release-please: `feat:` → version bump PR → merge → release + binaires
- Binaires: aarch64-apple-darwin, x86_64-apple-darwin (metal), x86_64-unknown-linux-gnu, x86_64-pc-windows-msvc
- Tests: `cargo test` (unitaires + integration UX/security/perf)
