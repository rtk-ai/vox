# Documentation fonctionnelle

## Synthese vocale (TTS)

Fonctionnalite principale : transformer du texte en parole.

```bash
vox "Bonjour le monde"              # Backend par defaut
vox -b kokoro "Hello world"         # Backend specifique
vox -b qwen-native -l fr "Salut"   # Backend + langue
echo "Texte pipe" | vox             # Lecture depuis stdin
```

### Options de parole

| Flag | Description | Exemple |
|------|-------------|---------|
| `-b` | Backend TTS | `-b kokoro`, `-b say`, `-b qwen-native` |
| `-v` | Voix ou clone | `-v Chelsie`, `-v patrick` |
| `-l` | Langue | `-l fr`, `-l ja`, `-l en` |
| `-r` | Debit (mots/min, backend say) | `-r 200` |
| `--gender` | Genre vocal | `--gender feminine` |
| `--style` | Intonation | `--style warm`, `--style energetic` |
| `-m` | Modele TTS | `-m Qwen/Qwen3-TTS-12Hz-1.7B-Base` |

### Langues supportees

en, fr, es, de, it, pt, zh, ja, ko, ru, ar, nl

### Styles d'intonation

calm, energetic, warm, authoritative, cheerful, serious

## Voice cloning

Cloner une voix a partir d'un fichier audio de reference.

```bash
# Ajouter un clone depuis un fichier
vox clone add patrick --audio ~/voice.wav --text "Transcription du fichier"

# Enregistrer directement depuis le micro (capture cpal, aucun outil externe)
vox clone record myvoice --duration 10 --text "Ce que je dis pendant l'enregistrement"

# Utiliser un clone
vox -v patrick "Ceci parle avec ma voix"

# Gerer les clones
vox clone list
vox clone remove patrick
```

Formats audio acceptes : wav, mp3, flac, ogg, m4a.

Le voice cloning bascule automatiquement sur le backend `qwen` (macOS) ou `qwen-native` (autres) car `say` et `kokoro` ne supportent pas le cloning.

## Configuration

Preferences persistantes en base SQLite.

```bash
vox config show                    # Afficher les preferences
vox config set backend kokoro      # Changer le backend par defaut
vox config set lang fr             # Langue par defaut
vox config set voice Chelsie       # Voix par defaut
vox config set gender feminine     # Genre vocal
vox config set style warm          # Style d'intonation
vox config set rate 180            # Debit (say uniquement)
vox config set model <model_id>    # Modele TTS specifique
vox config set pack peon           # Sound pack actif
vox config reset                   # Reinitialiser tout
```

Priorite de resolution : **flags CLI / params MCP > preferences DB > valeurs par defaut**.

### Reference des cles de preferences

