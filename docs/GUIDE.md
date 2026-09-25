# Guide utilisateur

## Installation

### Script rapide (macOS / Linux / WSL)

```bash
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh
```

### Depuis les sources

```bash
# Standard (CPU)
cargo install --path .

# macOS avec GPU Metal (recommande sur Apple Silicon)
cargo install --path . --features metal

# Linux avec GPU NVIDIA
cargo install --path . --features cuda
```

### Prerequis optionnels

| Composant | Pour quoi | Installation |
|-----------|----------|--------------|
| `mlx-audio` | Backend qwen (macOS) | `uv pip install mlx-audio` |

## Demarrage rapide

```bash
# Parler
vox "Hello, world!"

# En francais
vox -l fr "Bonjour le monde"

# Pipe depuis une commande
echo "Message important" | vox

# Voir les voix
vox --list-voices
```

## Configuration avec un assistant IA

La methode recommandee est d'utiliser vox comme serveur MCP :

```bash
vox init
```

Cette commande configure automatiquement **tous les outils IA** presents sur votre machine (Claude Code, Cursor, VS Code, Zed, etc.). Redemarrez votre outil apres l'init.

L'assistant IA pourra alors :
- Vous parler apres avoir termine une tache
- Lister et changer les voix
- Gerer vos voice clones
- Jouer des sons de packs
- Ecouter votre voix (STT, macOS)

### Exemple avec Claude Code

Apres `vox init`, Claude Code peut utiliser les outils MCP directement :

```
> Corrige le bug dans auth.rs

[Claude corrige le bug, puis parle :]
"Le bug d'authentification a ete corrige. Le token etait expire
 car la duree etait en secondes au lieu de millisecondes."
```

## Personnaliser la voix

```bash
# Changer le backend par defaut
vox config set backend kokoro

# Voix feminine, style chaleureux
vox config set gender feminine
vox config set style warm

# Langue par defaut
vox config set lang fr

# Voir la config actuelle
vox config show
```

## Cloner votre voix

Creez un clone vocal a partir d'un enregistrement audio :

```bash
# Depuis un fichier existant
vox clone add mavoix --audio ~/enregistrement.wav --text "Transcription exacte"

# Enregistrer depuis le micro (capture cpal integree)
vox clone record mavoix --duration 10 --text "Ce que je dis"

# Utiliser le clone
vox -v mavoix "Ceci parle avec ma voix clonee"
```

Pour de meilleurs resultats :
- Enregistrement de 5-15 secondes
- Environnement calme, sans bruit de fond
- Parler naturellement, pas trop vite
- Fournir la transcription exacte avec `--text`

## Sound packs

Ajoutez des sons thematiques (style Warcraft peon, StarCraft, etc.) :

```bash
vox pack install peon          # Installer
vox pack set peon              # Activer
vox pack play greeting         # "Ready to work!"
vox pack play complete         # "Work complete."
vox pack play error            # "Can't do that."
```

Voir les packs disponibles : `vox pack list`

## Conversation vocale (macOS)

Discutez vocalement avec Claude :

```bash
export ANTHROPIC_API_KEY=sk-ant-...
vox chat -l fr
```

La boucle : vous parlez → Whisper transcrit → Claude repond → vox parle la reponse.

## Transcription (macOS)

Enregistrez et transcrivez votre voix :

```bash
vox hear -l fr
# Parlez... (s'arrete apres 2s de silence)
# => "Votre texte transcrit ici"
```

## Backends en detail

### say (macOS natif)
- Latence quasi-nulle (~100ms)
- Voix Apple integrees (Samantha, Thomas, etc.)
- Support du debit (`-r 200`)
- Pas de voice cloning
- Zero dependance, zero configuration

### kokoro (Rust pur)
- Cross-platform, necessite `kokoro-onnx` et `soundfile` (Python)
- Bonne qualite vocale, voix pre-definies avec prefixe langue (`af_`, `ff_`, `jf_`, etc.)
- ~2-5s warm / ~5-8s cold start
- Pas de voice cloning
- Modele ONNX ~80 MB dans `~/.config/vox/kokoro/`

### qwen (MLX Python, macOS)
- Qualite neurale superieure
- Voice cloning supporte (via `ref_audio` + `ref_text`)
- Necessite `mlx-audio` + Apple Silicon
- ~1-2s warm / ~5-15s cold start
- Pipeline de chunking pour les textes longs (overlap generation/playback)

### qwen-native (Rust pur)
- Meme modele Qwen3-TTS mais en Rust (candle)
- Voice cloning supporte
- GPU via Metal (macOS) ou CUDA (Linux), feature flags `metal`/`cuda`
- Cross-platform, zero dependance Python
- ~2-5s warm / ~10-30s cold start
- Le modele est garde en memoire via `Mutex` global — ideal pour le serveur MCP

## Configuration avancee

### Variables d'environnement

| Variable | Description | Defaut |
|----------|-------------|--------|
| `VOX_CONFIG_DIR` | Repertoire de configuration alternatif | `~/.config/vox/` |
| `VOX_DB_PATH` | Chemin de base de donnees alternatif | `~/.config/vox/vox.db` |
| `ANTHROPIC_API_KEY` | Cle API Claude (requis pour `vox chat`) | Aucun |

### Modeles personnalises

Pour les backends `qwen` et `qwen-native`, il est possible d'utiliser un modele different :

```bash
# Via CLI
vox -b qwen-native -m "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit" "Texte"

# Via preference persistante
vox config set model "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit"
```

