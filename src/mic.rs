//! Microphone capture via cpal (cross-platform, no sox dependency).
//!
//! Records from the default input device, downmixes to mono, resamples to
//! 16 kHz (what Whisper expects) and optionally stops on silence using a
//! simple energy-based voice activity detector.

use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Sample rate of the audio returned by [`record`].
pub const TARGET_RATE: u32 = 16_000;

/// Frame size used by the VAD, in milliseconds.
pub const VAD_FRAME_MS: usize = 50;

/// Default RMS threshold above which a frame counts as speech (~ -38 dBFS).
pub const DEFAULT_VAD_THRESHOLD: f32 = 0.0125;

/// Frames used to estimate the ambient noise floor before speech starts.
pub const NOISE_FLOOR_FRAMES: usize = 6; // 300 ms at 50 ms/frame

/// The adaptive threshold is `noise_floor * NOISE_FLOOR_FACTOR`, never below
/// the configured threshold and never above [`MAX_VAD_THRESHOLD`].
pub const NOISE_FLOOR_FACTOR: f32 = 3.0;
pub const MAX_VAD_THRESHOLD: f32 = 0.1;

/// Effective VAD threshold given the RMS of the first frames (ambient noise).
pub fn adaptive_threshold(base: f32, first_frames: &[f32]) -> f32 {
    if first_frames.is_empty() {
        return base;
    }
    let floor = first_frames.iter().sum::<f32>() / first_frames.len() as f32;
    (floor * NOISE_FLOOR_FACTOR).clamp(base, MAX_VAD_THRESHOLD)
}

/// When to stop recording.
#[derive(Debug, Clone, PartialEq)]
pub enum StopWhen {
    /// Record for a fixed number of seconds.
    Duration(f64),
    /// Record until the user presses Enter on stdin.
    Enter,
    /// Start on the first voiced frame (or after `max_wait` seconds), then
    /// stop after `silence` seconds without voice, or at `timeout` seconds.
    Silence {
        silence: f64,
        timeout: f64,
        max_wait: f64,
    },
}

#[derive(Debug, Clone)]
pub struct RecordOptions {
    pub stop: StopWhen,
    /// RMS threshold for the VAD (only used with [`StopWhen::Silence`]).
    pub vad_threshold: f32,
}

impl RecordOptions {
    pub fn until_silence(silence: f64, timeout: f64) -> Self {
        Self {
            stop: StopWhen::Silence {
                silence,
                timeout,
                max_wait: timeout,
            },
            vad_threshold: vad_threshold_from_env(),
        }
    }

    pub fn until_enter() -> Self {
        Self {
            stop: StopWhen::Enter,
            vad_threshold: vad_threshold_from_env(),
        }
    }

    pub fn for_duration(secs: f64) -> Self {
        Self {
            stop: StopWhen::Duration(secs),
            vad_threshold: vad_threshold_from_env(),
        }
    }
}

/// `VOX_VAD_THRESHOLD` lets users tune the silence detector for noisy rooms.
pub fn vad_threshold_from_env() -> f32 {
    std::env::var("VOX_VAD_THRESHOLD")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| *v > 0.0 && *v < 1.0)
        .unwrap_or(DEFAULT_VAD_THRESHOLD)
}

pub fn mic_install_hint() -> &'static str {
    if cfg!(target_os = "linux") {
        "No input device found. Check that ALSA/PulseAudio sees your microphone (arecord -l)."
    } else if cfg!(target_os = "macos") {
        "No input device found. Grant microphone access to your terminal in System Settings > Privacy & Security > Microphone."
    } else {
        "No input device found. Check your microphone settings."
    }
}

