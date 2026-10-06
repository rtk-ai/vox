# Architecture technique

## Vue d'ensemble

vox est un binaire Rust unique. Il n'y a pas de Python, ni a la compilation ni a l'execution. Il fait trois choses : la synthese vocale (TTS), la reconnaissance vocale (STT) avec Whisper, et un serveur MCP (Model Context Protocol) qui expose ces fonctions aux assistants IA.

Trois backends TTS sont presents dans tous les builds (`pocket`, `piper`, `qwen-native`). `say` s'y ajoute sur macOS, et `kokoro` dans un build compile avec `--features kokoro`.

```
                              vox (un binaire Rust)
                                       |
            +--------------------------+---------------------------+
            |                                                      |
       speak (TTS)                                            hear (STT)
            |                                                      |
   +--------+--------+-------------+-----------+                Whisper
   |        |        |             |           |                (candle)
 pocket   piper   qwen-native    kokoro       say              99 langues
(candle)  (ONNX)   (candle)      (ONNX)     (macOS)          CPU, ou GPU en
  CPU      CPU    CPU, ou GPU    feature   /usr/bin/say     build Metal/CUDA
                  en build       kokoro
                  Metal/CUDA
   |        |        |             |                               |
   +--------+        +-------------+                         cpal (capture)
   Player (rodio,    WAV temporaire,
   echantillons)     puis rodio
```

## Modules source

```
src/
  main.rs         CLI (clap) : parsing des arguments, dispatch des sous-commandes, routage vers le daemon
  lib.rs          Declaration des modules publics
  mcp.rs          Serveur MCP JSON-RPC sur stdio (14 outils)
  backend/
    mod.rs        Trait TtsBackend, SpeakOptions, get_backend(), supported_backends()
    pocket.rs     Kyutai pocket-tts (candle, CPU, anglais) : backend par defaut pour l'anglais
                  et quand aucune langue n'est donnee
    piper.rs      Piper (ONNX Runtime via piper-rs, espeak-ng embarque, une voix par defaut
                  par langue, les autres par leur nom) : backend par defaut des autres langues
    qwen_native.rs  Qwen3-TTS (candle, clonage de voix, GPU en build Metal ou CUDA)
    kokoro.rs     Kokoro (ONNX, crate kokoro-tts) : compile seulement avec la feature kokoro
    say.rs        /usr/bin/say : compile seulement sur macOS
    pocket_config_b6369a24.yaml  Config du modele pocket, embarquee dans le binaire
  config.rs       Repertoire de config, backend par defaut selon la langue, langues supportees,
                  enums (Gender, IntonationStyle), lecture de models.toml
  db.rs           SQLite (rusqlite) : preferences, clones, journal d'utilisation, stats
  daemon.rs       Daemon HTTP local qui garde les modeles charges (vox daemon start|stop|status)
  init.rs         vox init : configuration MCP des outils IA installes (14 connus), bloc CLAUDE.md,
                  hook Stop
  input.rs        Lecture du texte (arguments ou stdin)
  lang.rs         Resolution de langue pour vox init et les instructions MCP
                  (drapeau > preference > locale systeme)
  clone.rs        Clonage de voix : validation du fichier audio, enregistrement micro
  pack.rs         Sound packs (compatibles peon-ping)
  audio.rs        Lecture via rodio : Player (echantillons directs), deliver() (jouer ou ecrire
                  un WAV), bips de debut et de fin d'ecoute
  levels.rs       Spectre de l'audio joue et annonce now-playing.json
  timing.rs       Chronometrage des etapes sur stderr (VOX_TIMINGS=1)
  accel.rs        Acceleration du build (Metal, CUDA ou CPU), ligne acceleration: de vox config show
  mic.rs          Capture micro via cpal, VAD par energie, reechantillonnage a 16 kHz
  stt/            Speech-to-text Whisper sur candle (toutes plateformes)
    mod.rs        Cache du modele par processus, choix du modele, transcribe / transcribe_samples
    whisper.rs    Inference (mel, encodeur, decodage greedy avec repli en temperature)
  chat/           vox chat : conversation vocale qui appelle l'API Claude (macOS uniquement)
    mod.rs, claude_api.rs, sentence.rs, streaming.rs
  tui.rs          vox setup : configuration interactive (ratatui)
build.rs          Copie espeak-ng-data dans OUT_DIR pour l'embarquer dans le binaire
models.toml       Identifiants de modeles par defaut, embarques dans le binaire
plugins/vox/      Plugin Claude Code : visualiseur de voix
  .claude-plugin/plugin.json
  hooks/hooks.json, hooks/register.js, hooks/wave.js
  tests/vox.test.ts
.claude-plugin/marketplace.json  Marketplace qui liste le plugin
```

