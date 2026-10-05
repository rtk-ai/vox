//! What vox is playing right now, as a spectrum a visualizer can follow.
//!
//! vox writes the spectrum of what it plays next to its config when playback
//! starts, and removes it when playback ends. A visualizer (the Claude Code
//! plugin in `plugins/vox`) reads the file and stays in sync from the start
//! time alone: no stream, no socket, nothing to keep open.
//!
//! A backend that renders the whole utterance first announces all of it at
//! once. One that plays while it generates announces what it has, marked
//! incomplete, and rewrites the file as frames come; the start time never
//! moves, so a reader only has to read again to learn more.
//!
//! Everything here is best effort. A visualizer is decoration; a failure to
//! analyze or to write the file must never cost the user their audio.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rodio::Source;
use rustfft::{FftPlanner, num_complex::Complex};
use serde::Serialize;

/// Time between two frames of the spectrum, in milliseconds.
pub const FRAME_MS: u32 = 50;

/// Bars per frame, spaced evenly on a log scale.
pub const BANDS: usize = 20;

/// The spectrum covers the range that carries speech.
const LOW_HZ: f32 = 100.0;
const HIGH_HZ: f32 = 8000.0;

/// How far below the loudest bar of the utterance a bar still shows.
const RANGE_DB: f32 = 45.0;

/// Voice loses about this much per octave; adding it back keeps the upper
/// bars from sitting flat while the lower ones do all the moving.
const TILT_DB_PER_OCTAVE: f32 = 3.0;

/// An utterance whose loudest bar stays under this, relative to a full-scale
/// tone, is silence, not a faint voice to stretch across the whole height.
const SILENCE_DB: f32 = -70.0;

/// Where the current playback is announced.
pub fn path() -> PathBuf {
    crate::config::config_dir().join("now-playing.json")
}

/// Decode an audio file to mono samples and its sample rate.
pub fn decode_mono(path: &Path) -> Option<(Vec<f32>, u32)> {
    let file = File::open(path).ok()?;
    let source = rodio::Decoder::new(BufReader::new(file)).ok()?;
    let rate = source.sample_rate();
    let channels = source.channels().max(1) as usize;
    let interleaved: Vec<f32> = source.convert_samples::<f32>().collect();
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();
    Some((mono, rate))
}

/// The spectrum of an utterance: one row of `BANDS` bars every `FRAME_MS`,
/// each bar from 0 to 100 relative to the loudest bar of the whole utterance.
pub fn spectrum(samples: &[f32], sample_rate: u32) -> Vec<Vec<u8>> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let mut analyzer = Analyzer::new(sample_rate);
    analyzer.feed(samples);
    analyzer.finish();
    analyzer.levels()
}

