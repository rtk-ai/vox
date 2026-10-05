//! Cross-platform audio playback via rodio.
//!
//! Supports blocking and async (threaded) playback of WAV/MP3/OGG/FLAC files.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// Apply volume gain to a WAV file in-place by rewriting the sample data.
/// A volume of 1.0 leaves the file unchanged.
pub fn apply_wav_gain(path: &Path, volume: f32) -> Result<()> {
    if (volume - 1.0).abs() <= f32::EPSILON {
        return Ok(());
    }
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("Failed to open WAV for gain: {}", path.display()))?;
    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1i32 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    let gained: Vec<f32> = samples
        .iter()
        .map(|s| (s * volume).clamp(-1.0, 1.0))
        .collect();

    // Always write back as 16-bit signed int
    let out_spec = hound::WavSpec {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, out_spec)
        .with_context(|| format!("Failed to rewrite WAV: {}", path.display()))?;
    for &sample in &gained {
        let scaled = (sample * 32767.0) as i16;
        writer.write_sample(scaled)?;
    }
    writer.finalize()?;

    Ok(())
}

/// Play a WAV file and block until playback finishes.
pub fn play_wav_blocking(path: &Path) -> Result<()> {
    play_audio_blocking(path)
}

/// Where a backend's rendered audio should go: to the speakers, or to a file.
///
/// For a backend that renders a WAV before it plays, saving is just a
/// different destination for the same bytes — no re-encoding, no second
/// render. piper and pocket speak through `Player` instead and come here only
/// to save.
pub fn deliver(rendered: &Path, destination: Option<&Path>) -> Result<()> {
    let Some(out) = destination else {
        return play_wav_blocking(rendered);
    };
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
    }
    // Rename would fail across filesystems (temp dir vs. target), so copy.
    std::fs::copy(rendered, out)
        .with_context(|| format!("Failed to write audio to {}", out.display()))?;
    eprintln!("Saved audio to {}", out.display());
    Ok(())
}

/// Play an audio file (WAV, MP3, OGG, FLAC) and block until playback finishes.
pub fn play_audio_blocking(path: &Path) -> Result<()> {
    let (_stream, stream_handle) =
        rodio::OutputStream::try_default().context("Failed to open audio output device")?;
    crate::timing::mark("audio: output device open");
    let file = File::open(path).context("Failed to open audio file")?;
    let source =
        rodio::Decoder::new(BufReader::new(file)).context("Failed to decode audio file")?;
    let sink = rodio::Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
    // Analyzed before the first sample plays, so a visualizer has the whole
    // spectrum from the moment the sound starts.
    let frames = crate::levels::decode_mono(path)
        .map(|(samples, rate)| crate::levels::spectrum(&samples, rate));
    let _now_playing = frames.as_deref().and_then(crate::levels::NowPlaying::start);
    crate::timing::mark("audio: spectrum analyzed and announced");
    sink.append(source);
    crate::timing::mark("audio: first sample queued");
    sink.sleep_until_end();
    crate::timing::mark("audio: playback finished");
    Ok(())
}

/// Handle for async WAV playback — keeps audio alive until `wait()` or drop.
pub struct PlayHandle {
    join: Option<thread::JoinHandle<Result<()>>>,
}

impl PlayHandle {
    /// Block until playback finishes. Returns any error from the playback thread.
    pub fn wait(mut self) -> Result<()> {
        if let Some(h) = self.join.take() {
            h.join()
                .map_err(|_| anyhow::anyhow!("audio playback thread panicked"))?
        } else {
            Ok(())
        }
    }
}

