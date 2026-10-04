import { expect, mock, test } from 'claude-code/testing'
import { PRESETS, TONES, barColor, barsText, describeColors, levels, parseColors, rasterCells, rasterColumns, resample } from '../hooks/wave.js'

// What Claude Code passes to a ui.render hook for the band, apart from the app
const BAND = {
  plugin: 'vox',
  component: 'AbovePrompt',
  viewport: { columns: 120, rows: 40 },
  props: {
    hasSurvey: false,
    isWorking: true,
    maxRows: 10,
    bodyColumns: 120,
    scroll: { offset: 0, bodyRows: 10 },
    view: {},
  },
} as const

// Stands for what Claude Code and later mods draw at a site
const THEIRS = { type: 'Text', props: {}, children: ['drawn by Claude Code'] }

// The time the tests start at, in milliseconds since the epoch
const T0 = 1_700_000_000_000
// The bars the band draws at BAND's width, and the bands vox announces
const BARS = 20

// One row of a spectrum: a single tall band, so two rows never look alike
function row(tall: number): number[] {
  return Array.from({ length: BARS }, (_, band) => (band === tall ? 100 : 10))
}

// What vox writes while it plays (src/levels.rs)
function announcement(startedMs: number, frames: number[][]): string {
  return JSON.stringify({ version: 1, pid: 4242, started_ms: startedMs, frame_ms: 50, bands: BARS, frames })
}

// The cells the band draws for one announced row
function cellsOf(frame: number[]): string {
  return rasterCells(frame.map(level => level / 100), PRESETS.sunset, 2)
}

// Stub the clock, vox's config directory, and the announcement file, which
// holds whatever `file.text` is: nothing, until a test sets it
function voxWorld(on) {
  const clock = mock.clock(on, { now: T0 })
  mock.env(on, { VOX_CONFIG_DIR: '/vox' })
  const file: { text?: string; reads: string[] } = { reads: [] }
  on('fs.read', ($, e) => {
    file.reads.push(e.path)
    return file.text === undefined ? { deny: 'no such file' } : { value: file.text }
  })
  return { clock, file }
}

test('cells pack twelve bytes each, and heights fit any number of bars', async () => {
  const heights = levels('speak', 0, 10)
  const cells = rasterCells(heights, PRESETS.sunset, 2)
  expect(Uint8Array.fromBase64(cells).length).toBe(rasterColumns(10) * 2 * 12)
  expect(rasterCells(levels('speak', 1, 10), PRESETS.sunset, 2)).not.toBe(cells)
  expect(rasterCells(heights, TONES.hear, 2)).not.toBe(cells)
  for (const mode of ['speak', 'hear', 'wait']) {
    for (let frame = 0; frame < 200; frame++) {
      for (const level of levels(mode, frame, 10)) {
        expect(level >= 0 && level <= 1).toBe(true)
      }
    }
  }
  expect(barsText(levels('speak', 3, 5)).length).toBe(5)
  // Merging averages, spreading repeats, and the same count is left alone
  expect(resample([0, 1, 0, 1], 2)).toEqual([0.5, 0.5])
  expect(resample([0.2, 0.8], 4)).toEqual([0.2, 0.2, 0.8, 0.8])
  expect(resample([0.1, 0.2, 0.3], 3)).toEqual([0.1, 0.2, 0.3])
})

test('the band waits for the sound, then follows the spectrum vox announces', async ($, on) => {
  const { clock, file } = voxWorld(on)
  on('ui.render', () => THEIRS)
  // A vox call that takes five seconds to return
  on('tool.call', async () => {
    await clock.sleep(5000)
    return { result: 'ok' }
  })

  const call = $.tool.call({ tool: 'Bash', command: 'vox -l fr "Bonjour tout le monde"' })
  await clock.settle()

  // The call has started and vox has announced nothing: hooks, model, synthesis
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: 'vox · preparing' })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: 'Bonjour tout le monde' })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: 'drawn by Claude Code' })).toBeDefined()

  // vox starts playing 100 ms from now: three frames, 150 ms of sound
  const frames = [row(2), row(9), row(17)]
  file.text = announcement(T0 + 100, frames)

  await clock.advance(100)
  expect(file.reads.at(-1)).toBe('/vox/now-playing.json')
  expect(await ui.find({ type: 'Text', text: 'vox · speaking' })).toBeDefined()
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(cellsOf(frames[0]))
  await clock.advance(50)
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(cellsOf(frames[1]))
  await clock.advance(50)
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(cellsOf(frames[2]))

  // The sound is over and the call isn't: no bars left over from the last frame
  await clock.advance(50)
  expect(await ui.find({ type: 'Text', text: 'vox · preparing' })).toBeDefined()

  await clock.advance(5000)
  await call
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  expect(await ui.find({ type: 'Text', text: 'drawn by Claude Code' })).toBeDefined()
  await ui.unmount()
})

