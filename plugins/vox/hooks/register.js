import {
  PRESETS,
  TONES,
  barsText,
  colorAt,
  describeColors,
  hexOf,
  isStops,
  levels,
  parseColors,
  rasterCells,
  rasterColumns,
  resample,
} from './wave.js'

// Milliseconds between frames: 20 a second, under the band's limit of 30, and
// the pace of the spectrum vox announces
const TICK_MS = 50
// Milliseconds between two looks for vox's announcement, while none is playing
const POLL_MS = 100
// The most bars the band draws, which is the bands vox analyzes, and the
// fewest worth drawing as a grid
const MAX_BARS = 20
const MIN_BARS = 6
// Bars in the short wave drawn beside the spinner
const SPINNER_BARS = 5
// The file vox writes in its config directory while it plays (src/levels.rs)
const ANNOUNCEMENT = 'now-playing.json'
// Frames the last known row stays up while vox is still generating the next
// ones: two seconds, after which the announcement is taken for abandoned
const HOLD_FRAMES = 40
// The store key the speaking gradient is kept under, from one session to the next
const COLORS_KEY = 'colors'
// Seconds the bars show after a change of colors
const COLOR_PREVIEW_SECONDS = 4

// vox subcommands that make no sound and record nothing
const SILENT = new Set([
  'clone', 'config', 'stats', 'setup', 'daemon', 'init', 'serve', 'pack', 'mcp',
  'help', '-h', '--help', '-V', '--version',
])

// What vox may be doing now, the newest last: each is { mode, caption }.
// A mode is 'speak', 'hear', 'preview' (a speak drawn without vox), or
// 'watch' (vox may speak, as from a Stop hook: drawn only once it does).
let running = []
// The frame the next synthetic drawing shows
let frame = 0
// The timer that advances the frame, while something is running
let ticker = null
// Where vox announces what it plays: undefined until looked for, null if unknown
let announcementPath
// The playback vox last announced: { startedMs, frameMs, frames, isComplete }.
// vox announces a playback it is still generating as incomplete, and adds
// frames to it as they come.
let playing = null
// The time at the last tick, and at the last look for an announcement
let now = 0
let polledAt = 0
// The gradient the speaking bars are drawn in, as 0xRRGGBB stops
let speakStops = PRESETS.sunset

// Where vox's config directory is, the way the `dirs` crate finds it
async function findAnnouncement($) {
  const override = await $.env.get('VOX_CONFIG_DIR')
  if (override) return override + '/' + ANNOUNCEMENT
  const appData = await $.env.get('APPDATA')
  if (appData) return appData + '/vox/' + ANNOUNCEMENT
  const home = await $.env.get('HOME')
  if (!home) return null
  const mac = home + '/Library/Application Support'
  if (await $.fs.exists(mac)) return mac + '/vox/' + ANNOUNCEMENT
  const xdg = await $.env.get('XDG_CONFIG_HOME')
  return (xdg || home + '/.config') + '/vox/' + ANNOUNCEMENT
}

// Read what vox is playing, if it is playing and says so
async function poll($) {
  if (announcementPath === undefined) announcementPath = await findAnnouncement($)
  if (!announcementPath) return
  let data
  try {
    data = JSON.parse(await $.fs.read(announcementPath))
  } catch {
    // No file: vox isn't playing, or is a build that doesn't announce. A
    // playback still waiting for frames has ended without them.
    if (playing && !playing.isComplete) playing = null
    return
  }
  if (data.version !== 1 || !Array.isArray(data.frames) || !(data.frame_ms > 0)) return
  playing = {
    startedMs: data.started_ms,
    frameMs: data.frame_ms,
    frames: data.frames,
    // Builds that announce the whole utterance at once don't say so
    isComplete: data.complete !== false,
  }
}

// The spectrum row sounding now, or null when vox isn't playing
function currentRow() {
  if (!playing) return null
  const index = Math.floor((now - playing.startedMs) / playing.frameMs)
  if (index < 0) return null
  const row = playing.frames[index]
  if (row) return row
  // Past the last frame of a playback vox is still generating: hold that
  // frame rather than drop to "preparing" between two updates
  const missing = index - playing.frames.length
  if (!playing.isComplete && playing.frames.length > 0 && missing < HOLD_FRAMES) {
    return playing.frames.at(-1)
  }
  return null
}

