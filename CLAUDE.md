<!-- vox:start -->
## Voice feedback (vox)

This project uses [vox](https://github.com/rtk-ai/vox) for spoken feedback.

After completing a significant task (feature, bug fix, refactor), provide an oral summary:

```bash
vox -l fr "Short summary of what was done, in French"
```

(`-l fr` routes to a backend with proper French voices; `-b say` only exists on macOS.)

Keep summaries under 2 sentences. Use French.
Do not speak for trivial operations (formatting, single-line fixes).
<!-- vox:end -->

## Runtime: Bun
Toujours utiliser `bun` au lieu de `npm`, `npx`, `node`, ou `tsx`.

## Roadmap: Mode conversationnel (inspiré PersonaPlex/Moshi)

Objectif: transformer vox d'un outil TTS one-shot en un assistant vocal conversationnel
intégré à Claude Code / Codex, avec Claude comme cerveau (bien supérieur au 7B de Moshi).

### Architecture cible

```
vox_converse mode:
  Micro → Whisper STT (local) → Claude API (streaming) → TTS streaming (qwen-native) → speakers
  Boucle continue avec VAD pour détection fin de parole
  Modes: push-to-talk | vad | full-duplex
  Latence cible: ~800ms première syllabe
```

### Phases d'implémentation

| Phase | Feature | Description |
|-------|---------|-------------|
| 1 | `vox_hear` | STT local avec Whisper (candle-whisper ou whisper.cpp bindings), VAD pour détection fin de parole |
| 2 | Streaming TTS | Parler pendant la génération Claude — alimenter qwen-native phrase par phrase |
| 3 | `vox_converse` | Boucle conversation complète: hear → Claude API stream → speak stream. Nouveau tool MCP |
| 4 | Personas | `.voxpersona` = voice clone + style + system prompt. `vox persona create "nom"` |

### Références techniques (PersonaPlex)

- **Codec Mimi** (Kyutai, MIT): SEANet + Transformer + RVQ 8 codebooks, 12.5 fps, 24kHz
- **sphn** lib: Opus streaming en Python — référence pour le streaming audio
- **Protocole WebSocket binaire**: 0x01+Opus (audio), 0x02+UTF8 (texte) — inspiration pour vox_converse
- **Voice conditioning**: embeddings .pt pré-calculés — enrichir voice cloning existant
- **VAD pattern**: silence detection pour turn-taking naturel
- **Repo**: https://github.com/NVIDIA/personaplex | Model: nvidia/personaplex-7b-v1

### Contraintes

- Pas de GPU NVIDIA requis — doit tourner sur Apple Silicon (M2 Pro+)
- Garder la compatibilité avec les backends existants (say, qwen, qwen-native)
- Le mode converse est un AJOUT, pas un remplacement des fonctionnalités actuelles
