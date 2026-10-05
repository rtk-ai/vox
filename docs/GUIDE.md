# Guide utilisateur

## Installation

### Script rapide (macOS Apple Silicon / Linux x86_64 et ARM64 / WSL2)

```bash
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh
```

Avec Homebrew (macOS Apple Silicon, Linux) : `brew install rtk-ai/tap/vox`.

### Quel build choisir ?

La plupart des utilisateurs n'ont rien a faire. Les deux voix par defaut,
`pocket` (anglais) et `piper` (autres langues), tournent sur le CPU dans tous
les builds. Le GPU n'accelere que deux choses : la transcription Whisper
(`vox hear`) et le backend `qwen-native` (clonage de voix). Si le GPU ne peut
pas etre ouvert a l'execution, les deux retombent sur le CPU.

| Plateforme | Build | Comment l'obtenir |
|------------|-------|-------------------|
| macOS Apple Silicon | Metal (GPU) | Installeur ou Homebrew |
| macOS Intel | aucun | Non supporte (voir ci-dessous) |
| Linux x86_64 | CPU, ou CUDA avec une carte NVIDIA | Installeur (il pose la question). Homebrew, `.deb` et `.rpm` : CPU |
| Linux ARM64 | CPU | Installeur, Homebrew, `.deb` ou `.rpm` |
| Windows x86_64 | CPU | Le `.zip` des GitHub Releases. Avec une carte NVIDIA : WSL2 et l'installeur Linux |

Pour savoir quel build est installe : `vox config show` affiche une ligne
`acceleration:` (`Metal (GPU): used by Whisper and qwen-native`,
`CUDA (NVIDIA GPU): used by Whisper and qwen-native` ou `CPU only`). Elle
decrit la compilation du binaire, pas le peripherique utilise.

Il n'y a pas de build pour Mac Intel : le runtime ONNX utilise par `piper` n'a
pas de binaire precompile pour cette plateforme (derniere release avec un
binaire Mac Intel : v0.10.0). L'installeur s'arrete avec un message qui le dit,
avant tout telechargement. Il n'y a pas non plus de build CUDA pour Windows :
avec une carte NVIDIA, utilisez WSL2 et l'installeur Linux.

### Choix du GPU a l'installation (`VOX_GPU`)

Sur Linux x86_64, quand `nvidia-smi` fonctionne, l'installeur demande s'il faut
installer le build CUDA. La question precise ce que cela change (clonage de
voix avec `qwen-native` et transcription Whisper plus rapides ; les voix par
defaut restent sur le CPU) et ce qu'il faut. Reponse par defaut : non.

| `VOX_GPU` | Effet sur Linux x86_64 avec une carte NVIDIA |
|-----------|----------------------------------------------|
| `auto` (defaut) | Pose la question si un terminal est accessible. Sinon installe le build CPU et indique comment obtenir le build CUDA |
| `cuda` | Installe le build CUDA sans poser de question |
| `cpu` | Installe le build CPU sans poser de question |

```bash
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | VOX_GPU=cuda sh
```

Toute autre valeur est une erreur. `VOX_GPU=cuda` sur une plateforme ou il ne
s'applique pas (macOS, Linux ARM64) affiche un avertissement puis installe le
build normal de la plateforme. macOS recoit toujours le build Metal, le seul
build macOS : `VOX_GPU=cpu` n'y affiche lui aussi qu'un avertissement. Sur
Linux x86_64 sans `nvidia-smi` fonctionnel, `VOX_GPU=cuda` affiche un
avertissement et essaie quand meme le build CUDA.

Le build CUDA a besoin, sur la machine qui l'execute :

- du pilote NVIDIA (il fournit `libcuda.so.1`) ;
- des bibliotheques runtime CUDA 12 cuBLAS (`libcublas.so.12`,
  `libcublasLt.so.12`) et cuRAND (`libcurand.so.10`).

Ces bibliotheques sont liees dynamiquement : sans elles le binaire CUDA ne
demarre pas du tout. L'installeur lance donc le binaire telecharge
(`vox --version`) avant de l'installer. S'il ne demarre pas, il liste les
bibliotheques manquantes et installe le build CPU a la place. Il installe
aussi le build CPU, avec un avertissement, quand le telechargement echoue ou
que la release ne contient pas de binaire CUDA : c'est le cas de v0.16.0 et des
versions precedentes. Ses
dernieres lignes disent quel build a ete installe et, pour un build CPU sur
une machine avec une carte NVIDIA, comment changer.

Le build CUDA est compile pour CUDA 12.6 et la compute capability 8.0 : il
demande une carte RTX serie 30 ou plus recente. Verifie sur une RTX 4070 Ti
SUPER sous WSL2 : le build CUDA demarre, et Whisper transcrit 24,6 s d'audio
en 1,4 s environ contre 26 s pour le build CPU sur la meme machine. Le
binaire produit par le workflow de release n'a pas encore ete execute : la CI
le compile sur des machines sans GPU.