| Cle | Valeurs acceptees | Validation |
|-----|-------------------|-----------|
| `backend` | macOS: `kokoro`, `say`, `qwen`, `qwen-native` / Autres: `kokoro`, `qwen-native` | Whitelist par plateforme |
| `voice` | Nom de voix ou de clone (texte libre) | Aucune (le backend valide au moment du speak) |
| `lang` | `en`, `fr`, `es`, `de`, `it`, `pt`, `zh`, `ja`, `ko`, `ru`, `ar`, `nl` | Validation contre `SUPPORTED_LANGS` |
| `rate` | Entier positif (mots/min, ex: `150`, `200`) | Parse en `u32`, erreur si non-numerique |
| `gender` | `feminine`, `masculine` | Parse via `Gender::parse()`, erreur sinon |
| `style` | `calm`, `energetic`, `warm`, `authoritative`, `cheerful`, `serious` | Parse via `IntonationStyle::parse()`, erreur sinon |
| `model` | ID de modele HuggingFace (texte libre, ex: `Qwen/Qwen3-TTS-12Hz-1.7B-Base`) | Aucune (le backend valide au chargement) |
| `pack` | Nom de pack installe (texte libre) | Aucune (verifie a l'utilisation) |

### Matrice des capacites par backend

| Capacite | `say` | `piper` | `pocket` | `kokoro` | `qwen-native` |
|----------|-------|---------|----------|----------|----------------|
| Voice cloning | Non | Non | Oui (HF_TOKEN) | Non | Oui |
| Rate (debit) | Oui (`-r`) | Non | Non | Non | Non |
| Gender hint | Non | Non | Non | Non | Non (accepte, sans effet) |
| Style hint | Non | Non | Non | Non | Non (accepte, sans effet) |
| Choix de voix | Oui (voix Apple) | Par langue | 8 voix predefinies | Oui (prefixe `xx_nom`) | Non (clones uniquement) |
| Choix de modele | Non | Non | Non | Non | Oui |
| Langues | Toutes (via voix) | 50+ (voix rhasspy) | en (checkpoint embarque) | en, fr, ja, zh, ko, hi, it, pt, de, es | zh, en, ja, ko, de, fr, ru, pt, es, it |
| Plateforme | macOS | Toutes | Toutes | macOS (opt-in) | Toutes |
| GPU | Non | Non | Non (CPU par conception) | Non | Metal (macOS) / CUDA (Linux) |
| Dependance externe | Aucune | Aucune (Rust pur) | Aucune (Rust pur) | Aucune (Rust pur) | Aucune (Rust pur) |

## Sound packs

Packs de sons thematiques (compatible peon-ping). Sons courts joues pour signaler des evenements.

```bash
vox pack list                      # Voir packs installes + disponibles
vox pack install peon              # Installer un pack
vox pack set peon                  # Activer un pack
vox pack play greeting             # Jouer un son de la categorie "greeting"
vox pack play error -p peon_fr     # Jouer depuis un pack specifique
vox pack remove peon               # Desinstaller un pack
```

Categories de sons : greeting, acknowledge, complete, error, permission, resource_limit, annoyed.

## Statistiques d'utilisation

Dashboard complet de l'historique TTS.

```bash
vox stats
```

Affiche :
- Temps de parole total (format humain : h/m/s)
- Nombre total d'appels et de caracteres
- Latence moyenne et throughput (chars/s)
- Repartition par backend (calls, chars, duree, moyenne)
- Repartition par langue (avec barres visuelles)
- 10 derniers appels avec details

## Integration IA (`vox init`)

Configuration automatique pour 14 outils IA en une commande.

```bash
vox init                # Mode MCP (defaut) — configure tous les outils
vox init -m cli         # Mode CLI — CLAUDE.md + Stop hook
vox init -m skill       # Mode Skill — commande /speak
vox init -m all         # Les trois modes
```

### Outils supportes (mode MCP)

| Outil | Config |
|-------|--------|
| Claude Code | `~/.claude.json` |
| Claude Desktop | Config specifique OS |
| Cursor | `~/.cursor/mcp.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |
| VS Code / Copilot | `Code/User/mcp.json` |
| Zed | `~/.config/zed/settings.json` |
| Codex | `~/.codex/config.toml` |
| OpenCode | `~/.config/opencode/opencode.json` |
| Gemini Code Assist | `~/.gemini/settings.json` |
| Amazon Q | `~/.aws/amazonq/mcp.json` |
| Cline | Extension VS Code globalStorage |
| Roo Code | Extension VS Code globalStorage |
| Kilo Code | Extension VS Code globalStorage |
| Amp | `~/.ampcode/settings.json` |

L'init est idempotent : relancer `vox init` ne duplique pas les configurations.

### Comparaison des modes d'init

| Mode | Ce qu'il fait | Quand l'utiliser |
|------|--------------|-----------------|
| `mcp` (defaut) | Configure le serveur MCP dans les fichiers de config de 14 outils IA | L'assistant appelle `vox_speak`, `vox_hear`, etc. nativement via le protocole MCP. Meilleure integration. |
| `cli` | Cree `CLAUDE.md` + hook `Stop` dans `.claude/settings.json` | L'assistant appelle `vox` via bash. Plus simple mais moins de fonctionnalites (pas de STT, stats, etc.). |
| `skill` | Cree `/speak` dans `~/.claude/commands/speak.md` | L'utilisateur invoque manuellement `/speak <texte>` dans Claude Code. |
| `all` | Les trois modes combines | Maximum de compatibilite. |

### Mode CLI

Cree un `CLAUDE.md` dans le projet courant avec des instructions pour que l'assistant appelle `vox` apres les taches significatives. Ajoute un hook `Stop` dans `.claude/settings.json` qui dit "Termine" a la fin de chaque reponse.

### Mode Skill

Cree une commande `/speak` dans `~/.claude/commands/speak.md` pour invoquer vox via slash command.

## Serveur MCP (`vox serve`)

Lance le serveur MCP sur stdio (JSON-RPC 2.0, protocole `2024-11-05`). C'est cette commande que les outils IA appellent apres `vox init`.

```bash
vox serve    # Lance le serveur (bloque, lit stdin, ecrit stdout)
```

Le serveur est normalement lance automatiquement par l'outil IA. Il n'est pas necessaire de le lancer manuellement, sauf pour du debug.

### Reference complete des 14 outils MCP

| Outil | Description | Parametres |
|-------|-------------|------------|
| `vox_speak` | Synthetise et joue du texte | **`text`** (requis, string) : texte a prononcer. `voice` (string) : nom de voix ou clone. `lang` (string) : code langue. `backend` (string) : kokoro/say/qwen/qwen-native. `style` (string) : calm/energetic/warm/authoritative/cheerful/serious. `gender` (string) : feminine/masculine. `rate` (integer) : debit mots/min (say uniquement). |
| `vox_list_voices` | Liste les voix d'un backend | `backend` (string) : kokoro/say/qwen/qwen-native. Defaut : backend par defaut de la plateforme. |
| `vox_clone_list` | Liste les voice clones | Aucun parametre. |
| `vox_clone_add` | Ajoute un voice clone | **`name`** (requis, string) : nom du clone. **`audio`** (requis, string) : chemin du fichier audio de reference. `text` (string) : transcription de l'audio (ameliore la qualite). |
| `vox_clone_remove` | Supprime un voice clone | **`name`** (requis, string) : nom du clone a supprimer. |
| `vox_config_show` | Affiche les preferences | Aucun parametre. Retourne : backend, voice, lang, rate, gender, style, model, pack. |
| `vox_config_set` | Modifie une preference | **`key`** (requis, string) : cle (backend/voice/lang/rate/gender/style/model). **`value`** (requis, string) : valeur. |
| `vox_stats` | Statistiques d'utilisation | Aucun parametre. Retourne : total requests, total chars, 10 dernieres entrees. |
| `vox_pack_list` | Liste les sound packs | Aucun parametre. Retourne : packs installes (avec actif marque) + disponibles. |
| `vox_pack_install` | Installe un sound pack | **`name`** (requis, string) : nom du pack (peon, peon_fr, peon_pl, peasant, peasant_fr, sc_kerrigan, sc_battlecruiser, ra2_soviet_engineer). |
| `vox_pack_set` | Active un sound pack | **`name`** (requis, string) : nom du pack installe. |
| `vox_pack_play` | Joue un son d'un pack | `category` (string, defaut: "greeting") : greeting/acknowledge/complete/error/permission/resource_limit/annoyed. `pack` (string) : nom du pack (utilise le pack actif si omis). |
| `vox_pack_remove` | Supprime un sound pack | **`name`** (requis, string) : nom du pack. Si le pack supprime etait actif, le pack actif est remis a vide. |
| `vox_hear` | Enregistre et transcrit (STT) | `lang` (string, defaut: auto-detection) : code langue Whisper. `timeout` (integer, defaut: 30) : duree max en secondes. `silence` (number, defaut: 2.0) : secondes de silence avant arret. `model` (string) : repo Whisper (defaut `openai/whisper-small`). `file` (string) : fichier WAV a transcrire au lieu du micro. Toutes plateformes. |

Les parametres en **gras** sont requis. Le serveur renvoie `isError: true` si un parametre requis est manquant ou invalide.

## Speech-to-Text (toutes plateformes)

Transcription locale via Whisper sur candle (Rust pur, 99 langues, Metal/CUDA si compile avec la feature).
Le modele est telecharge depuis Hugging Face au premier usage.

```bash
# CLI
vox hear                                   # Ecoute, detecte la langue, transcrit
vox hear -l fr -t 60 -s 3.0                # Francais force, timeout 60s, silence 3s
vox hear -m openai/whisper-large-v3-turbo  # Meilleure qualite (GPU + 16 Go RAM conseilles)
vox hear -f enregistrement.wav             # Transcrire un fichier au lieu du micro

# Via MCP
vox_hear                           # Utilise par l'assistant IA
```

Variables : `VOX_STT_MODEL` (repo Whisper, defaut `openai/whisper-small` ~1 Go RAM), `VOX_VAD_THRESHOLD` (seuil RMS minimal du detecteur de silence, 0-1, defaut 0.0125 ; le seuil effectif s'adapte au bruit ambiant mesure sur les 300 premieres ms), `VOX_VAD_DEBUG=1` (affiche les niveaux RMS et le seuil retenu).
Aucun prerequis externe : la capture micro utilise cpal.

## Mode conversation (macOS)

Boucle vocale complete : ecouter → reflechir → parler.

```bash
export ANTHROPIC_API_KEY=sk-...
vox chat                           # Conversation avec Claude
vox chat -v patrick -l fr          # Avec voice clone en francais
```

Utilise Claude API en streaming pour la reflexion, STT local pour l'ecoute, et TTS pour la reponse.

## Lecture de voix

Lister les voix disponibles pour un backend.

```bash
vox --list-voices                  # Backend par defaut
vox -b say --list-voices           # Voix macOS
vox -b kokoro --list-voices        # Voix Kokoro
```

## Donnees locales

Tout est stocke localement, aucune donnee n'est envoyee a un serveur externe (sauf le mode chat qui utilise l'API Claude).

```
~/.config/vox/
  vox.db          # SQLite : preferences, clones, logs
  clones/         # Fichiers audio des voice clones
  packs/          # Sound packs installes
```

Variables d'environnement :
- `VOX_CONFIG_DIR` — repertoire de configuration alternatif
- `VOX_DB_PATH` — chemin de base de donnees alternatif