// Start animating for one activity
function begin($, activity) {
  running.push(activity)
  if (!ticker) {
    ticker = $.clock.every(TICK_MS, async () => {
      frame += 1
      now = await $.clock.now()
      const isWaitingForFrames = playing !== null && !playing.isComplete
      if ((!currentRow() || isWaitingForFrames) && now - polledAt >= POLL_MS) {
        polledAt = now
        await poll($)
      }
      $.ui.invalidate('ui.render')
    })
  }
  $.ui.invalidate('ui.render')
}

// Stop animating for one activity, and stop the timer after the last one
function end($, activity) {
  running = running.filter(other => other !== activity)
  if (running.length === 0 && ticker) {
    ticker.cancel()
    ticker = null
  }
  $.ui.invalidate('ui.render')
}

// What a shell command does with vox: an activity, or null when it's silent
// or isn't a vox command at all
function voxActivity(command) {
  const call = /(?:^|&&|\|\||[;|(])\s*vox(?:\s+([^;&|]*)|$)/.exec(command ?? '')
  if (!call) return null
  const args = (call[1] ?? '').trim()
  const first = args.split(/\s+/)[0]
  if (SILENT.has(first)) return null
  if (first === 'hear') return { mode: 'hear', caption: '' }
  const quoted = /"([^"]*)"|'([^']*)'/.exec(args)
  return { mode: 'speak', caption: quoted ? (quoted[1] ?? quoted[2]) : '' }
}

// What to draw for one activity in that many bars: { stops, label, heights,
// isDim }, or null when there is nothing to show yet
function scene(activity, bars) {
  if (activity.mode === 'hear') {
    return { stops: TONES.hear, label: 'vox · listening', heights: levels('hear', frame, bars) }
  }
  if (activity.mode === 'preview') {
    return { stops: speakStops, label: 'vox · speaking', heights: levels('speak', frame, bars) }
  }
  const row = currentRow()
  if (row) {
    // The real spectrum of what the speakers are playing now
    const heights = resample(row.map(level => level / 100), bars)
    return { stops: speakStops, label: 'vox · speaking', heights }
  }
  if (activity.mode === 'watch') return null
  // The call has started and the sound hasn't: hooks, model load, synthesis
  return { stops: TONES.wait, label: 'vox · preparing', heights: levels('wait', frame, bars), isDim: true }
}

// Show the synthetic bars for some seconds
function preview($, mode, seconds, caption) {
  const activity = { mode, caption }
  begin($, activity)
  $.clock.after(seconds * 1000, () => end($, activity))
}

// The newest activity with something to show, and what it shows
function drawing(bars) {
  for (let i = running.length - 1; i >= 0; i--) {
    const shown = scene(running[i], bars)
    if (shown) return { ...shown, caption: running[i].caption }
  }
  return null
}

// Cut a caption to a width, on one line
function clip(text, width) {
  const line = text.replace(/\s+/g, ' ').trim()
  if (line.length <= width) return line
  return width > 1 ? line.slice(0, width - 1) + '…' : ''
}