Le repertoire de config est `dirs::config_dir()/vox` (`config::config_dir()`) : `~/Library/Application Support/vox` sur macOS, `~/.config/vox` sur Linux. La variable `VOX_CONFIG_DIR` le remplace. Un fichier `models.toml` place dans ce repertoire remplace celui du binaire.

## Backends TTS

| Backend | Disponibilite | Moteur | Peripherique | Langues | Clonage de voix |
|---------|---------------|--------|--------------|---------|-----------------|
| `pocket` | Tous les builds | Kyutai pocket-tts sur candle | CPU | Anglais, 8 voix predefinies | Oui, avec `HF_TOKEN` et le checkpoint a acces restreint |
| `piper` | Tous les builds | Piper sur ONNX Runtime (piper-rs), espeak-ng embarque | CPU | 11 langues avec une voix par defaut (pas de japonais) ; une autre voix piper se choisit par son nom (`-v fr_FR-siwis-low`) | Non |
| `qwen-native` | Tous les builds | Qwen3-TTS sur candle | GPU en build Metal ou CUDA, sinon CPU | en, fr, es, de, it, pt, zh, ja, ko, ru | Oui |
| `kokoro` | Build `--features kokoro` | Kokoro sur ONNX (crate kokoro-tts) | Non verifie | Voix listees dans `kokoro.rs` | Non |
| `say` | macOS | `/usr/bin/say` | Systeme | Voix systeme | Non |

Un build de release standard ne contient pas `kokoro` : il repond `Unknown backend: kokoro`. Les fichiers du modele kokoro ne sont pas telecharges par vox ; le message d'erreur du backend donne les commandes `curl`.

Les modeles sont telecharges au premier usage : pocket et qwen-native depuis le hub Hugging Face ; piper telecharge la voix choisie (un fichier `.onnx` et son `.onnx.json`) dans le sous-repertoire `piper/` du repertoire de config.

Avec `qwen-native` et sans clone de voix, `-l` n'a pas d'effet : le modele deduit la langue du texte, et vox ecrit un avertissement sur stderr. Les codes `ar` et `nl` sont refuses par ce backend.

Les latences mesurees sont dans la section "Latence par backend".

### Trait TtsBackend

```rust
pub trait TtsBackend {
    fn name(&self) -> &str;
    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()>;
    fn list_voices(&self) -> Result<Vec<String>>;
    fn is_available(&self) -> bool;
}
```

Chaque backend implemente ce trait. Le dispatch se fait via `get_backend(name)` dans `backend/mod.rs`. `supported_backends()` donne la liste des backends compiles dans le build courant.

### Backend par defaut

Le defaut est le meme sur toutes les plateformes (`config::default_backend_for_lang`) :

- langue absente ou `en` : `pocket` (`config::DEFAULT_BACKEND`)
- `ja` : `qwen-native`
- toute autre langue : `piper`

