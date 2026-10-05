# Documentation fonctionnelle

vox est un seul binaire Rust : il n'y a pas de Python, ni a la compilation ni a
l'execution. Cette page decrit chaque commande. L'installation et le choix du
build sont dans le [README](../README_fr.md#installation).

## Synthese vocale (TTS)

Fonctionnalite principale : transformer du texte en parole.

```bash
vox "Hello world"                             # Backend par defaut : pocket (anglais)
vox -l fr "Bonjour le monde"                  # Autre langue que l'anglais : piper
vox -b say "Hello world"                      # Backend explicite (say : macOS uniquement)
vox -b qwen-native -l fr -v patrick "Salut"   # Voix clonee
echo "Texte pipe" | vox -l fr                 # Lecture depuis stdin
```

Backends disponibles :

- `pocket` : Kyutai pocket-tts, sur le CPU. Backend par defaut pour l'anglais et
  quand aucune langue n'est donnee. 8 voix anglaises predefinies.
- `piper` : ONNX, sur le CPU. Backend par defaut pour toutes les autres langues.
  Une voix par defaut par langue ; `-v` en choisit une autre par son nom.
- `qwen-native` : Qwen3-TTS sur candle, clonage de voix. Utilise le GPU dans un
  build Metal ou CUDA.
- `kokoro` : present seulement dans un build compile avec `--features kokoro`.
  Un build de release standard repond `Unknown backend: kokoro`.
- `say` : la commande `/usr/bin/say`, macOS uniquement.

Ces valeurs par defaut sont les memes sur toutes les plateformes. Les modeles
de `pocket`, `piper` et `qwen-native` sont telecharges au premier usage.

`piper` et `pocket` envoient les echantillons directement au peripherique audio,
ouvert en parallele du chargement du modele ; `pocket` joue pendant qu'il genere
encore. `qwen-native` et `kokoro` rendent d'abord un WAV complet, puis le
jouent.

### Options de parole

| Flag | Description | Exemple |
|------|-------------|---------|
| `-b`, `--backend` | Backend TTS | `-b piper`, `-b say`, `-b qwen-native` |
| `-v`, `--voice` | Voix ou clone | `-v alba`, `-v patrick` |
| `-l`, `--lang` | Langue. Sans `-b` ni preference `backend`, elle choisit aussi le backend : `en` ou rien donne `pocket`, toute autre langue donne `piper` | `-l fr`, `-l de`, `-l en` |
| `-r`, `--rate` | Debit en mots/min, backend `say` uniquement | `-r 200` |
| `--gender` | Genre vocal. Accepte, mais aucun backend actuel ne l'utilise | `--gender feminine` |
| `--style` | Intonation. Accepte, mais aucun backend actuel ne l'utilise | `--style warm` |
| `-m`, `--model` | Modele du backend `qwen-native` (depot Hugging Face) | `-m Qwen/Qwen3-TTS-12Hz-1.7B-Base` |
| `--volume` | Multiplicateur de volume, de 0.0 a 5.0 (defaut 1.0) | `--volume 2.0` |
| `-o`, `--output` | Ecrire un WAV au lieu de parler | `-o note.wav` |
| `--list-voices` | Lister les voix du backend selectionne | `-b say --list-voices` |

### Enregistrer dans un fichier

```bash
vox -o note.wav "Texte a enregistrer"
vox -b qwen-native -l fr -v mavoix -o note.wav "Avec un clone"
ffmpeg -i note.wav -c:a libopus -b:a 32k note.ogg   # vocal WhatsApp, par exemple
```

`-o fichier.wav` ecrit un WAV au lieu de jouer le son, avec tous les backends et
sur toutes les plateformes. Les repertoires parents manquants sont crees.

Le drapeau est reserve au CLI : il n'est volontairement pas expose en MCP, ou
un chemin fourni par l'agent serait une ecriture de fichier arbitraire. Un
appel avec `-o` ne passe pas par le daemon, dont le repertoire de travail n'est
pas le votre.

### Langues supportees

en, fr, es, de, it, pt, zh, ja, ko, ru, ar, nl

Ce sont les langues acceptees par `vox config set lang` et `vox init --lang`.
`piper` a une voix par defaut pour chacune, sauf `ja` : il n'a pas de voix
japonaise, et le japonais utilise `qwen-native` par defaut (`-b piper -l ja`
s'arrete aussitot avec un message qui le dit). La valeur de `-l` n'est pas verifiee sur la ligne de commande :
avec `piper`, un code inconnu donne la voix anglaise ; avec `qwen-native`, `ar`
et `nl` sont refuses, et sans clone `-l` n'a pas d'autre effet (le modele
deduit la langue du texte) ; `pocket` ne parle que l'anglais et `say` ignore
`-l`.

### Styles d'intonation

calm, energetic, warm, authoritative, cheerful, serious

Ce sont les valeurs acceptees par `vox config set style`. Aucun backend actuel
n'utilise le style ni le genre.

## Voice cloning

Cloner une voix a partir d'un fichier audio de reference.

```bash
# Ajouter un clone depuis un fichier
vox clone add patrick --audio ~/voice.wav --text "Transcription du fichier"

# Enregistrer directement depuis le micro (capture cpal, aucun outil externe)
vox clone record myvoice --duration 10 --text "Ce que je dis pendant l'enregistrement"

# Utiliser un clone
vox -l fr -v patrick "Ceci parle avec ma voix"

# Gerer les clones
vox clone list
vox clone remove patrick
```

`vox clone add` accepte les fichiers wav, mp3, flac et ogg ; un `.m4a` est
refuse. Le fichier est converti en WAV et garde dans `clones/<nom>.wav`, dans
le repertoire de configuration : le clone ne depend plus du fichier d'origine.
`clone record` ecrit `clones/<nom>.wav` au meme endroit ; `--duration` vaut 10
secondes par defaut. Le nom d'un clone est un nom simple, pas un chemin, et un
nom deja pris est refuse, quelle que soit la casse.

Deux backends clonent une voix :

- `qwen-native`, sans prerequis. Langues : en, fr, es, de, it, pt, zh, ja, ko,
  ru (`-l`, anglais par defaut).
- `pocket`, qui demande `HF_TOKEN` et l'acces au checkpoint a acces restreint
  `kyutai/pocket-tts` sur Hugging Face. Sans jeton, vox s'arrete avec un
  message qui le dit.

Quand `-v` designe un clone, un backend donne avec `-b` est respecte ; s'il ne
sait pas cloner (`piper` ou `say`, par exemple), vox l'indique et le clone est
ignore. Sans `-b`, vox part du backend prevu (la preference `backend`, sinon le
defaut selon la langue) : il le garde si c'est `qwen-native`, ou si c'est
`pocket` et que `HF_TOKEN` est defini, et utilise `qwen-native` dans tous les
autres cas. La regle est la meme via MCP (`vox_speak`), avec le parametre
`backend` a la place de `-b`.

Pendant `vox clone record` et `vox hear`, deux bips encadrent la prise : un bip
aigu (880 Hz) juste avant l'ouverture du micro, un bip plus grave (587 Hz) a
l'arret. Le premier est joue avant l'ouverture du micro et `clone record` joue
le second apres avoir copie l'enregistrement : ils ne se retrouvent ni dans le
clone ni dans le bruit de fond mesure. Avec `vox hear`, le second bip est joue
micro encore ouvert : il reste hors de l'audio transcrit quand la prise
s'arrete sur un silence, pas quand elle s'arrete sur la duree maximale (`-t`)
alors que vous parlez encore. `VOX_CUES=0` les desactive.

Gardez la reference courte : le temps de synthese depend beaucoup de sa duree.
Mesure sur Apple M2 avec `Qwen3-TTS-12Hz-0.6B-Base`, meme phrase generee :
reference de 6 s a 24 kHz, 21 s ; reference de 14 s a 48 kHz, 11 minutes.
Cinq a huit secondes de parole nette a 24 kHz est le bon compromis.

## Configuration

Preferences persistantes en base SQLite.

```bash
vox config show                    # Afficher les preferences et le build (ligne acceleration: Metal, CUDA ou CPU)
vox config set backend piper       # Changer le backend par defaut
vox config set lang fr             # Langue par defaut
vox config set voice alba          # Voix par defaut
vox config set gender feminine     # Genre vocal (enregistre, sans effet)
vox config set style warm          # Style d'intonation (enregistre, sans effet)
vox config set rate 180            # Debit (say uniquement)
vox config set model <model_id>    # Modele du backend qwen-native
vox config set stt_model <repo>    # Modele Whisper pour la transcription
vox config set pack peon           # Sound pack actif
vox config reset                   # Reinitialiser tout
vox setup                          # Interface interactive : backend, voix, langue, style, test
vox bench                          # Chronometre une phrase de test avec chaque backend, rendue dans un fichier temporaire : rien n'est joue
vox bench --set                    # Idem, et enregistre le plus rapide comme backend par defaut
```

Priorite de resolution : **flags CLI / params MCP > preferences > valeurs par
defaut**. Pour le modele Whisper, l'ordre complet est : `--model` >
`VOX_STT_MODEL` > preference `stt_model` > `models.toml` > valeur compilee.
Pour le modele `qwen-native` : `--model` ou preference `model` > `models.toml`
> valeur compilee. Un fichier `models.toml` place dans le repertoire de
configuration remplace celui qui est compile dans le binaire.

Une preference `backend` s'applique a toutes les langues : le choix automatique
entre `pocket` et `piper` ne joue que sans preference `backend`.

### Reference des cles de preferences

| Cle | Valeurs acceptees | Validation |
|-----|-------------------|-----------|
| `backend` | `piper`, `pocket`, `qwen-native` ; `say` sur macOS ; `kokoro` dans un build `--features kokoro` | Liste des backends compiles dans le binaire |
| `voice` | Nom de voix ou de clone (texte libre) | Aucune (le backend valide au moment du speak) |
| `lang` | `en`, `fr`, `es`, `de`, `it`, `pt`, `zh`, `ja`, `ko`, `ru`, `ar`, `nl` | Validation contre `SUPPORTED_LANGS` |
| `rate` | Entier positif (mots/min, ex: `150`, `200`) | Parse en `u32`, erreur si non-numerique |
| `gender` | `feminine`, `masculine` | Parse via `Gender::parse()`, erreur sinon |
| `style` | `calm`, `energetic`, `warm`, `authoritative`, `cheerful`, `serious` | Parse via `IntonationStyle::parse()`, erreur sinon |
| `model` | ID de modele HuggingFace pour `qwen-native` (texte libre, ex: `Qwen/Qwen3-TTS-12Hz-0.6B-Base`, la valeur par defaut) | Aucune (le backend valide au chargement) |
| `stt_model` | Depot Whisper sur HuggingFace (texte libre, ex: `openai/whisper-small`) | Aucune (verifie au chargement) |
| `pack` | Nom de pack installe (texte libre) | Aucune (verifie a l'utilisation) |

### Matrice des capacites par backend

| Capacite | `say` | `piper` | `pocket` | `kokoro` | `qwen-native` |
|----------|-------|---------|----------|----------|----------------|
| Voice cloning | Non | Non | Oui (`HF_TOKEN` et checkpoint a acces restreint) | Non | Oui |
| Rate (debit) | Oui (`-r`) | Non | Non | Non | Non |
| Gender hint | Non | Non | Non | Non | Non (accepte, sans effet) |
| Style hint | Non | Non | Non | Non | Non (accepte, sans effet) |
| Volume (`--volume`) | Oui | Oui | Oui | Oui | Oui |
| Choix de voix | Oui (voix Apple) | Oui (une voix par defaut par langue ; `-v` avec un nom de voix piper, ex: `fr_FR-siwis-low`, en choisit une autre) | 8 voix predefinies, ou un fichier d'embedding `.safetensors` | Oui (prefixe `xx_nom`) | Non (clones uniquement) |
| Choix de modele | Non | Non | Non | Non | Oui |
| Langues | Selon la voix Apple choisie (`-l` ignore) | en, fr, es, de, it, pt, zh, ko, ru, ar, nl | en (checkpoint embarque) | en, fr, es, hi, it, ja, pt, zh | en, fr, es, de, it, pt, zh, ja, ko, ru |
| Plateforme | macOS | Toutes | Toutes | Build `--features kokoro` uniquement | Toutes |
| GPU | Non | Non (CPU) | Non (CPU par conception) | Non | Selon le build : Metal (Apple Silicon) ou CUDA (Linux x86_64, NVIDIA) ; CPU sinon |
| Passe par le daemon | Non | Oui | Oui | Oui | Oui |
| Spectre pour le visualiseur | Non (sauf avec un `--volume` autre que 1.0) | Oui | Oui | Oui | Oui |
| Dependance externe | `/usr/bin/say` (fourni par macOS) | Aucune | Aucune | Fichiers du modele a telecharger a la main (vox affiche les commandes) | Aucune |

Sous Linux, le binaire a besoin d'ALSA et d'OpenSSL 3 a l'execution : voir la
section Install du [README anglais](../README.md#install).

## Daemon (modeles gardes en memoire)

```bash
vox daemon start                      # Lancer le daemon en arriere-plan
vox daemon start --idle-timeout 900   # Arret apres 900 s sans requete (defaut : 300 ; 0 : jamais)
vox daemon status                     # Pid, port, duree de fonctionnement, chemin du journal, modeles charges
vox daemon stop                       # Arreter
```

Le daemon garde les modeles charges entre deux appels. Il n'est pas lance
automatiquement. Quand il tourne, les appels `vox "texte"` du CLI avec
`pocket`, `piper`, `qwen-native` ou `kokoro` passent par lui ; `say` et les
appels avec `-o` n'y passent pas. Le serveur MCP n'utilise pas le daemon : il
garde lui-meme ses modeles charges tant qu'il tourne.

Le daemon ecoute sur `127.0.0.1`, port 19876 (`VOX_DAEMON_PORT` pour en
changer). `vox daemon status` signale les modeles `pocket`, `piper` et
`qwen-native` : `kokoro` n'y apparait pas, meme charge.
`--idle-timeout 0` desactive l'arret automatique. La sortie du daemon est
ecrite dans `daemon.log`, dans le repertoire de configuration ; `vox daemon
status` en affiche le chemin.

Temps mesure entre le lancement et le premier son, pour une phrase de 3
secondes, sur un portable Apple A18 Pro (8 Go, mode economie d'energie), build
Metal de release : `pocket` (anglais) 0.18 a 0.55 s sans daemon ; `piper`
(francais) environ 0.75 s sans daemon et 0.26 a 0.32 s avec. Un texte anglais
long (15 s d'audio) commence a etre joue apres 0.81 s.

## Sound packs

Packs de sons thematiques (compatible peon-ping). Sons courts joues pour signaler des evenements.

```bash
vox pack list                      # Voir packs installes + quelques noms a installer
vox pack install peon              # Installer un pack
vox pack set peon                  # Activer un pack
vox pack play greeting             # Jouer un son de la categorie "greeting"
vox pack play error -p peon_fr     # Jouer depuis un pack specifique
vox pack remove peon               # Desinstaller un pack
```

Packs proposes par `vox pack list` : peon, peon_fr, peon_pl, peasant,
peasant_fr, sc_kerrigan, sc_battlecruiser, ra2_soviet_engineer. Tout autre pack
du registre peon-ping s'installe de la meme facon, par son nom (`vox pack
install glados`, par exemple) ; ils sont listes sur https://openpeon.com/packs.

Categories de sons : greeting, acknowledge, complete, error, permission, resource_limit, annoyed.
Les categories reellement disponibles sont celles du manifeste du pack.

`vox pack install` cherche le pack dans le registre peon-ping, telecharge son
manifeste `openpeon.json` (format CESP) et ses sons depuis le depot que le
registre indique, puis ecrit son propre `manifest.json`. Une installation
interrompue est reprise de zero par la suivante.

## Statistiques d'utilisation

Historique des appels TTS reussis (CLI et `vox_speak`).

```bash
vox stats
```

Affiche :
- Temps total des appels (synthese et lecture, format humain : h/m/s)
- Nombre total d'appels et de caracteres
- Duree moyenne par appel, longueur moyenne et throughput (chars/s)
- Repartition par backend (calls, chars, duree, moyenne)
- Repartition par langue (avec barres visuelles)
- 10 derniers appels avec details

## Integration IA (`vox init`)

Configuration automatique, en une commande, de ceux des 14 outils IA pris en
charge qui sont installes sur la machine.

```bash
vox init                # Mode MCP (defaut) — configure les outils installes
vox init -m cli         # Mode CLI — CLAUDE.md + Stop hook
vox init -m skill       # Mode Skill — commande /speak
vox init -m all         # Les trois modes
vox init -m cli -l fr   # Langue des instructions et du Stop hook
```

Sans `-l`, la langue est la preference `lang`, sinon la locale du systeme.
A la fin, `vox init` affiche les deux commandes qui installent le
[plugin Claude Code](#plugin-claude-code--visualiseur-de-voix).

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
Seuls les outils trouves sur la machine sont configures : un outil est reconnu
a son fichier de configuration ou a son repertoire de donnees. Les autres sont
affiches avec `not installed, skipped` et rien n'est cree pour eux.

### Comparaison des modes d'init

| Mode | Ce qu'il fait | Quand l'utiliser |
|------|--------------|-----------------|
| `mcp` (defaut) | Configure le serveur MCP dans les fichiers de config des outils IA installes, parmi les 14 pris en charge | L'assistant appelle `vox_speak`, `vox_hear`, etc. via le protocole MCP. |
| `cli` | Cree `CLAUDE.md` + hook `Stop` dans `.claude/settings.json` | L'assistant appelle `vox` via bash. Le bloc ecrit dans `CLAUDE.md` ne decrit que la synthese. |
| `skill` | Cree `/speak` dans `~/.claude/commands/speak.md` | L'utilisateur invoque manuellement `/speak <texte>` dans Claude Code. |
| `all` | Les trois modes combines | Pour avoir les trois a la fois. |

### Mode CLI

Cree un `CLAUDE.md` dans le projet courant, ou ajoute un bloc court a celui qui
existe, avec des instructions pour que l'assistant appelle `vox` apres les
taches significatives. Le bloc mentionne aussi le plugin Claude Code. Ajoute
un hook `Stop` dans `.claude/settings.json` qui prononce un mot de fin a la fin
de chaque reponse, dans la langue retenue (`Done.` en anglais ou sans langue).

### Mode Skill

Cree une commande `/speak` dans `~/.claude/commands/speak.md` pour invoquer vox via slash command.

## Plugin Claude Code : visualiseur de voix

Le depot contient un plugin Claude Code (`plugins/vox`) qui dessine, au-dessus
du prompt, le spectre du son que vox est en train de jouer. Il demande Claude
Code 2.1.287 ou plus recent. Dans une session Claude Code :

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
```

```text
/vox-wave                          # Apercu de l'animation, sans vox ni audio
/vox-wave hear 10                  # Apercu de l'animation d'ecoute pendant 10 s
/vox-wave color                    # Couleurs actuelles et presets
/vox-wave color ocean              # Preset : sunset, ocean, forest, fire, violet, rainbow, mono
/vox-wave color #00ff00 #0000ff    # Un a trois arrets de degrade, de gauche a droite
```

Les couleurs choisies sont conservees d'une session a l'autre.

Pendant qu'il joue, vox ecrit le spectre du son (20 bandes toutes les 50 ms)
dans `now-playing.json`, dans son repertoire de configuration, et supprime le
fichier a la fin de la lecture. Le plugin lit ce fichier : les barres suivent
le son reel. `pocket`, qui joue pendant qu'il genere, complete le fichier au
fur et a mesure.

- `vox · preparing` : Claude a appele vox et aucun son n'a encore commence.
- `vox · speaking` : le spectre de ce que les haut-parleurs jouent.
- `vox · listening` : `vox hear` enregistre. Ces barres-la sont synthetiques.

Le plugin reagit aux outils MCP `vox_speak`, `vox_hear` et `vox_pack_play`, aux
commandes `vox ...` lancees depuis le shell et au hook `Stop`. Le backend `say`
joue le son hors de vox et n'ecrit rien : l'affichage reste sur `preparing`.
Avec un `--volume` autre que 1.0, vox joue lui-meme le son et ecrit le spectre.
Un appel avec `-o` ne joue rien et n'ecrit rien non plus.

Details dans [plugins/vox](../plugins/vox/README.md).

## Serveur MCP (`vox serve`)

Lance le serveur MCP sur stdio (JSON-RPC 2.0, protocole `2024-11-05`). C'est cette commande que les outils IA appellent apres `vox init`.

```bash
vox serve    # Lance le serveur (bloque, lit stdin, ecrit stdout)
```

Le serveur est normalement lance automatiquement par l'outil IA. Il n'est pas necessaire de le lancer manuellement, sauf pour du debug.

Les instructions envoyees a l'agent a l'initialisation decrivent quand parler,
la boucle de conversation `vox_hear` → reflexion → `vox_speak` (sans cle API :
l'agent est le cerveau) et l'existence du plugin Claude Code.

### Reference complete des 14 outils MCP

| Outil | Description | Parametres |
|-------|-------------|------------|
| `vox_speak` | Synthetise et joue du texte | **`text`** (requis, string) : texte a prononcer. `voice` (string) : nom de voix ou clone. `lang` (string) : code langue. `backend` (string) : pocket/piper/qwen-native, say (macOS), kokoro (build `--features kokoro`). `style` (string) : calm/energetic/warm/authoritative/cheerful/serious, sans effet. `gender` (string) : feminine/masculine, sans effet. `rate` (integer) : debit mots/min (say uniquement). `volume` (number) : multiplicateur, ramene entre 0.0 et 5.0, defaut 1.0. Pas de parametre de sortie fichier. |
| `vox_list_voices` | Liste les voix d'un backend | `backend` (string) : pocket/piper/qwen-native, say (macOS), kokoro (build `--features kokoro`). Defaut : le backend que `vox_speak` utiliserait (preference `backend`, sinon le defaut selon la preference `lang`). |
| `vox_clone_list` | Liste les voice clones | Aucun parametre. |
| `vox_clone_add` | Ajoute un voice clone | **`name`** (requis, string) : nom du clone (un nom simple, pas deja pris). **`audio`** (requis, string) : chemin du fichier audio de reference (wav, mp3, flac ou ogg), converti et garde en WAV dans `clones/`. `text` (string) : transcription de l'audio (ameliore la qualite). |
| `vox_clone_remove` | Supprime un voice clone | **`name`** (requis, string) : nom du clone a supprimer. |
| `vox_config_show` | Affiche les preferences | Aucun parametre. Retourne les memes lignes que `vox config show` : backend, voice, lang, rate, gender, style, model, stt_model, pack, acceleration. |
| `vox_config_set` | Modifie une preference | **`key`** (requis, string) : cle (backend/voice/lang/rate/model/stt_model/pack ; `gender` et `style` sont acceptes, sans effet). **`value`** (requis, string) : valeur. |
| `vox_stats` | Statistiques d'utilisation | Aucun parametre. Retourne : total requests, total chars, 10 dernieres entrees. |
| `vox_pack_list` | Liste les sound packs | Aucun parametre. Retourne : packs installes (avec actif marque) + quelques noms a installer. |
| `vox_pack_install` | Installe un sound pack | **`name`** (requis, string) : nom d'un pack du registre peon-ping (peon, peon_fr, peon_pl, peasant, peasant_fr, sc_kerrigan, sc_battlecruiser, ra2_soviet_engineer, ou tout autre pack du registre). |
| `vox_pack_set` | Active un sound pack | **`name`** (requis, string) : nom du pack installe. |
| `vox_pack_play` | Joue un son d'un pack | `category` (string, defaut: "greeting") : greeting/acknowledge/complete/error/permission/resource_limit/annoyed. `pack` (string) : nom du pack (utilise le pack actif si omis). |
| `vox_pack_remove` | Supprime un sound pack | **`name`** (requis, string) : nom du pack. Si le pack supprime etait actif, le pack actif est remis a vide. |
| `vox_hear` | Enregistre et transcrit (STT) | `lang` (string, defaut: auto-detection) : code langue Whisper. `timeout` (integer, defaut: 30) : duree max en secondes. `silence` (number, defaut: 2.0) : secondes de silence avant arret. `model` (string) : repo Whisper (defaut `openai/whisper-base`). `file` (string) : fichier WAV a transcrire au lieu du micro. Toutes plateformes. |

Les parametres en **gras** sont requis. Le serveur renvoie `isError: true` si un parametre requis est manquant ou invalide.

## Speech-to-Text (toutes plateformes)

Transcription locale via Whisper sur candle (Rust pur, 99 langues). Whisper utilise le GPU dans un build Metal (Apple Silicon) ou CUDA (Linux x86_64, NVIDIA), et retombe sur le CPU si le GPU ne peut pas etre ouvert ; un build CPU utilise toujours le CPU. La ligne `acceleration:` de `vox config show` indique le build installe.
Le modele est telecharge depuis Hugging Face au premier usage. Le serveur MCP le garde charge entre deux appels.

```bash
# CLI
vox hear                                   # Ecoute, detecte la langue, transcrit
vox hear -l fr -t 60 -s 3.0                # Francais force, timeout 60s, silence 3s
vox hear -m openai/whisper-large-v3-turbo  # Meilleure qualite (GPU + 16 Go RAM conseilles)
vox hear -f enregistrement.wav             # Transcrire un fichier WAV au lieu du micro

# Via MCP
vox_hear                           # Utilise par l'assistant IA
```

L'enregistrement commence a la premiere trame de voix et s'arrete apres `-s`
secondes de silence (defaut 2.0) ou au bout de `-t` secondes (defaut 30). Le
texte sort sur stdout, les messages d'etat sur stderr.

Variables : `VOX_STT_MODEL` (repo Whisper, defaut `openai/whisper-base`, ~630 Mo de RAM mesures sur Apple M2), `VOX_VAD_THRESHOLD` (seuil RMS du detecteur de silence, entre 0 et 1, defaut 0.0125 ; c'est un plafond : le seuil effectif s'ajuste au niveau du micro et au bruit ambiant mesure sur les 300 premieres ms, sans descendre sous 0.002 ni depasser cette valeur, elle-meme plafonnee a 0.1), `VOX_VAD_DEBUG=1` (affiche les niveaux RMS et le seuil retenu).
Aucun prerequis externe : la capture micro utilise cpal.

Mesure avec Whisper base sur 24.6 s d'audio, Linux x86_64 12 coeurs (WSL2) avec
une RTX 4070 Ti SUPER : environ 26 s avec le build CPU, environ 1.4 s avec le
build CUDA, meme transcription.

## Mode conversation (macOS)

Boucle vocale complete : ecouter → reflechir → parler. `vox chat` appelle
lui-meme l'API Claude : il n'existe que sur macOS et demande
`ANTHROPIC_API_KEY`.

```bash
export ANTHROPIC_API_KEY=sk-...
vox chat                           # Conversation avec Claude
vox chat -v patrick -l fr          # Avec voice clone en francais
```

A chaque tour, parlez puis appuyez sur Entree : vox transcrit avec Whisper,
envoie le texte a l'API Claude en streaming et prononce la reponse phrase par
phrase, avec `say` ou, si `-v` designe un clone, avec `qwen-native`. Le message
d'accueil et l'au revoir sont prononces de la meme facon. Les messages
affiches, le message d'accueil, l'au revoir et la consigne donnee a Claude sont
en anglais par defaut, en francais avec `-l fr` ou une preference `lang` a
`fr` ; pour une autre langue ils restent en anglais, et la consigne demande a
Claude de repondre dans cette langue. Le modele Claude est celui de
`VOX_CHAT_MODEL`, `claude-haiku-4-5` par defaut. Dire
"au revoir", "arrete", "stop", "quit", "exit", "bye" ou "goodbye" termine la
conversation.

La boucle MCP (`vox_hear` puis `vox_speak`, l'assistant faisant la reflexion)
ne demande aucune cle et fonctionne sur toutes les plateformes.

## Lecture de voix

Lister les voix disponibles pour un backend.

```bash
vox --list-voices                  # Backend par defaut (pocket) : alba, marius, javert, jean, fantine, cosette, eponine, azelma
vox -b say --list-voices           # Voix macOS
vox -b piper --list-voices         # Voix piper par defaut, une par langue
```

## Diagnostic

`vox config show` se termine par une ligne `acceleration:` qui dit avec quoi le
binaire a ete compile : `Metal (GPU): used by Whisper and qwen-native`,
`CUDA (NVIDIA GPU): used by Whisper and qwen-native` ou `CPU only`. Elle decrit
le build, pas le peripherique utilise a l'instant.

`VOX_TIMINGS=1` affiche sur stderr, pour chaque etape d'un enonce, le temps
ecoule depuis le lancement, la duree de l'etape et son nom :

```text
$ VOX_TIMINGS=1 vox -l fr -o essai.wav "Bonjour le monde, ceci est un essai."
[timing]     0.1 ms  +    0.1 ms  arguments parsed
[timing]     0.5 ms  +    0.5 ms  preferences and backend resolved
[timing]     0.6 ms  +    0.1 ms  piper: espeak data ready
[timing]   522.4 ms  +  521.8 ms  piper: model loaded
[timing]   827.1 ms  +  304.7 ms  piper: audio synthesized
[timing]   827.9 ms  +    0.7 ms  piper: wav written
```

(Sortie relevee sur un portable Apple A18 Pro, 8 Go, build Metal de release.)

Le choix du build a l'installation (`VOX_GPU=auto|cuda|cpu`) est decrit dans le
[README](../README_fr.md#choix-du-gpu-a-linstallation-vox_gpu).

## Donnees locales

Tout est stocke localement, aucune donnee n'est envoyee a un serveur externe (sauf le mode chat qui utilise l'API Claude). Les modeles sont telecharges depuis Hugging Face au premier usage.

```
~/.config/vox/          # Linux ; sur macOS : ~/Library/Application Support/vox/
  vox.db                # SQLite : preferences, clones, logs
  clones/               # References des clones (vox clone add et vox clone record)
  packs/                # Sound packs installes
  piper/                # Voix piper telechargees et donnees espeak-ng
  pocket/               # Configuration du modele pocket
  models.toml           # Optionnel : remplace la configuration des modeles compilee
  daemon.pid            # Present tant que le daemon tourne
  daemon.log            # Sortie du daemon, reecrit a chaque demarrage
  now-playing.json      # Present pendant une lecture (spectre pour le visualiseur)
```

Les modeles `pocket`, `qwen-native` et Whisper sont dans le cache Hugging Face
(`~/.cache/huggingface/hub`).

Variables d'environnement :
- `VOX_CONFIG_DIR` — repertoire de configuration alternatif
- `VOX_DB_PATH` — chemin de base de donnees alternatif
- `VOX_STT_MODEL` — repo Whisper utilise par `vox hear`
- `VOX_VAD_THRESHOLD`, `VOX_VAD_DEBUG` — detecteur de silence, voir Speech-to-Text
- `VOX_CUES` — `0`, `false` ou `off` desactive les bips d'enregistrement
- `VOX_TIMINGS` — `1` affiche le temps de chaque etape sur stderr
- `VOX_DAEMON_PORT` — port du daemon (defaut 19876)
- `HF_TOKEN` — jeton Hugging Face, requis pour le clonage avec `pocket`
- `ANTHROPIC_API_KEY` — requis par `vox chat`
- `VOX_CHAT_MODEL` — modele Claude utilise par `vox chat` (defaut `claude-haiku-4-5`)
- `VOX_GPU`, `VOX_INSTALL_DIR` — lues par l'installeur et non par vox, voir le [README](../README_fr.md#choix-du-gpu-a-linstallation-vox_gpu)
