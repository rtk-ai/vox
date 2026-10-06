# Mesures de performance

Temps mesures en octobre 2026 avec les binaires de la release v0.17.0, sur
macOS, Linux et Windows. Le resume est dans le
[README](../README.md#generation-time) ; cette page donne le detail, la methode
et les limites.

## Ce qui est mesure

Chaque chiffre est le temps d'une commande `vox -o fichier.wav "..."` entiere,
du lancement du processus a la fin de l'ecriture du fichier : demarrage,
chargement du modele depuis le disque, synthese, ecriture. Rien n'est joue, donc
le chiffre ne depend pas d'un peripherique audio et se compare d'une plateforme
a l'autre.

- Un nouveau processus a chaque commande : le modele est recharge a chaque fois.
  Le daemon et le serveur MCP gardent le modele charge et sont plus rapides.
- Les modeles sont deja telecharges. Un premier appel, non compte, les
  telecharge et les charge une fois.
- Chaque chiffre est la mediane de 5 commandes (3 pour `qwen-native`). Le
  minimum et le maximum sont donnes dans le detail.
- La configuration et la base de donnees sont dans un repertoire vide
  (`VOX_CONFIG_DIR`, `VOX_DB_PATH`), sans preference enregistree.

## Machines

| Nom | Machine | Systeme | Build |
|-----|---------|---------|-------|
| macOS | Portable Apple A18 Pro, 8 Go, sur secteur, mode economie d'energie desactive | macOS 27.0 | `vox-aarch64-apple-darwin` (Metal) |
| Linux CPU | Intel Core i7-8700 (6 coeurs, 12 fils), 23 Go visibles | Ubuntu 26.04 sous WSL2 | `vox-x86_64-unknown-linux-gnu` |
| Linux CUDA | La meme, avec une RTX 4070 Ti SUPER (16 Go), pilote 591.86 | Ubuntu 26.04 sous WSL2 | `vox-x86_64-unknown-linux-gnu-cuda` |
| Windows | La meme, 32 Go | Windows 11 | `vox-x86_64-pc-windows-msvc` (CPU, le seul build Windows) |

Sous Windows, les commandes ont ete lancees depuis WSL. Ce lancement ajoute
46 a 51 ms par commande (mesure sur `vox --version`), compris dans les chiffres.

## Resultats

Mediane, en secondes.

| Commande | macOS | Linux CPU | Linux CUDA | Windows |
|----------|------:|----------:|-----------:|--------:|
| `pocket`, anglais, 1 phrase | 0,79 | 3,66 | 3,77 | 2,50 |
| `pocket`, anglais, 4 phrases | 3,33 | 13,48 | 13,31 | 9,51 |
| `piper`, francais, 1 phrase | 0,56 | 1,19 | 1,19 | 1,56 |
| `piper`, francais, 4 phrases | 1,59 | 2,22 | 2,18 | 2,53 |
| `qwen-native`, voix clonee, 1 phrase | 31,35 | 55,99 | 8,72 | 46,07 |
| Whisper `base`, 13,8 s d'audio | 1,72 | 15,05 | 1,07 | 5,55 |

### Detail

Minimum et maximum des commandes comptees, et duree de l'audio produit par la
commande mediane. `pocket` et `qwen-native` ne produisent pas deux fois le meme
audio pour le meme texte : la duree varie d'une commande a l'autre.

| Commande | Machine | Mediane | Min | Max | Audio produit |
|----------|---------|--------:|----:|----:|--------------:|
| `pocket`, 1 phrase | macOS | 0,79 | 0,73 | 0,94 | 2,96 s |
| | Linux CPU | 3,66 | 3,08 | 4,00 | 2,48 s |
| | Linux CUDA | 3,77 | 3,41 | 4,87 | 3,20 s |
| | Windows | 2,50 | 2,33 | 2,69 | 2,72 s |
| `pocket`, 4 phrases | macOS | 3,33 | 3,14 | 3,48 | 13,76 s |
| | Linux CPU | 13,48 | 12,96 | 13,75 | 13,84 s |
| | Linux CUDA | 13,31 | 13,13 | 14,21 | 13,68 s |
| | Windows | 9,51 | 9,30 | 10,15 | 13,84 s |
| `piper`, 1 phrase | macOS | 0,56 | 0,52 | 0,57 | 2,61 s |
| | Linux CPU | 1,19 | 1,13 | 1,27 | 2,53 s |
| | Linux CUDA | 1,19 | 1,16 | 1,25 | 2,54 s |
| | Windows | 1,56 | 1,49 | 1,62 | 2,61 s |
| `piper`, 4 phrases | macOS | 1,59 | 1,56 | 1,69 | 13,06 s |
| | Linux CPU | 2,22 | 2,10 | 3,59 | 13,28 s |
| | Linux CUDA | 2,18 | 2,15 | 2,23 | 13,12 s |
| | Windows | 2,53 | 2,50 | 2,68 | 13,02 s |
| `qwen-native`, 1 phrase | macOS | 31,35 | 27,72 | 37,14 | 2,32 s |
| | Linux CPU | 55,99 | 55,64 | 56,25 | 2,40 s |
| | Linux CUDA | 8,72 | 8,20 | 8,94 | 2,48 s |
| | Windows | 46,07 | 45,62 | 47,59 | 2,64 s |
| Whisper `base` | macOS | 1,72 | 1,67 | 1,83 | |
| | Linux CPU | 15,05 | 14,55 | 15,06 | |
| | Linux CUDA | 1,07 | 1,03 | 1,10 | |
| | Windows | 5,55 | 5,48 | 5,68 | |

Whisper a rendu le texte attendu sur les quatre configurations, a un mot pres
avec le build CPU de Linux ("a merge" au lieu de "the merge").

### Premier appel

Le premier appel, non compte dans les medianes, telecharge le modele quand il
manque, puis le charge pour la premiere fois. Sa duree depend donc de la
connexion. Pour `qwen-native` et Whisper, ou il pese le plus :

| Commande | macOS | Linux CPU | Linux CUDA | Windows |
|----------|------:|----------:|-----------:|--------:|
| `qwen-native` | 29,7 s (modele deja sur disque) | 83,3 s (avec telechargement) | 9,4 s (modele deja sur disque) | 97,8 s (avec telechargement) |
| Whisper `base` | 10,5 s | 15,7 s | 1,5 s | 6,6 s |

Sur le build CUDA installe pour la premiere fois sur une machine, le tout
premier appel de Whisper avait pris 15,5 s lors d'un essai precedent.

## Ce que montrent les chiffres

- `pocket` et `piper` prennent le meme temps avec le build CPU et le build
  CUDA : ils tournent sur le CPU dans les deux.
- Le build CUDA accelere `qwen-native` 6 fois et Whisper 14 fois par rapport au
  build CPU, sur cette machine.
- Sur cette machine Linux, `pocket` genere a peu pres aussi vite qu'il joue
  (13,5 s pour 13,8 s d'audio). Sur le Mac, il genere quatre fois plus vite.
  Comme il joue pendant qu'il genere, le son commence bien avant la fin des
  temps ci-dessus.
- Sur le meme processeur, le build CPU est plus lent sous WSL2 que le build
  Windows pour `pocket`, `qwen-native` et Whisper, et plus rapide pour `piper`.
  La cause n'a pas ete cherchee.

## Temps jusqu'au premier son

Ces chiffres sont differents des precedents : ils vont du lancement de la
commande au premier echantillon envoye au peripherique audio, avec une vraie
lecture. Ils ont ete pris les 4 et 5 octobre 2026, sur le meme portable Apple
A18 Pro, alors en mode economie d'energie, avec le build Metal, pour une phrase
de 3 secondes.

| Backend | Sans daemon | Avec daemon |
|---------|------------:|------------:|
| `pocket` (anglais) | 0,18 a 0,55 s | non mesure |
| `piper` (francais) | environ 0,75 s | 0,26 a 0,32 s |

Un texte anglais long (15 s d'audio) commence a etre joue apres 0,81 s.

Le premier son n'a pas ete mesure sous Linux ni sous Windows : la machine de
test est jointe par ssh, sans peripherique audio.

## Refaire les mesures

`VOX_TIMINGS=1` ecrit sur stderr le temps de chaque etape d'une commande.

```bash
VOX_TIMINGS=1 vox -o phrase.wav "The build is finished and all the tests pass."
VOX_TIMINGS=1 vox -l fr -o phrase.wav "Le build est fini et tous les tests passent."

# Voix clonee : une reference de 13,8 s a 24 kHz, avec sa transcription
vox clone add bench --audio reference.wav --text "..."
vox -v bench -o clone.wav "The build is finished and all the tests pass."

# Whisper
vox hear --file reference.wav
```

Textes utilises :

- 1 phrase, anglais : "The build is finished and all the tests pass."
- 4 phrases, anglais : "The build is finished and all the tests pass. The
  code review is open and waiting for your approval. There are three remarks
  about error handling, one about the documentation, and a question about the
  log format. Nothing blocks the merge for now."
- 1 phrase, francais : "Le build est fini et tous les tests passent."
- 4 phrases, francais : "La compilation est finie et tous les tests passent.
  La revue du code est ouverte et attend ta validation. Il reste trois
  remarques sur la gestion des erreurs, une sur la documentation, et une
  question sur le format du journal. Rien ne bloque la fusion pour le moment."

L'audio donne a Whisper et la reference de la voix clonee sont le fichier
produit par `pocket` pour les 4 phrases en anglais.

## Ce qui n'a pas ete mesure

- Linux sur ARM64.
- `say` et `kokoro`. Le README garde pour eux des chiffres plus anciens.
- Le daemon et le serveur MCP, hors des deux chiffres de `piper` ci-dessus.
- Les autres modeles Whisper que `base`.
- Une carte NVIDIA autre que la RTX 4070 Ti SUPER, et un Mac autre que
  l'A18 Pro.