Les modeles sont telecharges automatiquement depuis HuggingFace Hub au premier appel. Le modele par defaut de `qwen-native` est `Qwen/Qwen3-TTS-12Hz-0.6B-Base`. Le modele par defaut de `qwen` (MLX) est `mlx-community/Qwen3-TTS-12Hz-0.6B-Base-bf16`.

### Arborescence des donnees locales

```
~/.config/vox/
  vox.db              # SQLite : preferences, clones, usage logs
  clones/             # Fichiers audio des voice clones (.wav)
  packs/              # Sound packs installes
    peon/
      manifest.json
      sounds/
  kokoro/             # Modele Kokoro (si utilise)
    kokoro-v1.0.onnx
    voices-v1.0.bin
```

## Verification de l'integration MCP

Apres `vox init`, pour verifier que tout fonctionne :

```bash
# 1. Verifier que le binaire est dans le PATH
which vox

# 2. Tester le serveur MCP manuellement (Ctrl+C pour arreter)
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | vox serve

# 3. Verifier la configuration de Claude Code
cat ~/.claude.json | grep vox

# 4. Tester un appel complet
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | vox serve
```

Le serveur doit repondre avec la liste des 14 outils. Si l'outil IA ne detecte pas vox apres l'init, redemarrez-le.

## Depannage

### "command not found: vox"
Le binaire n'est pas dans votre PATH. Verifiez avec `which vox` ou reinstallez.

### "Backend 'say' is only available on macOS"
Utilisez `kokoro` ou `qwen-native` sur Linux/Windows : `vox config set backend kokoro`

### Backend lent (cold start vs warm start)

Tous les backends neuraux (kokoro, qwen, qwen-native) ont un temps de demarrage a froid (cold start) significant :

| Backend | Cold start | Warm start | Comment accelerer |
|---------|-----------|------------|-------------------|
| `say` | ~100ms | ~100ms | Rien a faire, toujours rapide |
| `kokoro` | ~5-8s | ~2-5s | Pas de cache persistent — chaque appel CLI recharge le modele |
| `qwen` | ~5-15s | ~1-2s | Le premier appel telecharge le modele (~1.2 GB). Ensuite, seul le startup Python est lent |
| `qwen-native` | ~10-30s | ~2-5s | Le modele est garde en memoire dans le processus MCP. Utiliser via MCP (pas CLI) pour beneficier du warm start |

**Conseil** : pour la latence la plus basse avec un backend neural, utilisez `vox serve` (via MCP) plutot que des appels CLI individuels. Le backend `qwen-native` garde son modele en memoire dans un `Mutex` global, donc les appels suivants au meme processus sont instantanes.

### Qualite du voice cloning

Pour de meilleurs resultats avec le voice cloning :

- **Format** : WAV 16-bit, mono, 16-48 kHz (le WAV est recommande pour eviter les artefacts de compression)
- **Duree** : 5-15 secondes de parole continue
- **Contenu** : parler naturellement, pas trop vite, avec des phrases completes
- **Environnement** : calme, sans bruit de fond, sans echo
- **Transcription** : toujours fournir `--text` avec la transcription exacte — cela ameliore significativement la qualite
- Formats acceptes : wav, mp3, flac, ogg, m4a (mais WAV recommande)

### Corruption de la base de donnees

Si la base SQLite est corrompue (`database disk image is malformed`) :

```bash
# Methode 1 : reinitialiser (perd les preferences et logs, garde les clones audio)
rm ~/.config/vox/vox.db
# La base sera recree automatiquement au prochain appel

# Methode 2 : utiliser une base temporaire pour depannage
VOX_DB_PATH=/tmp/vox_test.db vox config show

# Methode 3 : tenter une reparation SQLite
sqlite3 ~/.config/vox/vox.db ".recover" | sqlite3 ~/.config/vox/vox_recovered.db
mv ~/.config/vox/vox_recovered.db ~/.config/vox/vox.db
```

### Enregistrement micro ne fonctionne pas
La capture utilise cpal (aucun outil externe). Sur macOS, autorisez le micro pour votre terminal
(Reglages > Confidentialite et securite > Microphone). Sur Linux, verifiez que ALSA/PulseAudio voit
le peripherique (`arecord -l`). Si le detecteur de silence coupe trop tot ou trop tard, ajustez
`VOX_VAD_THRESHOLD` (defaut 0.0125).

### "ANTHROPIC_API_KEY is required" (chat mode)
Exportez votre cle API : `export ANTHROPIC_API_KEY=sk-ant-...`

## Choix du backend : guide de performance

| Usage | Backend recommande | Pourquoi |
|-------|-------------------|----------|
| Feedback rapide (1-2 phrases) | `say` (macOS) | Latence ~100ms, zero configuration |
| Cross-platform, zero config | `kokoro` | Marche partout, bonne qualite, pas de GPU |
| Qualite maximale | `qwen` ou `qwen-native` | Voix neurale, prosodie naturelle |
| Voice cloning | `qwen` (macOS) ou `qwen-native` | Seuls backends supportant le cloning |
| Serveur MCP longue duree | `qwen-native` | Modele reste en memoire, warm start ~2-5s |
| Texte long (>500 chars) | `qwen` | Pipeline de chunking avec overlap generation/playback |
| Machine sans GPU | `kokoro` ou `say` | Pas de GPU requis, latence acceptable |
| Machine avec GPU (Metal/CUDA) | `qwen-native` | Acceleration materielle via feature flags `metal`/`cuda` |