/// A spectrum computed as the audio arrives, for a backend that plays while it
/// is still generating. Fed all at once it gives what `spectrum` gives.
pub struct Analyzer {
    hop: usize,
    size: usize,
    window: Vec<f32>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    bins: Vec<(usize, usize)>,
    samples: Vec<f32>,
    /// One row per frame analyzed so far, in decibels relative to a full-scale
    /// tone, before any scaling to the loudest bar.
    decibels: Vec<Vec<f32>>,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        // A window of about 40 ms: long enough to separate the harmonics of a
        // voice, short enough that a bar moves with the syllable it belongs to.
        let size = (sample_rate as usize / 25).next_power_of_two().max(64);
        let window = (0..size)
            .map(|i| {
                let phase = 2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        Self {
            hop: (sample_rate as usize * FRAME_MS as usize / 1000).max(1),
            size,
            window,
            fft: FftPlanner::<f32>::new().plan_fft_forward(size),
            bins: band_bins(size, sample_rate),
            samples: Vec::new(),
            decibels: Vec::new(),
        }
    }

    /// Add audio, and analyze every frame whose window is now complete.
    pub fn feed(&mut self, samples: &[f32]) {
        self.samples.extend_from_slice(samples);
        while self.decibels.len() * self.hop + self.size / 2 <= self.samples.len() {
            self.analyze_next();
        }
    }

    /// Analyze the last frames, whose windows hang over the end of the audio.
    pub fn finish(&mut self) {
        while self.decibels.len() < self.samples.len().div_ceil(self.hop) {
            self.analyze_next();
        }
    }

    fn analyze_next(&mut self) {
        // Center the window on the frame's instant.
        let center = self.decibels.len() * self.hop;
        let mut buffer: Vec<Complex<f32>> = (0..self.size)
            .map(|i| {
                let at = (center + i).checked_sub(self.size / 2);
                let sample = at
                    .and_then(|at| self.samples.get(at))
                    .copied()
                    .unwrap_or(0.0);
                Complex::new(sample * self.window[i], 0.0)
            })
            .collect();
        self.fft.process(&mut buffer);

        // A full-scale tone peaks at size / 4 through this window.
        let full_scale = (self.size as f32 / 4.0).powi(2);
        let row = self
            .bins
            .iter()
            .enumerate()
            .map(|(band, &(low, high))| {
                let power = buffer[low..high].iter().map(|c| c.norm_sqr()).sum::<f32>()
                    / (high - low) as f32
                    / full_scale;
                let octaves = band as f32 / BANDS as f32 * (HIGH_HZ / LOW_HZ).log2();
                10.0 * (power + 1e-12).log10() + TILT_DB_PER_OCTAVE * octaves
            })
            .collect();
        self.decibels.push(row);
    }

    /// Every row analyzed so far, each bar from 0 to 100 relative to the
    /// loudest bar so far. Audio that never rises above silence stays flat.
    pub fn levels(&self) -> Vec<Vec<u8>> {
        let loudest = self
            .decibels
            .iter()
            .flatten()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        self.decibels
            .iter()
            .map(|row| {
                row.iter()
                    .map(|db| {
                        if loudest < SILENCE_DB {
                            return 0;
                        }
                        let level = (db - (loudest - RANGE_DB)) / RANGE_DB;
                        (level.clamp(0.0, 1.0) * 100.0).round() as u8
                    })
                    .collect()
            })
            .collect()
    }
}

/// The FFT bins each band averages, as a half-open range that is never empty.
fn band_bins(size: usize, sample_rate: u32) -> Vec<(usize, usize)> {
    let nyquist_bin = size / 2;
    let high_hz = HIGH_HZ.min(sample_rate as f32 / 2.0);
    let bin_of = |hz: f32| (hz * size as f32 / sample_rate as f32).round() as usize;
    (0..BANDS)
        .map(|band| {
            let edge = |i: usize| LOW_HZ * (high_hz / LOW_HZ).powf(i as f32 / BANDS as f32);
            let low = bin_of(edge(band)).clamp(1, nyquist_bin - 1);
            let high = bin_of(edge(band + 1)).clamp(low + 1, nyquist_bin);
            (low, high)
        })
        .collect()
}

#[derive(Serialize)]
struct Announcement<'a> {
    version: u8,
    pid: u32,
    started_ms: u64,
    frame_ms: u32,
    bands: usize,
    /// False while the backend is still generating: more frames will follow.
    complete: bool,
    frames: &'a [Vec<u8>],
}

/// The announcement of one playback: written from `begin`, removed on drop.
pub struct NowPlaying {
    path: PathBuf,
    started_ms: u64,
}

impl NowPlaying {
    /// Announce a whole playback that starts now. `None` when the file cannot
    /// be written: the audio plays all the same, only the visualizer goes
    /// without.
    pub fn start(frames: &[Vec<u8>]) -> Option<Self> {
        let playing = Self::begin_at(&path())?;
        playing.update(frames, true).then_some(playing)
    }

    /// Mark the instant a playback starts, before its frames are known. Call
    /// `update` as they come.
    pub fn begin() -> Option<Self> {
        Self::begin_at(&path())
    }

    fn begin_at(path: &Path) -> Option<Self> {
        let started_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;
        Some(Self {
            path: path.to_path_buf(),
            started_ms,
        })
    }

    /// Write the frames known so far. Returns whether the file was written.
    pub fn update(&self, frames: &[Vec<u8>], complete: bool) -> bool {
        self.write(frames, complete).is_some()
    }

    fn write(&self, frames: &[Vec<u8>], complete: bool) -> Option<()> {
        let announcement = Announcement {
            version: 1,
            pid: std::process::id(),
            started_ms: self.started_ms,
            frame_ms: FRAME_MS,
            bands: BANDS,
            complete,
            frames,
        };
        let json = serde_json::to_string(&announcement).ok()?;
        std::fs::create_dir_all(self.path.parent()?).ok()?;
        // A reader must never see half a file, so write beside it and rename.
        let partial = self
            .path
            .with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&partial, json).ok()?;
        if std::fs::rename(&partial, &self.path).is_err() {
            let _ = std::fs::remove_file(&partial);
            return None;
        }
        Some(())
    }

    /// Whether the file on disk still announces this playback, and not one
    /// another vox process started in the meantime.
    fn is_current(&self) -> bool {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        value["pid"].as_u64() == Some(std::process::id() as u64)
            && value["started_ms"].as_u64() == Some(self.started_ms)
    }
}

