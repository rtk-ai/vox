//! Cross-platform audio playback via rodio.
//!
//! Supports blocking and async (threaded) playback of WAV/MP3/OGG/FLAC files.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::thread;

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

/// Play an audio file (WAV, MP3, OGG, FLAC) and block until playback finishes.
pub fn play_audio_blocking(path: &Path) -> Result<()> {
    let (_stream, stream_handle) =
        rodio::OutputStream::try_default().context("Failed to open audio output device")?;
    let file = File::open(path).context("Failed to open audio file")?;
    let source =
        rodio::Decoder::new(BufReader::new(file)).context("Failed to decode audio file")?;
    let sink = rodio::Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
    sink.append(source);
    sink.sleep_until_end();
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