test('an announcement left by a vox that died is not played back', async ($, on) => {
  const { clock, file } = voxWorld(on)
  on('ui.render', () => THEIRS)
  on('tool.call', async () => {
    await clock.sleep(1000)
    return { result: 'ok' }
  })
  // Written a minute ago, never removed
  file.text = announcement(T0 - 60_000, [row(2), row(9)])

  const call = $.tool.call({ tool: 'Bash', command: 'vox "Hello"' })
  await clock.settle()
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await clock.advance(300)
  expect(file.reads.length > 0).toBe(true)
  expect(await ui.find({ type: 'Text', text: 'vox · preparing' })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: 'vox · speaking' })).toBeUndefined()
  await clock.advance(1000)
  await call
  await ui.unmount()
})

test('a Stop hook that speaks is drawn, and one that does not draws nothing', async ($, on) => {
  const { clock, file } = voxWorld(on)
  on('ui.render', () => THEIRS)
  // The Stop hooks in the settings files take two seconds to run
  on('classic.Stop', async () => {
    await clock.sleep(2000)
    return {}
  })

  const stop = $.classic.Stop({ stop_hook_active: false })
  await clock.settle()
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await clock.advance(200)
  // No vox so far: the band holds only what the others drew
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  expect(await ui.find({ type: 'Text', text: /^vox · / })).toBeUndefined()

  const frames = [row(5), row(6), row(7), row(8)]
  file.text = announcement(T0 + 300, frames)
  await clock.advance(100)
  expect(await ui.find({ type: 'Text', text: 'vox · speaking' })).toBeDefined()
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(cellsOf(frames[0]))

  await clock.advance(2000)
  await stop
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()
  await ui.unmount()
})

test('a shell command that makes no sound draws nothing', async ($, on) => {
  voxWorld(on)
  on('ui.render', () => THEIRS)
  // Mount inside the call, which is when the band would show
  const seen: unknown[] = []
  on('tool.call', async () => {
    const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
    seen.push(await ui.find({ type: 'Raster' }))
    await ui.unmount()
    return { result: 'ok' }
  })

  for (const command of ['ls -la', 'voxel build', 'vox config show', 'vox --version', 'cargo test && echo vox']) {
    await $.tool.call({ tool: 'Bash', command })
  }
  expect(seen).toEqual([undefined, undefined, undefined, undefined, undefined])
})

test('the vox MCP tools are drawn under any server name', async ($, on) => {
  voxWorld(on)
  on('ui.render', () => THEIRS)
  const labels: unknown[] = []
  on('tool.call', async () => {
    const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
    const text = await ui.find({ type: 'Text', text: /^vox · / })
    labels.push(text?.children[0])
    await ui.unmount()
    return { result: 'ok' }
  })

  await $.tool.call({ tool: 'mcp__vox__vox_speak', text: 'Hello' })
  await $.tool.call({ tool: 'mcp__plugin_vox_vox__vox_hear', timeout: 5 })
  await $.tool.call({ tool: 'mcp__vox__vox_stats' })
  expect(labels).toEqual(['vox · preparing', 'vox · listening', undefined])
})

test('/vox-wave previews for the seconds asked, in the terminal and the Desktop app', async ($, on) => {
  const { clock } = voxWorld(on)
  on('ui.render', () => THEIRS)

  const answer = await $.command.run({ command: 'vox-wave', args: 'hear 3' })
  expect(answer.text).toBe('Showing the hear animation for 3 s')

  const terminal = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await terminal.find({ type: 'Raster' })).toBeDefined()
  expect(await terminal.find({ type: 'Text', text: 'vox · listening' })).toBeDefined()

  // The Desktop app has no Raster, so the bars are a line of text there
  const desktop = await $.ui.mount({ ...BAND, surface: 'desktop' })
  expect(await desktop.find({ type: 'Raster' })).toBeUndefined()
  expect(await desktop.find({ type: 'Text', text: 'vox · listening' })).toBeDefined()
  await desktop.unmount()

  // A band too narrow for the grid falls back to text as well
  const narrow = await $.ui.mount({
    ...BAND,
    surface: 'terminal',
    props: { ...BAND.props, bodyColumns: 28 },
  })
  expect(await narrow.find({ type: 'Raster' })).toBeUndefined()
  expect(await narrow.find({ type: 'Text', text: 'vox · listening' })).toBeDefined()
  await narrow.unmount()

  // The preview moves by itself, with no vox to announce anything
  const before = (await terminal.find({ type: 'Raster' })).props.cells
  await clock.advance(50)
  expect((await terminal.find({ type: 'Raster' })).props.cells).not.toBe(before)

  await clock.advance(3000)
  expect(await terminal.find({ type: 'Raster' })).toBeUndefined()
  await terminal.unmount()
})