impl Drop for NowPlaying {
    fn drop(&mut self) {
        if self.is_current() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, seconds: f32, rate: u32) -> Vec<f32> {
        (0..(seconds * rate as f32) as usize)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    /// The band whose range holds a frequency.
    fn band_of(hz: f32, rate: u32) -> usize {
        let high = HIGH_HZ.min(rate as f32 / 2.0);
        ((hz / LOW_HZ).ln() / (high / LOW_HZ).ln() * BANDS as f32) as usize
    }

    #[test]
    fn spectrum_has_one_row_of_bands_per_frame() {
        let rate = 24_000;
        let frames = spectrum(&sine(440.0, 1.0, rate), rate);
        assert_eq!(frames.len(), 1000 / FRAME_MS as usize);
        assert!(frames.iter().all(|row| row.len() == BANDS));
        assert!(frames.iter().flatten().all(|&level| level <= 100));
    }

    #[test]
    fn a_pure_tone_lights_the_band_that_holds_it() {
        for (hz, rate) in [(440.0, 24_000), (2000.0, 22_050), (300.0, 16_000)] {
            let frames = spectrum(&sine(hz, 0.5, rate), rate);
            let expected = band_of(hz, rate);
            // Skip the edges, where the window hangs over the silence around.
            for row in &frames[2..frames.len() - 2] {
                let loudest = (0..BANDS).max_by_key(|&band| row[band]).unwrap();
                assert!(
                    loudest.abs_diff(expected) <= 1,
                    "{hz} Hz at {rate} Hz: band {loudest}, expected {expected}: {row:?}"
                );
                assert!(row[loudest] >= 90, "{row:?}");
            }
        }
    }

    #[test]
    fn a_louder_passage_draws_taller_bars() {
        let rate = 24_000;
        let mut samples = sine(440.0, 0.5, rate);
        samples.extend(sine(440.0, 0.5, rate).iter().map(|s| s * 0.05));
        let frames = spectrum(&samples, rate);
        let band = band_of(440.0, rate);
        let loud = frames[4][band];
        let soft = frames[15][band];
        assert!(loud > soft + 30, "loud {loud}, soft {soft}");
    }

    #[test]
    fn silence_stays_flat() {
        let frames = spectrum(&vec![0.0; 24_000], 24_000);
        assert_eq!(frames.len(), 20);
        assert!(frames.iter().flatten().all(|&level| level == 0));
        assert!(spectrum(&[], 24_000).is_empty());
    }

    #[test]
    fn audio_fed_in_pieces_gives_the_spectrum_of_the_whole() {
        let rate = 24_000;
        let mut samples = sine(440.0, 0.4, rate);
        samples.extend(sine(1800.0, 0.37, rate).iter().map(|s| s * 0.3));
        let whole = spectrum(&samples, rate);

        // One Mimi frame at a time, as pocket-tts hands them over.
        let mut analyzer = Analyzer::new(rate);
        let mut known = 0;
        for piece in samples.chunks(1920) {
            analyzer.feed(piece);
            let so_far = analyzer.levels().len();
            assert!(so_far >= known, "a frame was taken back");
            known = so_far;
        }
        // Frames arrive while the audio is still coming, not all at the end.
        assert!(known >= whole.len() - 2, "{known} of {}", whole.len());
        analyzer.finish();
        assert_eq!(analyzer.levels(), whole);
    }

    #[test]
    fn an_announcement_grows_while_the_backend_still_generates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("now-playing.json");
        let playing = NowPlaying::begin_at(&path).expect("begin");
        assert!(!path.exists(), "nothing to read before the first frames");

        let read = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        };
        assert!(playing.update(&[vec![1u8; BANDS]], false));
        let first = read();
        assert_eq!(first["complete"], false);
        assert_eq!(first["frames"].as_array().unwrap().len(), 1);

        assert!(playing.update(&[vec![1u8; BANDS], vec![2u8; BANDS]], true));
        let second = read();
        assert_eq!(second["complete"], true);
        assert_eq!(second["frames"].as_array().unwrap().len(), 2);
        // The start does not move: a reader stays in sync across updates.
        assert_eq!(second["started_ms"], first["started_ms"]);
    }

    #[test]
    fn the_announcement_lasts_exactly_as_long_as_the_playback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vox").join("now-playing.json");
        let frames = vec![vec![7u8; BANDS]; 3];

        let playing = NowPlaying::begin_at(&path).expect("announce");
        assert!(playing.update(&frames, true));
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["complete"], true);
        assert_eq!(value["frame_ms"], FRAME_MS);
        assert_eq!(value["bands"], BANDS);
        assert_eq!(value["frames"].as_array().unwrap().len(), 3);
        assert_eq!(value["frames"][0][0], 7);
        assert_eq!(value["pid"], std::process::id());

        drop(playing);
        assert!(!path.exists(), "the announcement outlived the playback");
        // Nothing half-written is left beside it either.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            0
        );
    }

    #[test]
    fn a_playback_never_removes_another_one_s_announcement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("now-playing.json");
        let first = NowPlaying::begin_at(&path).expect("announce");
        assert!(first.update(&[], true));
        // Another vox process takes over the file while the first still plays.
        std::fs::write(&path, r#"{"pid":1,"started_ms":1,"frames":[]}"#).unwrap();
        drop(first);
        assert!(
            path.exists(),
            "removed an announcement that was not its own"
        );
    }
}
