// Drawing for the vox visualizer. Pure functions, so a frame is reproducible
// and the drawing can be tested without a session.
//
// The bars come from one of two places. While vox plays, they are the real
// spectrum vox announced (see src/levels.rs). Otherwise they are synthetic:
// they show that vox is busy, not the level of any audio.

// The value a Raster reads as "the terminal's default color"
const DEFAULT_COLOR = 0x01000000

// A cell filled from the bottom, in eighths: index 0 is empty, 8 is full
const BLOCKS = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']

// Gradients for the speaking bars, as 0xRRGGBB stops from left to right
export const PRESETS = {
  sunset: [0xff8a00, 0xff3d81],
  ocean: [0x00c2ff, 0x3d5afe],
  forest: [0xb2ff59, 0x00c853],
  fire: [0xffd600, 0xff6d00, 0xdd2c00],
  violet: [0xb388ff, 0xff4081],
  rainbow: [0xff5252, 0xffd740, 0x40c4ff],
  mono: [0xe0e0e0],
}

// The gradients vox's other states are drawn in
export const TONES = {
  hear: [0x00c2ff, 0x3ddc84],
  wait: [0x6b7280, 0x9ca3af],
}

// The most stops a gradient typed by hand may have
const MAX_STOPS = 3

// Read colors as typed: one preset name, or one to three #rrggbb stops.
// Returns the stops, or null when the words are neither.
export function parseColors(words) {
  if (words.length === 1 && Object.hasOwn(PRESETS, words[0].toLowerCase())) {
    return PRESETS[words[0].toLowerCase()]
  }
  if (words.length < 1 || words.length > MAX_STOPS) return null
  const stops = []
  for (const word of words) {
    const hex = /^#?([0-9a-f]{6})$/i.exec(word)
    if (!hex) return null
    stops.push(parseInt(hex[1], 16))
  }
  return stops
}

// Whether a value is a list of stops, as one read back from the store must be
export function isStops(value) {
  return (
    Array.isArray(value) &&
    value.length >= 1 &&
    value.length <= MAX_STOPS &&
    value.every(stop => Number.isInteger(stop) && stop >= 0 && stop <= 0xffffff)
  )
}

// Stops as text, by preset name when they are one: 'sunset', or '#ff8a00 #ff3d81'
export function describeColors(stops) {
  for (const [name, preset] of Object.entries(PRESETS)) {
    if (preset.length === stops.length && preset.every((stop, i) => stop === stops[i])) return name
  }
  return stops.map(hexOf).join(' ')
}

// A color as the #rrggbb a Text takes
export function hexOf(color) {
  return '#' + color.toString(16).padStart(6, '0')
}

// Synthetic bar heights, each from 0 to 1, for a kind of motion at a frame
export function levels(mode, frame, bars) {
  const out = []
  for (let i = 0; i < bars; i++) {
    if (mode === 'hear') out.push(hearLevel(frame, i, bars))
    else if (mode === 'wait') out.push(waitLevel(frame, i, bars))
    else out.push(speakLevel(frame, i, bars))
  }
  return out
}

// Speech: two beating sines under a syllable-like envelope, louder in the middle
function speakLevel(frame, i, bars) {
  const t = frame * 0.35
  const envelope = 0.6 + 0.4 * Math.sin(t * 0.9)
  const middle = 0.45 + 0.55 * Math.sin((Math.PI * (i + 0.5)) / bars)
  const ripple = Math.abs(Math.sin(i * 0.55 + t) * Math.sin(i * 0.21 - t * 0.6))
  return clamp(envelope * middle * (0.25 + 0.75 * ripple))
}

// Listening: a low ripple that travels outward from the middle
function hearLevel(frame, i, bars) {
  const t = frame * 0.18
  const distance = Math.abs(i - (bars - 1) / 2)
  const wave = Math.max(0, Math.sin(distance * 0.7 - t))
  const falloff = 1 - distance / bars
  return clamp(0.12 + 0.6 * wave * falloff)
}

// Waiting for the sound: a flat line with one small bump that walks across
function waitLevel(frame, i, bars) {
  const at = (frame * 0.4) % (bars + 6) - 3
  const distance = Math.abs(i - at)
  return clamp(0.07 + 0.2 * Math.max(0, 1 - distance / 3))
}

function clamp(value) {
  return Math.min(1, Math.max(0, value))
}

// Fit a row of heights to a number of bars, by averaging the ones that merge
// and repeating the ones that spread
export function resample(heights, bars) {
  if (heights.length === bars) return heights
  const out = []
  for (let i = 0; i < bars; i++) {
    const from = Math.floor((i * heights.length) / bars)
    const to = Math.max(from + 1, Math.floor(((i + 1) * heights.length) / bars))
    let sum = 0
    for (let j = from; j < to; j++) sum += heights[j]
    out.push(sum / (to - from))
  }
  return out
}

// The color at a point of a gradient, from 0 at its left end to 1 at its right
export function colorAt(stops, ratio) {
  if (stops.length === 1) return stops[0]
  const scaled = Math.min(1, Math.max(0, ratio)) * (stops.length - 1)
  const index = Math.min(stops.length - 2, Math.floor(scaled))
  const within = scaled - index
  const channel = shift => {
    const a = (stops[index] >> shift) & 0xff
    const b = (stops[index + 1] >> shift) & 0xff
    return Math.round(a + (b - a) * within)
  }
  return (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

// The color of bar i, interpolated across a gradient
export function barColor(stops, i, bars) {
  return colorAt(stops, bars > 1 ? i / (bars - 1) : 0)
}

// How many terminal columns a Raster of that many bars takes: a gap between bars
export function rasterColumns(bars) {
  return bars * 2 - 1
}

// The character of one bar at one row, counting rows from the bottom
function blockAt(level, rows, rowFromBottom) {
  const eighths = Math.round(level * rows * 8)
  return BLOCKS[Math.min(8, Math.max(0, eighths - rowFromBottom * 8))]
}

// Pack bar heights into the base64 string a Raster takes: row-major cells,
// each three little-endian u32s (code point, foreground, background)
export function rasterCells(heights, stops, rows) {
  const bars = heights.length
  const columns = rasterColumns(bars)
  const words = new Uint32Array(columns * rows * 3)
  let at = 0
  for (let row = 0; row < rows; row++) {
    for (let column = 0; column < columns; column++) {
      const bar = column / 2
      // Odd columns are the gaps between bars
      const char = column % 2 === 0 ? blockAt(heights[bar], rows, rows - 1 - row) : ' '
      words[at++] = char.codePointAt(0)
      words[at++] = column % 2 === 0 ? barColor(stops, bar, bars) : DEFAULT_COLOR
      words[at++] = DEFAULT_COLOR
    }
  }
  return new Uint8Array(words.buffer).toBase64()
}

// The same heights as one line of block characters, for where a Raster can't
// draw. A bar never goes fully empty, so the line keeps its width.
export function barsText(heights) {
  return heights.map(level => blockAt(Math.max(level, 0.125), 1, 0)).join('')
}