test('the spinner carries a short wave while vox runs', async ($, on) => {
  voxWorld(on)
  // Record the suffix the mod passed on to Claude Code's spinner
  const suffixes: string[] = []
  on('ui.render', ($, e) => {
    if (e.component === 'Spinner') suffixes.push(e.props.suffix)
    return THEIRS
  })
  const SPINNER = {
    plugin: 'vox',
    component: 'Spinner',
    viewport: { columns: 120, rows: 40 },
    props: { word: 'Thinking', message: null, suffix: '…', mode: 'tool-use' },
    surface: 'terminal',
  } as const

  const idle = await $.ui.mount(SPINNER)
  await idle.unmount()
  await $.command.run({ command: 'vox-wave', args: '' })
  const busy = await $.ui.mount(SPINNER)
  await busy.unmount()

  expect(suffixes[0]).toBe('…')
  expect(suffixes.at(-1)).toMatch(/^ · [▁▂▃▄▅▆▇█]{5} vox · speaking$/)
})

test('colors read as a preset or as one to three hex stops, and blend between stops', async () => {
  expect(parseColors(['ocean'])).toEqual(PRESETS.ocean)
  expect(parseColors(['Fire'])).toEqual(PRESETS.fire)
  expect(parseColors(['#ff0000'])).toEqual([0xff0000])
  expect(parseColors(['00ff00', '#0000FF'])).toEqual([0x00ff00, 0x0000ff])
  for (const bad of [[], ['teal'], ['#ff00'], ['#ff0000', 'ocean'], ['#111111', '#222222', '#333333', '#444444']]) {
    expect(parseColors(bad)).toBe(null)
  }
  expect(describeColors(PRESETS.sunset)).toBe('sunset')
  expect(describeColors([0x00ff00, 0x0000ff])).toBe('#00ff00 #0000ff')

  // Two stops: the ends are the stops, the middle is halfway
  expect(barColor([0x000000, 0xffffff], 0, 5)).toBe(0x000000)
  expect(barColor([0x000000, 0xffffff], 2, 5)).toBe(0x808080)
  expect(barColor([0x000000, 0xffffff], 4, 5)).toBe(0xffffff)
  // Three stops: the middle bar is the middle stop
  expect(barColor([0xff0000, 0x00ff00, 0x0000ff], 2, 5)).toBe(0x00ff00)
  expect(barColor([0xff0000, 0x00ff00, 0x0000ff], 1, 5)).toBe(0x808000)
  // One stop is a solid color, and one bar takes the left end
  expect(barColor([0x123456], 3, 5)).toBe(0x123456)
  expect(barColor([0xff0000, 0x0000ff], 0, 1)).toBe(0xff0000)
})

test('/vox-wave color sets the gradient, saves it, and previews it', async ($, on) => {
  const { clock } = voxWorld(on)
  on('ui.render', () => THEIRS)
  const saved = new Map<string, unknown>()
  on('store.set', ($, e) => {
    saved.set(e.key, e.value)
    return { value: undefined }
  })

  const listing = await $.command.run({ command: 'vox-wave', args: 'color' })
  expect(listing.text).toMatch(/^Speaking colors: sunset\. Presets: sunset, ocean, /)

  const refused = await $.command.run({ command: 'vox-wave', args: 'color teal' })
  expect(refused.text).toMatch(/^Not a color: teal\./)
  expect(saved.size).toBe(0)

  const answer = await $.command.run({ command: 'vox-wave', args: 'color #00ff00 #0000ff' })
  expect(answer.text).toBe('Speaking colors set to #00ff00 #0000ff')
  expect(saved.get('colors')).toEqual([0x00ff00, 0x0000ff])

  // The preview that follows is drawn in the new gradient
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(
    rasterCells(levels('speak', 0, BARS), [0x00ff00, 0x0000ff], 2),
  )
  expect(await ui.find({ type: 'Text', text: 'colors: #00ff00 #0000ff' })).toBeDefined()

  // A line of text, as in the Desktop app, takes the middle of the gradient
  const desktop = await $.ui.mount({ ...BAND, surface: 'desktop' })
  expect((await desktop.find({ type: 'Text', text: /^[▁▂▃▄▅▆▇█]+$/ })).props.color).toBe('#008080')
  await desktop.unmount()

  await clock.advance(4000)
  expect(await ui.find({ type: 'Raster' })).toBeUndefined()

  const preset = await $.command.run({ command: 'vox-wave', args: 'color fire' })
  expect(preset.text).toBe('Speaking colors set to fire')
  expect(saved.get('colors')).toEqual(PRESETS.fire)
  await ui.unmount()
})

test('the colors saved in an earlier session come back at session start', async ($, on) => {
  voxWorld(on)
  on('ui.render', () => THEIRS)
  on('store.get', ($, e) => ({ value: e.key === 'colors' ? [0x112233, 0x445566] : undefined }))
  on('command.register', () => ({ value: undefined }))
  on('session.start', () => ({ cwd: '/work' }))

  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' })
  const listing = await $.command.run({ command: 'vox-wave', args: 'color' })
  expect(listing.text).toMatch(/^Speaking colors: #112233 #445566\./)

  await $.command.run({ command: 'vox-wave', args: '' })
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await ui.find({ type: 'Raster' })).props.cells).toBe(
    rasterCells(levels('speak', 0, BARS), [0x112233, 0x445566], 2),
  )
  await ui.unmount()
})