Le checkpoint pocket ne parle que l'anglais, d'ou le repli sur piper. piper n'a
pas de voix japonaise utilisable (la seule publiee demande un phonemiseur que
vox n'a pas), d'ou qwen-native pour le japonais.

Ordre de resolution :

- Serveur MCP (`mcp.rs::speak_request`) : parametre `backend` > preference `backend` > defaut selon la langue.
- CLI (`main.rs::handle_speak`) : `-b` > preference `backend` > defaut selon la langue. Un `-b` tape l'emporte toujours, `-b pocket` compris.
- Langue : `-l` ou parametre `lang` > preference `lang`. Sans langue, `piper`, `qwen-native` et `kokoro` prennent `en`.

## SpeakOptions

Structure centrale passee a chaque backend via `TtsBackend::speak()`. Les backends ignorent les champs qu'ils ne supportent pas.

```rust
pub struct SpeakOptions {
    pub voice: Option<String>,      // Nom de voix (ex: "alba" pour pocket, "af_heart" pour kokoro)
    pub lang: Option<String>,       // Code langue (ex: "fr", "en")
    pub rate: Option<u32>,          // Debit en mots/min (say uniquement)
    pub gender: Option<String>,     // "feminine" | "masculine"
    pub style: Option<String>,      // "calm" | "energetic" | "warm" | ...
    pub ref_audio: Option<String>,  // Chemin audio pour le clonage de voix
    pub ref_text: Option<String>,   // Transcription de l'audio de reference
    pub model: Option<String>,      // Model ID (qwen-native uniquement)
    pub volume: f32,                // Multiplicateur de volume, 1.0 par defaut
    pub output: Option<PathBuf>,    // Ecrire l'audio dans ce fichier au lieu de le jouer
}
```

`gender` et `style` sont acceptes et transmis, mais aucun backend actuel ne les lit. `piper` lit `voice` quand c'est un nom de voix piper (par exemple `fr_FR-siwis-low`) : cette voix l'emporte sur celle de la langue. Pour un autre nom, il ecrit une note sur stderr et prend la voix par defaut de la langue.

`output` n'est rempli que par le CLI : par `-o`, et par `vox bench`, qui rend chaque backend dans un fichier temporaire. Le serveur MCP ne l'expose pas, pour qu'un agent ne puisse pas ecrire un fichier arbitraire. `model` n'a pas de parametre MCP : le serveur transmet la preference `model` enregistree.

**Resolution de priorite** : flags CLI / params MCP > preferences DB > valeurs par defaut.

## Resolution du voice cloning

Quand un utilisateur demande `-v patrick` (ou `voice: "patrick"` via MCP), le systeme :

1. Cherche un clone nomme `patrick` dans la table `voice_clones` via `clone::resolve_voice()`
2. Si trouve : extrait `ref_audio` et `ref_text` du clone
3. Choisit le backend avec `clone::speak_backend()`, la meme regle pour le CLI (`main.rs::handle_speak`) et le serveur MCP (`mcp.rs::speak_request`) :
   - un backend donne pour cet appel (`-b`, parametre `backend`) est garde tel quel, meme s'il ne sait pas cloner
   - sinon, le backend de la preference ou le defaut selon la langue est garde si c'est `qwen-native`, ou si c'est `pocket` et que `HF_TOKEN` est defini ; dans tous les autres cas le clone part vers `qwen-native`
4. Met `voice = None` (ne pas passer le nom du clone comme voix au backend)
5. Passe `ref_audio` + `ref_text` dans `SpeakOptions`

Seuls `qwen-native` et `pocket` savent cloner (`clone::can_clone`). Quand le backend donne pour l'appel est un autre, il parle avec sa propre voix et vox dit que le clone est ignore : une note sur stderr pour le CLI, une note dans le resultat de `vox_speak` pour le serveur MCP. Avec `pocket`, le clonage depuis un `.wav` demande `HF_TOKEN` (poids a acces restreint) ; sans lui le backend renvoie une erreur.

## Playback audio

Le module `audio.rs` joue le son avec `rodio`. Il y a deux chemins.

### Player : echantillons directs (piper, pocket)

`piper` et `pocket` n'ecrivent pas de fichier pour parler. Ils passent par `audio::Player` :

```rust
pub struct Player { /* canal mpsc + thread */ }

impl Player {
    pub fn start(volume: f32) -> Self;                             // ouvre le peripherique en arriere-plan
    pub fn push(&self, sample_rate: u32, samples: Vec<f32>) -> bool; // met des echantillons mono en file
    pub fn finish(self) -> Result<()>;                             // ferme la file, bloque jusqu'a la fin
}
```

- `Player::start` lance un thread (`play_chunks`) qui ouvre le peripherique de sortie, puis rend la main tout de suite. Le backend l'appelle avant de charger son modele : l'ouverture du peripherique et le chargement du modele se font en parallele.
- Les echantillons arrivent au thread par un canal `mpsc` et sont ajoutes au `Sink` rodio a mesure. `piper` pousse l'enonce entier en une fois. `pocket` pousse chaque trame produite par `generate_stream` : le son commence pendant que la generation continue.
- Le volume est applique dans ce thread, echantillon par echantillon.
- `push` renvoie `false` quand le thread s'est arrete, c'est-a-dire quand le peripherique n'a pas pu s'ouvrir. `pocket` cesse alors de generer. `finish` renvoie l'erreur du thread.
- Le peripherique vit sur le thread du Player parce qu'un flux de sortie ne peut pas changer de thread sur toutes les plateformes. C'est aussi ce thread qui tient l'annonce `now-playing.json` a jour (section "Annonce de lecture").

### deliver : jouer ou ecrire un WAV

`audio::deliver(rendered, destination)` prend un WAV deja rendu. Sans destination il le joue (`play_wav_blocking`, qui bloque jusqu'a la fin). Avec une destination il le copie tel quel, sans re-encodage, et cree le repertoire parent au besoin.

- `qwen-native` et `kokoro` rendent toujours un WAV temporaire puis appellent `deliver`.
- `piper` et `pocket` ne passent par `deliver` qu'avec `-o` : ils rendent alors un WAV temporaire, appliquent le volume (`apply_wav_gain`) et le copient.
- `say` ne passe pas par `audio.rs` quand il joue au volume 1.0 : `/usr/bin/say` joue lui-meme. Avec `-o` ou un autre `--volume`, vox lui fait rendre un WAV temporaire, applique le volume (`apply_wav_gain`) et appelle `deliver`.

`-o fichier.wav` ecrit donc un WAV au lieu de jouer, sur tous les backends.

`play_audio_blocking(path)` lit WAV, MP3, OGG et FLAC. Les sound packs l'utilisent directement (`pack::play`).

`play_wav_async(path)` et `PlayHandle` existent encore dans `audio.rs` mais plus aucun code ne les appelle. Ils servaient a un backend supprime.

### Bips d'ecoute

`mic.rs` joue un bip avant d'ouvrir le micro et un second quand l'enregistrement s'arrete (`audio::play_cue`, `Cue::Start` et `Cue::Stop`). `VOX_CUES=0` les desactive.

## Annonce de lecture : now-playing.json

Pendant qu'il joue, vox ecrit le spectre de l'audio dans `now-playing.json`, dans son repertoire de config, et supprime le fichier a la fin (`src/levels.rs`). Le plugin Claude Code lit ce fichier pour dessiner le visualiseur. Il n'y a ni flux ni socket : le lecteur se synchronise avec l'heure de debut.

### Format

```json
{"version":1,"pid":4242,"started_ms":1791185353123,"frame_ms":50,"bands":20,"complete":true,"frames":[[0,12,40],[3,18,55]]}
```

(Exemple abrege : chaque ligne de `frames` contient `bands` valeurs.)

| Champ | Sens |
|-------|------|
| `version` | Version du format, `1` |
| `pid` | Processus vox qui joue |
| `started_ms` | Debut de la lecture, en millisecondes depuis l'epoque Unix. Ne change jamais pendant une lecture |
| `frame_ms` | Duree d'une ligne de `frames` : 50 ms (`levels::FRAME_MS`) |
| `bands` | Nombre de barres par ligne : 20 (`levels::BANDS`), reparties sur une echelle logarithmique de 100 Hz a 8 kHz |
| `complete` | `false` tant que le backend genere encore : d'autres lignes vont suivre |
| `frames` | Une ligne par tranche de `frame_ms`. Chaque valeur va de 0 a 100, relative a la barre la plus forte de l'enonce |

### Cycle de vie

- Le fichier est ecrit a cote puis renomme (`now-playing.json.<pid>`) : un lecteur ne voit jamais un fichier a moitie ecrit.
- Un backend qui rend tout l'enonce avant de jouer annonce tout d'un coup, avec `complete: true` (`NowPlaying::start`, appele par `play_audio_blocking`).
- Avec le Player, l'annonce commence au premier bloc mis en file (`NowPlaying::begin`), avec `complete: false`. Le fichier est reecrit au plus toutes les 200 ms (`ANNOUNCE_EVERY`) avec les lignes connues, puis une derniere fois avec `complete: true` quand le backend a fini de generer.
- Tant que l'annonce est incomplete, les valeurs sont relatives a la barre la plus forte entendue jusque-la : une reecriture peut changer les lignes deja publiees.
- Le fichier est supprime quand la lecture se termine (`Drop` de `NowPlaying`), et seulement s'il porte encore le `pid` et le `started_ms` de cette lecture : un vox ne supprime pas l'annonce d'un autre.
- Tout est en best effort. Un echec d'analyse ou d'ecriture ne coupe jamais le son.

Ce qui annonce : `piper` et `pocket` (Player), `qwen-native`, `kokoro`, `say` avec un `--volume` different de 1.0 et les sound packs (`play_audio_blocking`). Ce qui n'annonce pas : `say` au volume 1.0, qui joue hors de vox, tout ce que dit `vox chat` (message d'accueil, reponses et au revoir : dits par `/usr/bin/say`, ou lus par leur propre `Sink` dans `chat/streaming.rs` avec un clone de voix), les bips d'ecoute, et tout appel avec `-o`. Quand la requete passe par le daemon, c'est le daemon qui joue et qui annonce.

### Contrat avec le plugin

Le plugin (`plugins/vox/hooks/register.js`) :

- cherche le fichier dans `VOX_CONFIG_DIR`, sinon `%APPDATA%/vox`, sinon `~/Library/Application Support/vox` si le repertoire `~/Library/Application Support` existe, sinon `$XDG_CONFIG_HOME/vox` ou `~/.config/vox` ;
- ignore le fichier si `version` n'est pas `1`, si `frames` n'est pas un tableau ou si `frame_ms` n'est pas positif ;
- affiche la ligne d'indice `(maintenant - started_ms) / frame_ms`. L'horloge du plugin et celle de vox doivent donc etre la meme ;
- ne lit ni `pid` ni `bands` : il reechantillonne chaque ligne sur le nombre de barres qu'il dessine ;
- traite un fichier sans champ `complete` comme complet ;
- tant que `complete` est `false`, relit le fichier au plus toutes les 100 ms. Si la ligne courante n'est pas encore publiee, il garde la derniere pendant 40 lignes au plus (2 s), puis cesse d'afficher le spectre. Si le fichier disparait avant d'etre complet, il cesse aussi. Dans les deux cas il revient au comportement sans annonce, decrit plus bas ;
- ne rejoue pas une annonce laissee par un vox qui est mort : son `started_ms` est passe, aucune ligne ne correspond a l'instant present.

Sans annonce, le plugin affiche une animation synthetique (`vox · preparing`, ou `vox · listening` pendant `vox hear`). Pour le hook Stop, il ne dessine rien tant qu'aucune annonce n'apparait.

### Plugin Claude Code

Le plugin vit dans `plugins/vox` et le depot sert de marketplace (`.claude-plugin/marketplace.json`). Installation, dans une session Claude Code 2.1.287 ou plus recent :

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
```

`/vox-wave` affiche un apercu de l'animation et regle les couleurs (`/vox-wave color ocean`).

Le plugin reagit aux outils MCP `vox_speak`, `vox_hear` et `vox_pack_play`, aux commandes `vox ...` lancees par l'outil Bash, et au hook Stop.

Un agent ne peut pas decouvrir un plugin qui n'est pas installe. `init::plugin_note()` ajoute donc les deux commandes d'installation au bloc CLAUDE.md ecrit par `vox init -m cli` et aux instructions du serveur MCP.

## Daemon

`vox daemon start` lance un second processus vox qui garde les modeles charges. Il n'est jamais demarre automatiquement.

```bash
vox daemon start     # --idle-timeout <secondes>, 300 par defaut, 0 = jamais
vox daemon status
vox daemon stop
```

- Le daemon est un serveur HTTP sur `127.0.0.1:19876` (`VOX_DAEMON_PORT` change le port). Routes : `GET /health`, `POST /speak`, `POST /shutdown`.
- Il ecrit son PID dans `daemon.pid`, dans le repertoire de config, une fois le port ouvert. Sa sortie d'erreur va dans `daemon.log`, dans le meme repertoire ; le fichier est remis a zero a chaque demarrage et reste apres l'arret.
- Il s'arrete apres le delai d'inactivite, compte depuis la fin de la derniere requete, et jamais pendant qu'une requete est servie. Avec `--idle-timeout 0` il ne s'arrete pas seul.
- Les requetes `/speak` sont traitees une par une (`speak_lock`).

Routage (`main.rs::handle_speak`) : une requete part vers le daemon quand les trois conditions sont reunies.

1. Le backend est `pocket`, `piper`, `qwen-native` ou `kokoro`.
2. `-o` n'est pas donne.
3. Le daemon repond a `GET /health` (`daemon::is_running`, delai de 500 ms).

Sinon le backend tourne dans le processus du CLI. `say` ne passe jamais par le daemon. `-o` contourne le daemon expres : un chemin relatif y serait resolu par rapport au repertoire de travail du daemon, pas a celui de l'utilisateur.

Le daemon joue le son lui-meme : le CLI envoie le texte et les options (`daemon::speak_via_daemon`) et attend la reponse.

Le serveur MCP (`vox serve`) et `vox hear` n'utilisent pas le daemon. Le serveur MCP appelle les backends dans son propre processus, ou les modeles restent aussi charges d'un appel a l'autre.

## Decoupage par phrases (vox chat)

`vox chat` (macOS uniquement, `ANTHROPIC_API_KEY` requis) enchaine micro, Whisper, API Claude en streaming, puis TTS. La reponse de Claude est decoupee en phrases a mesure qu'elle arrive (`chat/sentence.rs`, `SentenceAccumulator`) :

1. Chaque fragment de texte est ajoute a un tampon.
2. Une phrase est emise quand un `.`, `!`, `?` ou `;` arrive et que le tampon fait au moins `STREAMING_MIN_CHUNK_CHARS` (60, compte en octets).
3. Le reste est emis a la fin de la reponse (`flush`).

Chaque phrase part vers un thread TTS par un canal `mpsc`, pendant que le texte continue d'arriver. Le TTS est `qwen-native` avec un clone de voix, sinon `/usr/bin/say` (`chat/streaming.rs`, `TtsStrategy`). Le message d'accueil et l'au revoir passent par le meme choix que les reponses. L'enregistrement s'arrete quand l'utilisateur appuie sur Entree.

La boucle ecouter, reflechir, parler du serveur MCP (`vox_hear` puis `vox_speak`) n'utilise pas ce code et ne demande pas de cle API : l'assistant qui appelle les outils fait la reflexion.

## Base de donnees

SQLite via `rusqlite` avec WAL mode. Fichier : `vox.db` dans le repertoire de config (`config::db_path()`). `VOX_DB_PATH` le remplace.

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
-- Migrations ajoutees dynamiquement pour les bases existantes :
-- ALTER TABLE preferences ADD COLUMN pack TEXT;
-- ALTER TABLE preferences ADD COLUMN stt_model TEXT;

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

**Migration** : les colonnes `pack` et `stt_model` de `preferences` sont ajoutees via `ALTER TABLE` si elles sont absentes (detection par `SELECT <colonne> FROM preferences LIMIT 0`). Cela garde la compatibilite avec les bases creees avant ces fonctionnalites.

**UPSERT** : `set_preference()` insere d'abord une ligne vide avec `ON CONFLICT(id) DO NOTHING`, puis fait `UPDATE`. La contrainte `CHECK (id = 1)` garantit une seule ligne.

### Requetes d'agregation

| Fonction | Requete | Retour |
|----------|---------|--------|
| `get_usage_summary()` | `SELECT COUNT(*), SUM(text_len)` | `(u64, u64)` — total calls + total chars |
| `get_backend_stats()` | `GROUP BY backend` + COUNT/SUM | `Vec<BackendStats>` — calls, chars, duration par backend |
| `get_lang_stats()` | `GROUP BY lang` + COUNT | `Vec<LangStats>` — calls par langue |
| `get_total_duration_ms()` | `SUM(duration_ms)` | `u64` — duree cumulee des appels en ms |
| `get_usage_stats()` | `ORDER BY id DESC LIMIT 50` | `Vec<UsageEntry>` — 50 dernieres entrees |

`duration_ms` est la duree de l'appel a `speak` (ou de la requete au daemon), mesuree par l'appelant.

## Strategie de securite

| Vecteur | Protection | Implementation |
|---------|-----------|----------------|
| SQL injection | Parametres lies (`?1`, `?2`, ...) | `rusqlite::params![]`, jamais d'interpolation de valeurs utilisateur. Le nom de colonne de `set_preference()` est interpole apres validation par whitelist |
| Cles de preferences invalides | Whitelist validee | `set_preference()` valide `key` contre `["backend", "voice", "lang", "rate", "gender", "style", "model", "stt_model", "pack"]` |
| Valeurs de preferences invalides | Validation par type/enum | `gender` → `Gender::parse()`, `style` → `IntonationStyle::parse()`, `rate` → `parse::<u32>()`, `lang` → `SUPPORTED_LANGS.contains()`, `backend` → `backend::supported_backends()` |
| Backends invalides | Liste du build courant | `supported_backends()` : `piper`, `pocket`, `qwen-native`, plus `kokoro` avec la feature `kokoro`, plus `say` sur macOS |
| Fichier audio de clone | Existence, extension et decodage | `validate_audio()` verifie que le fichier existe et que son extension est dans `[wav, mp3, flac, ogg]`. `clone::add_clone_from_file()` le decode ensuite et refuse un fichier dont aucun son ne peut etre lu |
| Path traversal (sound packs) | Nom reduit a un seul composant | `pack::validate_pack_name()` refuse `.`, `..`, les separateurs et les chemins absolus, pour le nom du pack comme pour les fichiers du manifeste |
| Ecriture de fichier par un agent | `output` non expose via MCP | `mcp.rs::speak_request` met toujours `output: None` |
| Injection shell | Pas de `sh -c` | Toutes les commandes externes passent par `std::process::Command` avec des arguments separes |

## Latence par backend

Les temps de generation mesures sur macOS, Linux et Windows avec les binaires
de la v0.17.0 sont dans [BENCHMARKS.md](BENCHMARKS.md).

Mesures du 2026-10-04 et du 2026-10-05 : temps entre le lancement de vox et le premier son, phrase de 3 secondes, portable Apple A18 Pro (8 Go, mode economie d'energie), build release Metal.

| Backend | Sans daemon | Avec daemon |
|---------|-------------|-------------|
| `pocket` (anglais) | 0,18 a 0,55 s | Non mesure |
| `piper` (francais) | environ 0,75 s | 0,26 a 0,32 s |

Sur la meme machine, un texte anglais long (15 s d'audio) commence a etre joue apres 0,81 s.

Les autres backends n'ont pas ete mesures jusqu'au premier son :

- `qwen-native` : 9 a 12 s entre le lancement et la fin de l'ecriture du WAV, pour une phrase de 2,3 a 2,9 s d'audio (trois essais, `-o`, nouveau processus a chaque fois, modele deja sur disque, meme portable, build Metal, mode economie d'energie desactive). Ce backend rend tout l'enonce avant de jouer. Ce document donnait auparavant environ 2 a 5 s quand le modele est deja charge, sans dire sur quelle machine ; cette valeur n'a pas ete remesuree.
- `say` : ce document donnait environ 100 ms, sans dire sur quelle machine ; non remesure.

Whisper `base` sur 24,6 s d'audio, Linux x86_64 12 coeurs (WSL2) avec une RTX 4070 Ti SUPER : environ 26 s avec le build CPU, environ 1,4 s avec le build CUDA, meme transcription.

Chaque backend a modele garde son modele dans une variable statique du processus (`MODEL` dans `pocket.rs`, `piper.rs`, `qwen_native.rs`, `kokoro.rs` et `stt/mod.rs`). Un appel CLI est un nouveau processus : il recharge le modele a chaque fois, sauf s'il passe par le daemon. Le serveur MCP et le daemon gardent le modele d'un appel a l'autre. `piper` recharge quand la voix change, `qwen-native` et Whisper quand l'identifiant du modele change.

`VOX_TIMINGS=1` ecrit sur stderr le temps de chaque etape d'un enonce (`timing::mark`) : une ligne `[timing]` par etape, avec le temps depuis le lancement et depuis l'etape precedente. Les etapes sont marquees dans `main.rs`, `piper.rs`, `pocket.rs` et `audio.rs`.

## Protocole MCP

Serveur JSON-RPC 2.0 sur stdio, lance par `vox serve`. Version de protocole annoncee : `2024-11-05`.

### Lifecycle

1. Client envoie `initialize` → serveur repond avec `protocolVersion`, `capabilities`, `serverInfo` et `instructions`
2. Client envoie `initialized` (notification, sans reponse : les messages sans `id` sont ignores)
3. Client envoie `tools/list` → serveur repond avec la liste des outils
4. Client appelle `tools/call` avec `name` et `arguments`
5. Serveur repond avec `content[{type: "text", text: "..."}]`, et `isError: true` quand l'outil echoue

`ping` est aussi accepte. Une methode inconnue renvoie l'erreur `-32601`, un JSON invalide `-32700`.

### 14 outils MCP exposes

| Tool | Description |
|------|-------------|
| `vox_speak` | Synthetise et joue du texte (params: text, voice, lang, backend, style, gender, rate, volume) |
| `vox_list_voices` | Liste les voix disponibles pour un backend (param: backend) |
| `vox_clone_list` | Liste les voice clones enregistres |
| `vox_clone_add` | Ajoute un voice clone (params: name, audio, text) |
| `vox_clone_remove` | Supprime un voice clone (param: name) |
| `vox_config_show` | Affiche les preferences courantes et la ligne `acceleration:` |
| `vox_config_set` | Modifie une preference (params: key, value) |
| `vox_stats` | Statistiques d'utilisation |
| `vox_pack_list` | Liste les sound packs installes et quelques noms a installer |
| `vox_pack_install` | Installe un sound pack (param: name) |
| `vox_pack_set` | Active un sound pack (param: name) |
| `vox_pack_play` | Joue un son d'un pack (params: category, pack) |
| `vox_pack_remove` | Supprime un sound pack (param: name) |
| `vox_hear` | Enregistre et transcrit avec Whisper en local, ou transcrit un WAV (params: lang, timeout, silence, model, file) |

## Compilation conditionnelle

```rust
#[cfg(target_os = "macos")]   // backend say, module chat, sous-commande vox chat
#[cfg(feature = "kokoro")]    // backend kokoro
```

Le reste, STT compris, est compile sur toutes les plateformes.

Feature flags Cargo (aucune par defaut) :
- `metal` — GPU Apple Silicon (Metal + Accelerate) pour qwen-native et Whisper (STT)
- `cuda` — GPU NVIDIA (CUDA 12) pour qwen-native et Whisper (STT)
- `kokoro` — ajoute le backend `kokoro` (dependance optionnelle `kokoro-tts`)
- Sans `metal` ni `cuda` : CPU. `pocket` et `piper` tournent sur le CPU dans tous les builds.

A l'execution, Whisper (`stt/whisper.rs::best_device`) et qwen-native (`qwen3_tts::auto_device`) essaient CUDA dans un build `cuda`, puis Metal dans un build `metal`, puis retombent sur le CPU si le peripherique ne peut pas etre cree.

Le build CUDA lie les bibliotheques CUDA dynamiquement (feature `dynamic-linking` de cudarc, activee par candle-core ; candle-kernels demande aussi `cudart` a l'edition de liens). Sans le pilote NVIDIA (`libcuda.so.1`) et les bibliotheques runtime CUDA 12 cuBLAS (`libcublas.so.12`, `libcublasLt.so.12`) et cuRAND (`libcurand.so.10`), le binaire ne demarre pas. Cette liste est celle que donne `ldd` sur un build CUDA reel ; elle vient de la section d'installation du README.

`accel.rs` expose le build courant : `vox config show` se termine par une ligne `acceleration:` (Metal, CUDA ou CPU). Cette ligne dit avec quelle feature le binaire a ete compile, pas quel peripherique a ete cree a l'execution. Pour qwen-native, le peripherique reellement cree est ecrit sur stderr au chargement du modele (`Using device: ...`).

## CI/CD

- GitHub Actions (CI) : macOS (Metal), Linux (CPU), Linux (CUDA, compilation et clippy seulement : pas de GPU sur les runners), Windows (CPU)
- CI de l'installeur, sur Linux et macOS : `shellcheck -s sh install.sh` puis `sh tests/install_test.sh`
- release-please : `feat:` → version bump PR → merge → release + binaires
- Binaires : aarch64-apple-darwin (Metal), x86_64-unknown-linux-gnu (CPU, + .deb/.rpm), aarch64-unknown-linux-gnu (CPU, + .deb/.rpm), x86_64-pc-windows-msvc (CPU, .zip), et x86_64-unknown-linux-gnu avec la feature `cuda` (asset `vox-x86_64-unknown-linux-gnu-cuda.tar.gz`, CUDA 12.6, `CUDA_COMPUTE_CAP=80`)
- Le job CUDA de la release est `continue-on-error` : une release peut sortir sans le binaire CUDA (aucune release jusqu'a v0.16.0 ne le contient)
- Pas de binaire Mac Intel (x86_64-apple-darwin) : `ort` (via piper-rs) n'a pas de binaire precompile pour cette plateforme ; derniere release qui en contient un : v0.10.0
- Pas de binaire Windows CUDA (job retire des workflows dans 578778d)
- Tests : `cargo test` (unitaires + integration UX/security/perf). Les tests du plugin sont dans `plugins/vox/tests/vox.test.ts`.