/// Root-mean-square energy of a frame.
pub fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// Downmix interleaved multi-channel samples to mono by averaging.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Resample mono audio with a box low-pass followed by linear interpolation.
/// Good enough for speech recognition; not meant for music.
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    // Anti-alias when downsampling: average over `ratio` input samples.
    let filtered: Vec<f32> = if ratio > 1.0 {
        let win = ratio.ceil() as usize;
        let half = win / 2;
        (0..samples.len())
            .map(|i| {
                let lo = i.saturating_sub(half);
                let hi = usize::min(samples.len(), i + half + 1);
                samples[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            })
            .collect()
    } else {
        samples.to_vec()
    };
    let out_len = ((samples.len() as f64) / ratio).round() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = filtered[usize::min(idx, filtered.len() - 1)];
            let b = filtered[usize::min(idx + 1, filtered.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

/// Given the RMS of successive frames, decide where speech starts and ends.
/// Returns `(first_voiced_frame, stop_frame)` where `stop_frame` is set once
/// `silence_frames` consecutive unvoiced frames follow speech.
pub fn speech_bounds(
    frame_rms: &[f32],
    threshold: f32,
    silence_frames: usize,
) -> (Option<usize>, Option<usize>) {
    let mut start = None;
    let mut quiet = 0usize;
    for (i, &e) in frame_rms.iter().enumerate() {
        let voiced = e >= threshold;
        match start {
            None => {
                if voiced {
                    start = Some(i);
                }
            }
            Some(_) => {
                if voiced {
                    quiet = 0;
                } else {
                    quiet += 1;
                    if quiet >= silence_frames {
                        return (start, Some(i + 1));
                    }
                }
            }
        }
    }
    (start, None)
}

/// Write 16 kHz mono samples as a 16-bit PCM WAV file.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    for s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Read any WAV file and return 16 kHz mono f32 samples.
pub fn read_wav_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let mono = downmix(&samples, spec.channels as usize);
    Ok(resample(&mono, spec.sample_rate, TARGET_RATE))
}

struct Capture {
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    _stream: cpal::Stream,
}

fn start_capture(errored: Arc<AtomicBool>) -> Result<Capture> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("{}", mic_install_hint()))?;
    let config = device
        .default_input_config()
        .context("Failed to query default input config")?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = Arc::clone(&buf);
    let err_flag = Arc::clone(&errored);
    let err_fn = move |e: cpal::StreamError| {
        eprintln!("Microphone stream error: {e}");
        err_flag.store(true, Ordering::SeqCst);
    };

    macro_rules! build {
        ($t:ty, $conv:expr) => {
            device.build_input_stream(
                &stream_config,
                move |data: &[$t], _| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|f| f.iter().map(|s| $conv(*s)).sum::<f32>() / f.len() as f32)
                        .collect();
                    if let Ok(mut b) = sink.lock() {
                        b.extend_from_slice(&mono);
                    }
                },
                err_fn,
                None,
            )
        };
    }

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::U16 => build!(u16, |s: u16| (s as f32 / u16::MAX as f32) * 2.0 - 1.0),
        cpal::SampleFormat::I32 => build!(i32, |s: i32| s as f32 / i32::MAX as f32),
        other => anyhow::bail!("Unsupported microphone sample format: {other:?}"),
    }
    .context("Failed to open microphone stream")?;
    stream.play().context("Failed to start microphone stream")?;

    Ok(Capture {
        buf,
        rate,
        _stream: stream,
    })
}

/// Record from the default microphone. Returns 16 kHz mono samples.
pub fn record(opts: &RecordOptions) -> Result<Vec<f32>> {
    let (samples, rate) = record_native(opts)?;
    Ok(resample(&samples, rate, TARGET_RATE))
}