### Depuis les sources

```bash
# Standard (CPU)
cargo install --git https://github.com/rtk-ai/vox

# macOS Apple Silicon avec GPU Metal
cargo install --git https://github.com/rtk-ai/vox --features metal

# Linux x86_64 avec GPU NVIDIA
cargo install --git https://github.com/rtk-ai/vox --features cuda

# Depuis un clone du depot (memes --features)
cargo install --path .
```

Ne lancez pas `cargo install vox` : sur crates.io ce nom appartient a un autre
projet (github.com/bearcove/vox).

Prerequis de compilation :

- Linux : `sudo apt install build-essential cmake pkg-config clang libclang-dev libssl-dev libasound2-dev`.
- `--features cuda` : le toolkit CUDA 12 avec `nvcc` dans le `PATH` (la CI
  utilise la version 12.6.3). Les noyaux GPU sont compiles pour une seule
  compute capability, lue via `nvidia-smi` sur la machine de compilation.
  Definissez `CUDA_COMPUTE_CAP` si vous compilez pour une autre machine ou sans
  `nvidia-smi` dans le `PATH` (sous WSL il est dans `/usr/lib/wsl/lib`). La
  plus petite valeur qui compile est `80` (8.0, RTX serie 30), utilisee par la
  CI et par le build de release.

### Prerequis optionnels

Rien de ce qui suit n'est necessaire pour installer vox. Le tableau dit ce
qu'il faut pour chaque usage.

| Composant | Pour quoi | Comment |
|-----------|-----------|---------|
| Acces reseau au premier appel | Telecharger le modele du backend utilise | Automatique. `pocket` : environ 226 Mo ; une voix `piper` : environ 60 Mo ; Whisper `base` : environ 280 Mo ; `qwen-native` : environ 2,3 Go |
| Micro | `vox hear`, `vox clone record`, `vox chat` | Sur macOS, autoriser le terminal a utiliser le micro |
| `ANTHROPIC_API_KEY` | `vox chat` (macOS) | `export ANTHROPIC_API_KEY=sk-ant-...` |
| `HF_TOKEN` | Clonage de voix avec le backend `pocket` | Jeton HuggingFace, apres avoir accepte la licence sur https://huggingface.co/kyutai/pocket-tts |

## Demarrage rapide

```bash
# Parler (anglais, backend pocket)
vox "Hello, world!"

# En francais (backend piper)
vox -l fr "Bonjour le monde"

# Pipe depuis une commande
echo "Message important" | vox

# Voir les voix du backend
vox --list-voices

# Ecrire un fichier au lieu de parler
vox -o note.wav "Texte a enregistrer"
```

Sans `-l`, ou avec `-l en`, vox utilise le backend `pocket`, qui parle anglais.
Avec une autre langue il utilise `piper`, qui a une voix par langue. Ce choix
est le meme sur toutes les plateformes. Codes de langue : `en`, `fr`, `es`,
`de`, `it`, `pt`, `zh`, `ja`, `ko`, `ru`, `ar`, `nl`. Au 5 octobre 2026,
`-l ja` echoue avec `piper` : l'adresse de sa voix japonaise repond `404`
(voir piper dans Backends en detail).

Le premier appel d'un backend telecharge son modele (tailles dans Prerequis
optionnels).

### Ecrire un fichier audio (`-o`)

`-o fichier.wav` ecrit un fichier WAV au lieu de jouer le son. L'option
fonctionne avec tous les backends et sur toutes les plateformes.

```bash
vox -o note.wav "Texte a enregistrer"
vox -l fr -o sorties/bonjour.wav "Bonjour"
```

- vox affiche `Saved audio to <chemin>` sur la sortie d'erreur.
- Les dossiers manquants du chemin sont crees, sauf avec le backend `say`.
- Avec `say`, donnez une extension `.wav` pour obtenir un WAV.
- Un appel avec `-o` ne passe pas par le daemon.
- L'outil MCP `vox_speak` n'a pas cette option : ecrire un fichier reste une
  commande locale.

## Configuration avec un assistant IA

La methode recommandee est d'utiliser vox comme serveur MCP :

```bash
vox init
```

Cette commande inscrit le serveur MCP de vox dans la configuration de 14
outils IA : Claude Code, Claude Desktop, Cursor, Windsurf, VS Code / Copilot,
Zed, Codex, OpenCode, Gemini, Amazon Q, Cline, Roo Code, Kilo Code et Amp. Le
fichier de configuration d'un outil est cree s'il n'existe pas, que l'outil
soit installe ou non. Redemarrez votre outil apres l'init.

Le serveur expose 14 outils MCP. L'assistant IA peut alors :
- Vous parler apres avoir termine une tache (`vox_speak`)
- Lister les voix (`vox_list_voices`)
- Gerer vos clones de voix (`vox_clone_list`, `vox_clone_add`, `vox_clone_remove`)
- Lire et changer les preferences (`vox_config_show`, `vox_config_set`) et lire
  les statistiques d'usage (`vox_stats`)
