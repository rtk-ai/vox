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
    let spec = reader.spec().clone();

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