impl Drop for PlayHandle {
    fn drop(&mut self) {
        // If not explicitly waited on, just let the thread finish in background
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

/// Play a WAV file in a background thread. Returns a handle to wait on.
pub fn play_wav_async(path: &Path) -> Result<PlayHandle> {
    let path = path.to_path_buf();
    let join = thread::spawn(move || play_wav_blocking(&path));
    Ok(PlayHandle { join: Some(join) })
}

// ---------------------------------------------------------------------------
// Playback that starts before the audio exists
// ---------------------------------------------------------------------------

/// How far the announced spectrum may lag behind the audio queued so far.
const ANNOUNCE_EVERY: Duration = Duration::from_millis(200);

/// A playback opened before there is anything to play.
///
/// Opening the output device takes 0.1 to 0.2 s. Started first, on a thread of
/// its own, that wait overlaps with loading the model instead of following the
/// synthesis. Audio is then queued as it comes, so a backend that generates in
/// chunks is heard from its first chunk rather than after its last.
///
/// The device lives on the player's thread because an output stream cannot
/// move between threads on every platform.
pub struct Player {
    chunks: Option<mpsc::Sender<(u32, Vec<f32>)>>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}

impl Player {
    /// Open the output device in the background and get ready to play.
    pub fn start(volume: f32) -> Self {
        let (chunks, queued) = mpsc::channel();
        let thread = thread::spawn(move || play_chunks(queued, volume));
        Self {
            chunks: Some(chunks),
            thread: Some(thread),
        }
    }

    /// Queue mono samples to play after the ones already queued. Returns
    /// false once the player has stopped, which means the device failed:
    /// `finish` says why, and there is no point in generating more.
    pub fn push(&self, sample_rate: u32, samples: Vec<f32>) -> bool {
        self.chunks
            .as_ref()
            .is_some_and(|chunks| chunks.send((sample_rate, samples)).is_ok())
    }

    /// Say that no more audio is coming, and block until all of it has played.
    pub fn finish(mut self) -> Result<()> {
        // Closing the channel is what tells the player the audio is complete.
        self.chunks.take();
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .map_err(|_| anyhow::anyhow!("audio playback thread panicked"))?,
            None => Ok(()),
        }
    }
}

/// The player's thread: play each chunk as it arrives, and keep the
/// visualizer's announcement up to date with what has been queued.
fn play_chunks(queued: mpsc::Receiver<(u32, Vec<f32>)>, volume: f32) -> Result<()> {
    let (_stream, stream_handle) =
        rodio::OutputStream::try_default().context("Failed to open audio output device")?;
    let sink = rodio::Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
    crate::timing::mark("audio: output device open");

    let mut analyzer: Option<crate::levels::Analyzer> = None;
    let mut now_playing: Option<crate::levels::NowPlaying> = None;
    let mut announced = Instant::now();
    let mut is_first = true;

    for (sample_rate, mut samples) in queued {
        if (volume - 1.0).abs() > f32::EPSILON {
            for sample in &mut samples {
                *sample = (*sample * volume).clamp(-1.0, 1.0);
            }
        }
        let analyzer = analyzer.get_or_insert_with(|| crate::levels::Analyzer::new(sample_rate));
        analyzer.feed(&samples);
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
        if is_first {
            now_playing = crate::levels::NowPlaying::begin();
            crate::timing::mark("audio: first sample queued");
        }
        if is_first || announced.elapsed() >= ANNOUNCE_EVERY {
            if let Some(now_playing) = &now_playing {
                now_playing.update(&analyzer.levels(), false);
            }
            announced = Instant::now();
        }
        is_first = false;
    }

    if let (Some(analyzer), Some(now_playing)) = (analyzer.as_mut(), &now_playing) {
        analyzer.finish();
        now_playing.update(&analyzer.levels(), true);
    }
    sink.sleep_until_end();
    crate::timing::mark("audio: playback finished");
    Ok(())
}

// ---------------------------------------------------------------------------
// Listening cues
// ---------------------------------------------------------------------------

/// Short tones that tell the user when vox starts and stops listening.
///
/// A terminal meter is invisible when vox runs as an MCP server or from a
/// Claude Code hook: its output goes through JSON-RPC or into a buffered pipe,
/// never to a live terminal. A sound reaches the user in every one of those
/// cases, which is the whole point of a voice loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Recording has started — speak now.
    Start,
    /// Recording has stopped, transcription is running.
    Stop,
}