- Gerer et jouer les sound packs (`vox_pack_list`, `vox_pack_install`,
  `vox_pack_set`, `vox_pack_play`, `vox_pack_remove`)
- Ecouter votre voix et la transcrire (`vox_hear`), sur toutes les plateformes

Avec `vox_hear` et `vox_speak`, l'assistant peut tenir une conversation vocale :
il ecoute, reflechit, repond. Cette boucle n'a besoin d'aucune cle API.

Le serveur MCP tourne pendant toute la session de l'outil : un modele charge
reste en memoire d'un appel a l'autre.

`vox init` accepte d'autres modes :

| Commande | Effet |
|----------|-------|
| `vox init` ou `vox init -m mcp` | Serveur MCP pour les 14 outils IA |
| `vox init -m cli` | Bloc d'instructions dans le `CLAUDE.md` du dossier courant et hook `Stop` dans `.claude/settings.json` : Claude Code appelle `vox` par le shell |
| `vox init -m skill` | Commande `/speak` pour Claude Code (`~/.claude/commands/speak.md`) |
| `vox init -m all` | Les trois |

En mode `cli`, `-l <langue>` fixe la langue demandee a l'assistant et celle de
la phrase du hook. Par defaut : la preference `lang`, sinon la langue du
systeme.

### Exemple avec Claude Code

Apres `vox init`, Claude Code peut utiliser les outils MCP directement :

```
> Corrige le bug dans auth.rs

[Claude corrige le bug, puis parle :]
"Le bug d'authentification a ete corrige. Le token etait expire
 car la duree etait en secondes au lieu de millisecondes."
```

### Plugin Claude Code

Le depot fournit un plugin Claude Code (`plugins/vox`). Il affiche un
visualiseur au-dessus du prompt pendant que vox parle ; les barres suivent le
spectre du son joue. Pour l'installer, dans une session Claude Code :

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
```

- `/vox-wave` affiche un apercu, sans vox ni son.
- `/vox-wave color` liste les couleurs et les presets ; `/vox-wave color ocean`
  applique un preset.
- Il faut Claude Code 2.1.287 ou plus recent.
- Le backend `say` joue le son hors de vox : le visualiseur n'a alors pas de
  spectre a afficher.

Pendant la lecture, vox ecrit le spectre du son dans `now-playing.json`, dans
son repertoire de configuration, et supprime ce fichier a la fin. Le plugin lit
ce fichier.

`vox init` rappelle les deux commandes d'installation a la fin de sa sortie.
Le bloc `CLAUDE.md` de `vox init -m cli` et les instructions du serveur MCP
signalent aussi le plugin a l'assistant.

## Personnaliser la voix

```bash
# Langue par defaut (avec fr, le backend par defaut devient piper)
vox config set lang fr

# Voix par defaut : une voix pocket, une voix say ou un clone
vox config set voice marius

# Backend par defaut, pour toutes les langues
vox config set backend piper

# Voir la config actuelle
vox config show

# Tout remettre aux valeurs par defaut
vox config reset
```

Cles acceptees : `backend`, `voice`, `lang`, `rate`, `gender`, `style`,
`model`, `stt_model`, `pack`.

- `backend` : `pocket`, `piper`, `qwen-native`, et `say` sur macOS. Une fois
  enregistre, il sert pour toutes les langues et remplace le choix automatique
  entre pocket et piper : avec `backend = pocket`, `vox -l fr "..."` utilise
  pocket, qui ne parle qu'anglais.
- `voice` : lue par `pocket` (`alba`, `marius`, ...), par `say` (voix du
  systeme) et pour les clones. `piper` l'ignore : sa voix depend de la langue.
- `rate` : debit en mots par minute, pour `say` uniquement.
- `gender` et `style` : enregistres, mais aucun backend actuel ne les utilise.
  Il en va de meme pour les drapeaux `--gender` et `--style`.
- `model` et `stt_model` : voir Modeles personnalises.
- `pack` : le sound pack actif, regle par `vox pack set`.

Un drapeau de la ligne de commande passe avant la preference. Exception :
`-b pocket` ne remplace pas une preference `backend` enregistree. Dans ce cas,
changez la preference (`vox config set backend pocket`).

## Cloner votre voix

Le clonage passe par le backend `qwen-native`.

```bash
# Depuis un fichier WAV existant
vox clone add mavoix --audio ~/enregistrement.wav --text "Transcription exacte"

# Enregistrer depuis le micro
vox clone record mavoix --duration 8 --text "Ce que je dis"

# Utiliser le clone
vox -b qwen-native -l fr -v mavoix "Ceci parle avec ma voix clonee"

