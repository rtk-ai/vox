# vox plugin for Claude Code

A [mod](https://code.claude.com/docs/en/plugins/mods/overview) that draws a
voice visualizer above the prompt while vox speaks, and a short one beside the
spinner.

While vox plays, the bars are the real spectrum of the audio: vox analyzes each
utterance before playing it and announces it in `now-playing.json`, in its
config directory, for as long as the sound lasts (`src/levels.rs`). The mod
reads that file and follows it from the start time.

- `vox · preparing`: Claude has called vox and no sound has started yet
- `vox · speaking`: the spectrum of what the speakers are playing now
- `vox · listening`: `vox hear` is recording. These bars are synthetic.

It reacts to the vox MCP tools (`vox_speak`, `vox_hear`, `vox_pack_play`), to
`vox ...` run from the shell, and to a Stop hook that speaks.

Install it in a Claude Code session:

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
```

Needs Claude Code 2.1.287 or later, and a vox build that announces its
playback. Tested with Claude Code 2.1.289. The `say` backend plays outside vox
and announces nothing, so it stays on `preparing`.

The speaking bars are drawn in a gradient you choose, kept from one session to
the next. The terminal rounds each color to its own palette.

```bash
/vox-wave color                      # the current colors and the presets
/vox-wave color ocean                # a preset: sunset, ocean, forest, fire, violet, rainbow, mono
/vox-wave color #00ff00 #0000ff      # one to three stops, left to right; one is a solid color
```

```bash
claude --plugin-dir plugins/vox      # from a checkout: load it for one session
/vox-wave [speak|hear] [seconds]     # synthetic preview, no vox or audio needed

claude plugin validate plugins/vox
cd plugins/vox && claude plugin test
```