impl Cue {
    /// (frequency in Hz, duration in milliseconds)
    pub fn tone(self) -> (f32, u64) {
        match self {
            // Rising, friendly: "go".
            Cue::Start => (880.0, 120),
            // Lower, settled: "got it".
            Cue::Stop => (587.33, 100),
        }
    }
}

/// Whether cues are enabled. Off with `VOX_CUES=0`, on otherwise.
pub fn cues_enabled() -> bool {
    !matches!(
        std::env::var("VOX_CUES").ok().as_deref(),
        Some("0") | Some("false") | Some("off")
    )
}

/// Render a cue as 16 kHz mono samples with a short fade in and out, so it
/// sounds like a chime rather than a click.
pub fn cue_samples(cue: Cue, sample_rate: u32) -> Vec<f32> {
    let (freq, ms) = cue.tone();
    let n = (sample_rate as u64 * ms / 1000) as usize;
    let fade = (n / 8).max(1);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let envelope = if i < fade {
                i as f32 / fade as f32
            } else if i + fade >= n {
                (n - i) as f32 / fade as f32
            } else {
                1.0
            };
            (t * freq * std::f32::consts::TAU).sin() * 0.25 * envelope
        })
        .collect()
}

/// Play a cue. Never fails the caller: a missing audio device must not stop a
/// transcription, so errors are swallowed deliberately.
pub fn play_cue(cue: Cue) {
    if !cues_enabled() {
        return;
    }
    const RATE: u32 = 16_000;
    let samples = cue_samples(cue, RATE);
    let _ = (|| -> Result<()> {
        let (_stream, handle) = rodio::OutputStream::try_default()?;
        let sink = rodio::Sink::try_new(&handle)?;
        sink.append(rodio::buffer::SamplesBuffer::new(1, RATE, samples));
        sink.sleep_until_end();
        Ok(())
    })();
}

#[cfg(test)]
mod deliver_tests {
    use super::*;

    fn tiny_wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..160 {
            w.write_sample((i % 32) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn deliver_writes_the_file_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = dir.path().join("rendered.wav");
        tiny_wav(&rendered);

        // A missing parent directory is created, not an error.
        let out = dir.path().join("nested/dir/out.wav");
        deliver(&rendered, Some(out.as_path())).unwrap();

        assert_eq!(
            std::fs::read(&rendered).unwrap(),
            std::fs::read(&out).unwrap(),
            "saving must not re-encode the rendered audio"
        );
    }

    #[test]
    fn deliver_refuses_an_unwritable_destination_instead_of_playing() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = dir.path().join("rendered.wav");
        tiny_wav(&rendered);
        // A directory as the destination: the copy has to fail loudly.
        assert!(deliver(&rendered, Some(dir.path())).is_err());
    }
}

#[cfg(test)]
mod cue_tests {
    use super::*;

    #[test]
    fn cues_differ_so_start_and_stop_are_distinguishable() {
        assert_ne!(Cue::Start.tone().0, Cue::Stop.tone().0);
    }

    #[test]
    fn cue_samples_have_the_requested_length_and_stay_in_range() {
        for cue in [Cue::Start, Cue::Stop] {
            let (_, ms) = cue.tone();
            let s = cue_samples(cue, 16_000);
            assert_eq!(s.len(), (16_000 * ms / 1000) as usize);
            assert!(s.iter().all(|v| v.abs() <= 1.0));
        }
    }

    #[test]
    fn cue_samples_fade_in_and_out_to_avoid_clicks() {
        let s = cue_samples(Cue::Start, 16_000);
        assert!(s[0].abs() < 0.01, "must start near silence");
        assert!(s[s.len() - 1].abs() < 0.01, "must end near silence");
        let mid = s[s.len() / 2].abs();
        assert!(mid > 0.05, "must be audible in the middle, got {mid}");
    }

    #[test]
    fn cues_can_be_disabled() {
        // Default is on; the env var is read at call time.
        assert!(cues_enabled() || std::env::var("VOX_CUES").is_ok());
    }
}