# Lister, supprimer
vox clone list
vox clone remove mavoix
```

- `vox clone add` enregistre le chemin du fichier sans le copier : ne le
  deplacez pas. `vox clone record` ecrit `clones/<nom>.wav` dans le repertoire
  de configuration.
- Le fichier doit etre un WAV. `vox clone add` accepte aussi les extensions
  `mp3`, `flac`, `ogg` et `m4a`, mais la synthese echoue ensuite avec
  `Failed to open WAV file`.
- Avec `-v <clone>`, vox passe de lui-meme a `qwen-native`, sauf quand le
  backend courant est `pocket`, c'est-a-dire par defaut sans `-l` ou avec
  `-l en`. `pocket` ne clone qu'avec `HF_TOKEN` et ses poids a acces restreint
  (licence a accepter sur https://huggingface.co/kyutai/pocket-tts). Sans eux il
  s'arrete avec `Voice cloning from a WAV needs the gated weights`. D'ou le
  `-b qwen-native` de l'exemple.
- Par le serveur MCP, un clone utilise toujours `qwen-native`.
- Langues de `qwen-native` avec un clone : `en`, `fr`, `es`, `de`, `it`, `pt`,
  `zh`, `ja`, `ko`, `ru`. Sans `-l`, il prend `en`.

Deux bips encadrent l'enregistrement : aigu au demarrage, plus grave a l'arret.
Ils sont joues en dehors de la capture, donc absents du clone. `VOX_CUES=0` les
desactive.

Pour de meilleurs resultats :
- Enregistrement de 5-8 secondes : au-dela, la synthese devient nettement plus
  lente (mesure sur M2 : reference de 6 s, 21 s de generation ; reference de
  14 s, 11 minutes)
- Environnement calme, sans bruit de fond
- Parler naturellement, pas trop vite
- Fournir la transcription exacte avec `--text`

## Sound packs

Les packs sont des sons courts ranges par categorie, au format des packs
peon-ping.

```bash
vox pack list                  # Packs installes et packs installables
vox pack install peon          # Installer
vox pack set peon              # Activer
vox pack play greeting         # Jouer un son de la categorie
vox pack play error -p peon    # Jouer depuis un pack precis
vox pack remove peon           # Desinstaller
```

Categories : `greeting` (defaut), `acknowledge`, `complete`, `error`,
`permission`, `annoyed`. `vox pack play` choisit un son au hasard dans la
categorie et affiche sa replique.

Packs proposes a l'installation : `peon`, `peon_fr`, `peon_pl`, `peasant`,
`peasant_fr`, `sc_kerrigan`, `sc_battlecruiser`, `ra2_soviet_engineer`.

Etat au 5 octobre 2026 : `vox pack install peon` echoue avec
`Failed to download manifest: HTTP 404 Not Found`. L'adresse d'ou vox
telecharge les packs (depot peon-ping, dossier `packs/`) ne repond plus.

## Conversation vocale (macOS)

`vox chat` appelle lui-meme l'API Claude : il lui faut une cle. La commande
n'existe que sur macOS.

```bash
export ANTHROPIC_API_KEY=sk-ant-...
vox chat -l fr
```

La boucle : vous parlez puis appuyez sur Entree, Whisper transcrit, Claude
repond, vox dit la reponse phrase par phrase. Pour terminer, dites "au revoir",
"arrete", "stop", "quit", "exit", "bye" ou "goodbye".

- Les reponses sont dites par `say`. Avec `-v <clone>`, elles le sont par
  `qwen-native` avec votre voix clonee.
- Le message d'accueil est dit par `qwen-native` dans les deux cas : au premier
  lancement, vox telecharge ce modele (environ 2,3 Go).
- `-l` fixe la langue de la transcription et, avec un clone, celle de la
  synthese. Les messages affiches, le message d'accueil et la consigne donnee
  a Claude sont en francais quelle que soit sa valeur.

Sur les autres plateformes, ou sans cle, utilisez la boucle MCP (`vox_hear`
puis `vox_speak`) decrite plus haut.

## Transcription

`vox hear` enregistre le micro et transcrit avec Whisper, en local, sur toutes
les plateformes.

```bash
vox hear -l fr
# Parlez... (s'arrete apres 2 s de silence, 30 s au plus)
# => "Votre texte transcrit ici"

# Transcrire un fichier WAV au lieu du micro
vox hear -f reunion.wav

# 1 s de silence pour arreter, 60 s au plus
vox hear -s 1.0 -t 60