export function register(on) {
  on('session.start', async ($, e, next) => {
    // The colors chosen in an earlier session, when they still read as colors
    const saved = await $.store.get(COLORS_KEY)
    if (isStops(saved)) speakStops = saved
    // A preview that needs no vox binary and no audio device
    await $.command.register({
      name: 'vox-wave',
      description: 'Preview the vox animation, or set its colors',
      argumentHint: '[speak|hear] [seconds] | color [preset | #rrggbb …]',
      immediate: true,
    })
    return next(e)
  })

  on('command.run', { command: 'vox-wave' }, async ($, e) => {
    const words = e.args.trim().split(/\s+/).filter(Boolean)
    if (words[0] === 'color' || words[0] === 'colors') {
      const presets = Object.keys(PRESETS).join(', ')
      if (words.length === 1) {
        return {
          text:
            'Speaking colors: ' + describeColors(speakStops) + '. Presets: ' + presets +
            '. Or give one to three #rrggbb for a gradient, left to right.',
        }
      }
      const stops = parseColors(words.slice(1))
      if (!stops) {
        return {
          text:
            'Not a color: ' + words.slice(1).join(' ') + '. Use a preset (' + presets +
            ') or one to three #rrggbb.',
        }
      }
      speakStops = stops
      await $.store.set(COLORS_KEY, stops)
      preview($, 'preview', COLOR_PREVIEW_SECONDS, 'colors: ' + describeColors(stops))
      return { text: 'Speaking colors set to ' + describeColors(stops) }
    }
    const mode = words.includes('hear') ? 'hear' : 'preview'
    const asked = Number(words.find(word => /^\d+(\.\d+)?$/.test(word)))
    const seconds = Math.min(60, Math.max(1, asked || 5))
    preview($, mode, seconds, 'preview, no audio')
    const name = mode === 'hear' ? 'hear' : 'speak'
    return { text: 'Showing the ' + name + ' animation for ' + seconds + ' s' }
  })

  // The vox MCP tools that use the speakers or the microphone, under whatever
  // name the server was added with
  on('tool.call', { tool: /^mcp__.*__vox_(speak|hear|pack_play)$/ }, async ($, e, next) => {
    const isHearing = e.tool.endsWith('__vox_hear')
    const activity = {
      mode: isHearing ? 'hear' : 'speak',
      caption: typeof e.text === 'string' ? e.text : '',
    }
    begin($, activity)
    try {
      // The permission check and the tool both run inside next
      return await next(e)
    } finally {
      end($, activity)
    }
  })

  // vox run from the shell, as the CLAUDE.md that `vox init` writes asks Claude to do
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const activity = voxActivity(e.command)
    if (!activity) return next(e)
    begin($, activity)
    try {
      return await next(e)
    } finally {
      end($, activity)
    }
  })

  // The Stop hook that `vox init` adds speaks when a turn ends. Nothing says
  // whether one is configured, so watch while the Stop hooks run and draw only
  // if vox announces a playback.
  on('classic.Stop', async ($, e, next) => {
    const activity = { mode: 'watch', caption: '' }
    begin($, activity)
    try {
      return await next(e)
    } finally {
      end($, activity)
    }
  })

  // The band above the prompt: the bars, then what vox is saying
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const theirs = await next(e)
    if (running.length === 0 || e.props.hasSurvey) return theirs

    const columns = e.props.bodyColumns
    const bars = Math.min(MAX_BARS, Math.floor((columns - 20) / 2))
    // The Desktop app has no Raster, and a narrow band has no room for one
    const hasGrid = e.surface === 'terminal' && bars >= MIN_BARS
    const shown = drawing(hasGrid ? bars : Math.max(3, Math.min(12, columns - 20)))
    if (!shown) return theirs

    const { Box, Text, Raster } = $.ui.resolve(e)
    const rows = e.props.maxRows >= 3 ? 2 : 1
    const wave = hasGrid
      ? Raster({
          key: 'vox-wave',
          columns: rasterColumns(bars),
          rows,
          cells: rasterCells(shown.heights, shown.stops, rows),
        })
      : Text({
          // A line of text takes one color: the middle of the gradient
          color: hexOf(colorAt(shown.stops, 0.5)),
          dimColor: shown.isDim === true,
          children: [barsText(shown.heights)],
        })
    const room = columns - (hasGrid ? rasterColumns(bars) : 12) - 2
    const caption = clip(shown.caption, room)
    const words = [Text({ bold: true, children: [shown.label] })]
    if (caption && rows > 1) words.push(Text({ dimColor: true, wrap: 'truncate-end', children: [caption] }))

    const mine = Box({
      flexDirection: 'row',
      columnGap: 2,
      children: [wave, Box({ flexDirection: 'column', children: words })],
    })
    // Keep what the mods after this one draw in the band
    return theirs ? Box({ flexDirection: 'column', children: [mine, theirs] }) : mine
  })

  // The spinner: a short wave after its word, while Claude waits on vox
  on('ui.render', { component: 'Spinner' }, async ($, e, next) => {
    const shown = running.length > 0 ? drawing(SPINNER_BARS) : null
    if (!shown) return next(e)
    const suffix = ' · ' + barsText(shown.heights) + ' ' + shown.label
    return next({ ...e, props: { ...e.props, suffix } })
  })
}