/// Record from the default microphone at the device's native sample rate.
/// Returns `(mono samples, sample_rate)`.
pub fn record_native(opts: &RecordOptions) -> Result<(Vec<f32>, u32)> {
    let errored = Arc::new(AtomicBool::new(false));
    let cap = start_capture(Arc::clone(&errored))?;
    let started = Instant::now();
    let poll = Duration::from_millis(VAD_FRAME_MS as u64);

    match &opts.stop {
        StopWhen::Duration(secs) => {
            while started.elapsed().as_secs_f64() < *secs && !errored.load(Ordering::SeqCst) {
                std::thread::sleep(poll);
            }
        }
        StopWhen::Enter => {
            let stdin = std::io::stdin();
            let mut line = String::new();
            stdin.lock().read_line(&mut line)?;
        }
        StopWhen::Silence {
            silence,
            timeout,
            max_wait,
        } => {
            let frame_len = (cap.rate as usize * VAD_FRAME_MS) / 1000;
            let silence_frames = ((silence * 1000.0) / VAD_FRAME_MS as f64).ceil() as usize;
            let debug = std::env::var_os("VOX_VAD_DEBUG").is_some();
            let mut frame_rms: Vec<f32> = Vec::new();
            let mut analysed = 0usize;
            let mut threshold = opts.vad_threshold;
            loop {
                std::thread::sleep(poll);
                if errored.load(Ordering::SeqCst) {
                    break;
                }
                {
                    let b = cap.buf.lock().map_err(|_| anyhow!("mic buffer poisoned"))?;
                    while analysed + frame_len <= b.len() {
                        frame_rms.push(rms(&b[analysed..analysed + frame_len]));
                        analysed += frame_len;
                    }
                }
                if frame_rms.len() >= NOISE_FLOOR_FRAMES {
                    threshold =
                        adaptive_threshold(opts.vad_threshold, &frame_rms[..NOISE_FLOOR_FRAMES]);
                }
                let (start, stop) = speech_bounds(&frame_rms, threshold, silence_frames);
                let elapsed = started.elapsed().as_secs_f64();
                if stop.is_some() || elapsed >= *timeout {
                    break;
                }
                if start.is_none() && elapsed >= *max_wait {
                    break;
                }
            }
            let raw = cap.buf.lock().map_err(|_| anyhow!("mic buffer poisoned"))?;
            let (start, stop) = speech_bounds(&frame_rms, threshold, silence_frames);
            if debug {
                let max = frame_rms.iter().cloned().fold(0.0f32, f32::max);
                let mean = frame_rms.iter().sum::<f32>() / frame_rms.len().max(1) as f32;
                eprintln!(
                    "[vad] rate={} frames={} rms mean={mean:.4} max={max:.4} threshold={threshold:.4} start={start:?} stop={stop:?}",
                    cap.rate,
                    frame_rms.len()
                );
            }
            let Some(start) = start else {
                return Ok((Vec::new(), cap.rate));
            };
            // Keep a little context before the first voiced frame.
            let lo = start.saturating_sub(4) * frame_len;
            let hi = stop
                .map(|f| f * frame_len)
                .unwrap_or(raw.len())
                .min(raw.len());
            return Ok((raw[lo..hi].to_vec(), cap.rate));
        }
    }

    let raw = cap.buf.lock().map_err(|_| anyhow!("mic buffer poisoned"))?;
    Ok((raw.clone(), cap.rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn rms_of_full_scale_square_is_one() {
        let frame: Vec<f32> = (0..100)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!((rms(&frame) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resample_halves_length_for_2x_downsample() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample(&input, 32_000, 16_000);
        assert_eq!(out.len(), 500);
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn resample_identity_when_rates_match() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }

    #[test]
    fn speech_bounds_detects_start_and_stop() {
        let quiet = 0.001;
        let loud = 0.1;
        let frames = [quiet, quiet, loud, loud, loud, quiet, quiet, quiet, loud];
        let (start, stop) = speech_bounds(&frames, 0.01, 3);
        assert_eq!(start, Some(2));
        assert_eq!(stop, Some(8));
    }

    #[test]
    fn speech_bounds_no_speech() {
        let frames = [0.001; 20];
        assert_eq!(speech_bounds(&frames, 0.01, 3), (None, None));
    }

    #[test]
    fn speech_bounds_still_talking() {
        let frames = [0.001, 0.1, 0.1, 0.001, 0.1];
        assert_eq!(speech_bounds(&frames, 0.01, 3), (Some(1), None));
    }

    #[test]
    fn wav_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        write_wav(&path, &samples, TARGET_RATE).unwrap();
        let back = read_wav_16k(&path).unwrap();
        assert_eq!(back.len(), samples.len());
        let max_err = samples
            .iter()
            .zip(&back)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_err < 1e-3, "max_err={max_err}");
    }

    #[test]
    fn adaptive_threshold_tracks_noise_floor() {
        assert_eq!(adaptive_threshold(0.01, &[]), 0.01);
        // Quiet room: stays at the base threshold.
        assert_eq!(adaptive_threshold(0.01, &[0.001; 6]), 0.01);
        // Noisy room: 3x the floor.
        assert!((adaptive_threshold(0.01, &[0.02; 6]) - 0.06).abs() < 1e-6);
        // Never above the cap.
        assert_eq!(adaptive_threshold(0.01, &[0.5; 6]), MAX_VAD_THRESHOLD);
    }

    #[test]
    fn env_threshold_fallback() {
        assert_eq!(vad_threshold_from_env(), DEFAULT_VAD_THRESHOLD);
    }
}