# Autre modele Whisper
vox hear -m openai/whisper-small
```

- Sans `-l`, Whisper detecte la langue.
- Le texte sort sur la sortie standard ; les messages (`Listening...`,
  `Transcribing...`) sur la sortie d'erreur.
- Un bip aigu marque le debut de l'enregistrement, un bip plus grave la fin.
  `VOX_CUES=0` les desactive.
- Le fichier passe a `-f` doit etre un WAV ; sa frequence et son nombre de
  canaux sont libres.
- Modele par defaut : `openai/whisper-base`, telecharge au premier appel. Pour
  en changer, voir Modeles personnalises.
- Whisper utilise le GPU avec un build Metal ou CUDA. Mesure sur 24,6 s d'audio
  (Linux x86_64 12 coeurs sous WSL2, RTX 4070 Ti SUPER) : environ 26 s avec le
  build CPU, environ 1,4 s avec le build CUDA, pour la meme transcription.

## Backends en detail

vox est un seul binaire Rust : les backends y sont compiles (`say` sur macOS
seulement, `kokoro` seulement avec `--features kokoro`), et il n'y a pas de
Python, ni a la compilation ni a l'execution.

Les mesures de cette section donnent le temps entre le lancement de la commande
et le premier son. Sauf mention contraire, elles ont ete prises sur un portable
Apple A18 Pro (8 Go, mode economie d'energie) avec le build Metal, pour une
phrase de 3 secondes.

### pocket (defaut pour l'anglais)
- Modele pocket-tts de Kyutai. Il tourne sur le CPU dans tous les builds.
- Anglais uniquement. Utilise quand aucune langue n'est donnee ou avec `-l en`.
- 8 voix : `alba` (defaut), `marius`, `javert`, `jean`, `fantine`, `cosette`,
  `eponine`, `azelma`. Choix avec `-v`.
- Le son part pendant que la suite de la phrase est encore generee.
- Clonage possible seulement avec `HF_TOKEN` et les poids a acces restreint
  (voir Cloner votre voix).
- Mesure sans daemon : 0,18 a 0,55 s. Un texte long (15 s d'audio) commence
  apres 0,81 s.

### piper (defaut pour les autres langues)
- Modeles ONNX. Il tourne sur le CPU dans tous les builds.
- Une voix par langue, choisie par `-l` : `fr`, `es`, `de`, `it`, `pt`, `zh`,
  `ko`, `ru`, `ar`, `nl`, `en`. Un autre code donne la voix anglaise. `-v` est
  ignore.
- Le code prevoit aussi une voix japonaise (`ja`), mais son adresse de
  telechargement repond `404` (verifie le 5 octobre 2026) : `vox -l ja "..."`
  s'arrete avec `server refused to send ja_JP-kokoro-medium.onnx`.
  `qwen-native` accepte `ja`.
- La voix d'une langue est telechargee a son premier usage, dans `piper/` du
  repertoire de configuration.
- La phrase entiere est synthetisee avant d'etre jouee. La sortie audio est
  ouverte pendant le chargement du modele.
- Mesure en francais : environ 0,75 s sans daemon, 0,26 a 0,32 s avec le
  daemon.

### qwen-native (clonage de voix)
- Qwen3-TTS execute en Rust avec candle. Modele par defaut :
  `Qwen/Qwen3-TTS-12Hz-0.6B-Base`.
- Il sert au clonage de voix. Sans clone, le modele choisit lui-meme la voix et
  `-l` n'a pas d'effet ; vox le signale par un avertissement.
- Codes de langue acceptes : `en`, `fr`, `es`, `de`, `it`, `pt`, `zh`, `ja`,
  `ko`, `ru`. `-l ar` et `-l nl` sont refuses avec
  `Unsupported language for qwen-native`.
- GPU avec un build Metal ou CUDA, CPU sinon (voir Lire la ligne
  `acceleration:`). Au chargement du modele, vox affiche le peripherique
  utilise : `Using device: ...`. Cette ligne n'apparait pas quand la demande
  passe par le daemon, dont la sortie n'est pas affichee.
- La phrase entiere est generee avant d'etre jouee.
- Mesures du 5 octobre 2026, portable Apple A18 Pro (8 Go), build Metal, modele
  deja telecharge, temps total de la commande avec `-o`, un essai par mesure :
  9,5 s et 10,4 s pour deux phrases anglaises de 7 mots sans clone ; avec un
  clone, 12,7 s (reference de 2,7 s) et 21,1 s (reference de 3,2 s) pour deux
  phrases francaises de 5 mots.
- Ancienne estimation de ce guide, machine non notee : 2 a 5 s quand le modele
  est deja en memoire, 10 a 30 s sinon.
- Le modele reste en memoire dans le serveur MCP et dans le daemon. En ligne de
  commande sans daemon, il est recharge a chaque appel.

### kokoro (builds compiles avec `--features kokoro`)
- Absent des builds distribues : `vox -b kokoro` y repond
  `Unknown backend: kokoro`. Il faut compiler vox depuis les sources en
  ajoutant `kokoro` aux `--features`.
- Les fichiers du modele ne sont pas telecharges automatiquement. Placez
  `kokoro-v1.0.onnx` (environ 325 Mo) et `voices.bin` (environ 28 Mo) dans
  `kokoro/` du repertoire de configuration. S'ils manquent, vox affiche les
  commandes `curl` a lancer.
- Voix predefinies avec un prefixe de langue (`af_`, `ff_`, `jf_`, etc.).
- Pas de clonage.

### say (macOS)
- Appelle la commande `say` du systeme : aucun modele a telecharger.
- Voix du systeme, par exemple `vox -b say -v Thomas "Bonjour"`. La langue
  vient de la voix : `-l` n'a pas d'effet.
- Debit avec `-r`, en mots par minute (`-r 200`).
- Pas de clonage. Les appels ne passent pas par le daemon.
- Ancienne estimation de ce guide, machine non notee : environ 100 ms avant le
  premier son.

## Daemon

Sans daemon, chaque commande `vox "texte"` charge le modele avant de parler. Le
daemon est un processus qui garde les modeles en memoire entre deux appels.

```bash
vox daemon start     # Lance le daemon en arriere-plan
vox daemon status    # Etat, duree de fonctionnement, modeles charges
vox daemon stop      # Arrete le daemon
```

- Il n'est jamais lance automatiquement.
- Quand il tourne, les demandes `pocket`, `piper`, `qwen-native` et `kokoro`
  de la ligne de commande passent par lui. `say`, les appels avec `-o`,
  `vox hear` et le serveur MCP n'y passent pas.
- Un modele est charge a la premiere demande qui l'utilise, pas au demarrage du
  daemon : le premier appel paie encore le chargement.
- `vox daemon status` ne liste que `pocket` et `qwen-native` : avec une voix
  `piper` seule en memoire, il affiche `(none loaded yet)`.
- Il s'arrete seul apres 300 secondes sans demande.
  `vox daemon start --idle-timeout 1800` change ce delai.
- Il ecoute sur `127.0.0.1:19876`. `VOX_DAEMON_PORT` change le port ; la
  variable doit avoir la meme valeur pour le daemon et pour les commandes `vox`.
- Les demandes sont jouees l'une apres l'autre.

Quand il sert :

- Pour les appels repetes en ligne de commande : hook `Stop`, mode
  `vox init -m cli`, scripts. Avec `piper` en francais, le premier son arrive
  apres 0,26 a 0,32 s au lieu de 0,75 s environ (mesure decrite dans Backends
  en detail).
- Pour `qwen-native`, qui recharge sinon son modele a chaque appel.

Quand il ne sert pas : avec le serveur MCP, qui garde deja ses modeles en
memoire, et avec `say`.

## Configuration avancee

### Variables d'environnement

| Variable | Description | Defaut |
|----------|-------------|--------|
| `VOX_CONFIG_DIR` | Repertoire de configuration | `~/Library/Application Support/vox` (macOS), `~/.config/vox` (Linux), `%APPDATA%\vox` (Windows) |
| `VOX_DB_PATH` | Chemin de la base de donnees | `vox.db` dans le repertoire de configuration |
| `VOX_STT_MODEL` | Repo Whisper pour la transcription | `[whisper] model_id` de models.toml |
| `VOX_TIMINGS` | `1` : affiche la duree de chaque etape sur la sortie d'erreur | Desactive |
| `VOX_CUES` | `0` : supprime les bips de debut et de fin d'enregistrement | Bips actifs |
| `VOX_VAD_THRESHOLD` | Seuil du detecteur de silence, strictement entre 0 et 1 | `0.0125` |
| `VOX_DAEMON_PORT` | Port du daemon | `19876` |
| `HF_TOKEN` | Jeton HuggingFace, pour le clonage avec `pocket` | Aucun |
| `ANTHROPIC_API_KEY` | Cle API Claude (requise pour `vox chat`) | Aucun |

`VOX_GPU`, lue par le script d'installation, est decrite dans Installation.

### Mesurer les etapes (`VOX_TIMINGS`)

```bash
VOX_TIMINGS=1 vox -l fr -o /tmp/test.wav "Le build est termine et les tests passent."
```

```text
[timing]     0.1 ms  +    0.1 ms  arguments parsed
[timing]     1.7 ms  +    1.6 ms  preferences and backend resolved
[timing]     2.5 ms  +    0.8 ms  piper: espeak data ready
[timing]   387.8 ms  +  385.3 ms  piper: model loaded
[timing]   660.4 ms  +  272.6 ms  piper: audio synthesized
[timing]   662.0 ms  +    1.6 ms  piper: wav written
```

La premiere colonne est le temps ecoule depuis le lancement, la deuxieme le
temps depuis l'etape precedente. Cet exemple a ete releve sur un portable Apple
A18 Pro. Sans `-o`, des lignes `audio:` donnent l'ouverture de la sortie audio
et l'envoi du premier echantillon (`audio: first sample queued`).

### Lire la ligne `acceleration:`

`vox config show` se termine par une ligne `acceleration:`. L'outil MCP
`vox_config_show` la renvoie aussi.

| Valeur | Sens |
|--------|------|
| `Metal (GPU): used by Whisper and qwen-native` | Build Metal (macOS Apple Silicon) |
| `CUDA (NVIDIA GPU): used by Whisper and qwen-native` | Build CUDA (Linux x86_64 avec une carte NVIDIA) |
| `CPU only` | Build sans GPU |

- La ligne decrit la compilation du binaire, pas le peripherique utilise. Si
  le GPU ne peut pas etre ouvert a l'execution, Whisper et `qwen-native`
  retombent sur le CPU.
- `pocket` et `piper` tournent sur le CPU dans tous les builds : la ligne ne
  change rien pour les voix par defaut.
- Pour connaitre le peripherique utilise par `qwen-native`, lisez la ligne
  `Using device: ...` affichee au chargement du modele, en ligne de commande
  sans daemon. Whisper n'affiche pas le sien.
- `CPU only` sur une machine qui a une carte NVIDIA : voir "Quel build
  choisir ?" et "Choix du GPU a l'installation" dans Installation pour passer
  au build CUDA.

### Modeles personnalises

Chaque modele se choisit a quatre niveaux, du plus precis au plus general :

1. le drapeau de l'appel (`-m` / `--model`)
2. la variable d'environnement (STT uniquement : `VOX_STT_MODEL`)
3. la preference stockee (`vox config set ...`)
4. `models.toml` — les defauts compiles dans le binaire, surchargeables en
   placant votre propre fichier `models.toml` dans le repertoire de
   configuration

L'environnement passe avant la preference pour qu'un `VOX_STT_MODEL=...` reste
efficace meme quand une preference a deja ete enregistree.

Pour la synthese, seul `qwen-native` a un modele reglable ; `-m` n'a pas
d'effet sur les autres backends :

```bash
vox -b qwen-native -m "Qwen/Qwen3-TTS-12Hz-1.7B-Base" "Texte"
vox config set model "Qwen/Qwen3-TTS-12Hz-1.7B-Base"
```

Pour la transcription (Whisper) :

```bash
vox hear -m openai/whisper-small
vox config set stt_model openai/whisper-small
```

Et le fichier correspondant :

```toml
[qwen-native]
model_id = "Qwen/Qwen3-TTS-12Hz-0.6B-Base"

[whisper]
model_id = "openai/whisper-base"
```

Les modeles sont telecharges automatiquement depuis HuggingFace Hub au premier
appel. Changer de modele en cours de route n'exige pas de redemarrer le serveur
MCP ni le daemon : le modele en memoire est remplace quand l'identifiant
demande differe. Par le serveur MCP, `vox_speak` ne lit pas la preference
`model` : pour changer le modele de `qwen-native`, modifiez `models.toml`.

Modeles Whisper mesures a chaud sur 11,85 s de francais (Apple M2), temps de
transcription et memoire vive : `tiny` 0,93 s / 350 Mo — `base` 1,52 s /
631 Mo (defaut) — `small` 5,07 s / 1,98 Go. `base` est 3x plus rapide et 3x
plus leger que `small`, qui ne fait qu'une faute de moins sur une phrase de
30 mots.

### Arborescence des donnees locales

Le repertoire de configuration est `~/Library/Application Support/vox` sur
macOS et `~/.config/vox` sur Linux ; `VOX_CONFIG_DIR` le remplace.

```
<repertoire de configuration>/
  vox.db              # SQLite : preferences, clones, statistiques d'usage
  models.toml         # Optionnel : vos modeles par defaut
  clones/             # Enregistrements de `vox clone record` (.wav)
  packs/              # Sound packs installes
    peon/
      manifest.json
      sounds/
  piper/              # Voix piper (.onnx) et donnees espeak-ng
  pocket/             # Configuration du modele pocket
  kokoro/             # Build kokoro uniquement
    kokoro-v1.0.onnx
    voices.bin
  now-playing.json    # Spectre du son, present seulement pendant la lecture
  daemon.pid          # Present tant que le daemon tourne
```

Les poids de `pocket`, de Whisper et de `qwen-native` sont dans le cache
HuggingFace : `~/.cache/huggingface/hub`.

## Verification de l'integration MCP

Apres `vox init`, pour verifier que tout fonctionne :

```bash
# 1. Verifier que le binaire est dans le PATH
which vox

# 2. Tester le serveur MCP manuellement (il s'arrete a la fin de l'entree)
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | vox serve

# 3. Verifier la configuration de Claude Code
grep vox ~/.claude.json

# 4. Tester un appel complet
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | vox serve
```

Le serveur doit repondre avec la liste des 14 outils. Si l'outil IA ne detecte pas vox apres l'init, redemarrez-le.

## Depannage

### "command not found: vox"
Le binaire n'est pas dans votre PATH. Verifiez avec `which vox` ou reinstallez
(voir Installation).

### "Unknown backend: kokoro"
Le backend `kokoro` n'existe que dans un build compile avec
`--features kokoro`. Retirez `-b kokoro` pour utiliser les backends par defaut,
ou voyez kokoro dans Backends en detail.

### "Backend 'say' is only available on macOS"
Retirez `-b say`. Sans `-b`, vox utilise `pocket` (anglais) ou `piper` (autres
langues), qui existent sur toutes les plateformes.

### "server refused to send ja_JP-kokoro-medium.onnx"
La voix japonaise de `piper` n'est plus a l'adresse que vox utilise (`404`,
verifie le 5 octobre 2026). `vox -l ja` et une preference `lang = ja` echouent
donc avec le backend par defaut. Ajoutez `-b qwen-native`, qui accepte `ja`.

### "Voice cloning from a WAV needs the gated weights"
Le clone a ete envoye au backend `pocket` sans `HF_TOKEN`. Ajoutez
`-b qwen-native` a la commande (voir Cloner votre voix).

### Le premier appel est long
Le premier appel d'un backend telecharge son modele (tailles dans Prerequis
optionnels). `pocket` et `piper` l'annoncent sur la sortie d'erreur. Le
telechargement d'une voix piper est abandonne au bout de 300 secondes.

Sur macOS, `pocket` affiche `First run: downloading the pocket-tts model` a
chaque appel, meme quand le modele est deja telecharge : il cherche le cache
dans `~/Library/Caches/huggingface/hub` alors que le modele est dans
`~/.cache/huggingface/hub`. Le modele n'est pas telecharge de nouveau.

### Backend lent

- Mesurez d'abord : `VOX_TIMINGS=1 vox ...` montre ou part le temps (voir
  Mesurer les etapes).
- En ligne de commande, chaque appel recharge le modele. Le daemon l'evite
  (voir Daemon).
- Par le serveur MCP, le modele reste en memoire : seul le premier appel de la
  session le charge.
- `qwen-native` genere la phrase entiere avant de la jouer : une dizaine de
  secondes pour une phrase courte sans clone sur la machine de mesure, plus
  avec un clone (voir Backends en detail). Il utilise le GPU avec un build
  Metal ou CUDA : verifiez la ligne `acceleration:` de `vox config show`.
- Avec un clone, une reference longue ralentit beaucoup la synthese (voir
  Cloner votre voix).

### Qualite du voice cloning

Pour de meilleurs resultats avec le voice cloning :

- **Format** : WAV 16-bit, mono, 16-48 kHz. Les autres formats ne sont pas lus
  a la synthese.
- **Duree** : 5-8 secondes de parole continue
- **Contenu** : parler naturellement, pas trop vite, avec des phrases completes
- **Environnement** : calme, sans bruit de fond, sans echo
- **Transcription** : toujours fournir `--text` avec la transcription exacte

### Corruption de la base de donnees

Si la base SQLite est corrompue (`database disk image is malformed`) :

```bash
# Repertoire de configuration (macOS ; sur Linux : "$HOME/.config/vox")
CONF="$HOME/Library/Application Support/vox"

# Methode 1 : reinitialiser. Les preferences, les statistiques et la liste des
# clones sont perdues ; les fichiers audio de clones/ restent, a declarer de
# nouveau avec `vox clone add`
rm "$CONF/vox.db"
# La base sera recree automatiquement au prochain appel

# Methode 2 : utiliser une base temporaire pour depannage
VOX_DB_PATH=/tmp/vox_test.db vox config show

# Methode 3 : tenter une reparation SQLite
sqlite3 "$CONF/vox.db" ".recover" | sqlite3 "$CONF/vox_recovered.db"
mv "$CONF/vox_recovered.db" "$CONF/vox.db"
```

### Enregistrement micro ne fonctionne pas
La capture utilise cpal (aucun outil externe). Sur macOS, autorisez le micro pour votre terminal
(Reglages > Confidentialite et securite > Microphone). Sur Linux, verifiez que ALSA/PulseAudio voit
le peripherique (`arecord -l`). Si le detecteur de silence coupe trop tot ou trop tard, ajustez
`VOX_VAD_THRESHOLD` (defaut 0.0125).

### "ANTHROPIC_API_KEY environment variable is required for chat mode"
Exportez votre cle API : `export ANTHROPIC_API_KEY=sk-ant-...`

## Choix du backend : guide de performance

Les durees renvoient aux mesures de Backends en detail.

| Usage | Backend | Pourquoi |
|-------|---------|----------|
| Retour vocal court en anglais | `pocket` (defaut) | Sur le CPU, joue pendant qu'il genere : premier son apres 0,18 a 0,55 s |
| Autres langues | `piper` (defaut avec `-l` autre que `en`) | Une voix par langue, sur le CPU : premier son apres 0,75 s environ, 0,26 a 0,32 s avec le daemon |
| Texte long en anglais | `pocket` | Le son part avant la fin de la generation : 0,81 s pour 15 s d'audio |
| Clonage de voix | `qwen-native` | Fonctionne sans jeton ; `pocket` demande `HF_TOKEN` |
| Voix du systeme macOS | `say` | Aucun modele a telecharger |
| Appels repetes en ligne de commande | `pocket` ou `piper` avec le daemon | Le modele n'est charge qu'une fois |
| Machine sans GPU | `pocket` ou `piper` | Ils tournent sur le CPU dans tous les builds |
| Machine avec GPU (build Metal ou CUDA) | `qwen-native`, et Whisper pour `vox hear` | Ce sont les deux seuls usages que le GPU accelere (voir Installation) |
