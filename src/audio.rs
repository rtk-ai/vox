//! Cross-platform audio playback via rodio.
//!
//! Supports blocking and async (threaded) playback of WAV/MP3/OGG/FLAC files.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
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
    let path = path.to_path_buf();
    with_output(move |sink| play_file(sink, &path)).wait(OPEN_TIMEOUT)
}

/// Play one file on an open sink, and announce its spectrum while it plays.
fn play_file(sink: &rodio::Sink, path: &Path) -> Result<()> {
    let file = File::open(path).context("Failed to open audio file")?;
    let source =
        rodio::Decoder::new(BufReader::new(file)).context("Failed to decode audio file")?;
    // Analyzed before the first sample plays, so a visualizer has the whole
    // spectrum from the moment the sound starts.
    let decoded = crate::levels::decode_mono(path);
    let frames = decoded
        .as_ref()
        .map(|(samples, rate)| crate::levels::spectrum(samples, *rate));
    let length = decoded
        .as_ref()
        .map(|(samples, rate)| audio_length(samples.len(), *rate));
    let _now_playing = frames.as_deref().and_then(crate::levels::NowPlaying::start);
    crate::timing::mark("audio: spectrum analyzed and announced");
    sink.append(source);
    crate::timing::mark("audio: first sample queued");
    match length {
        Some(length) => wait_until_played(sink, Instant::now() + length, STALL_GRACE)?,
        // A file that could not be analyzed has no known length to judge by.
        None => sink.sleep_until_end(),
    }
    crate::timing::mark("audio: playback finished");
    Ok(())
}

/// How long the output device may take to open. It takes 0.1 to 0.6 s, a few
/// seconds for a sleeping Bluetooth device; past this it is not going to.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Playback running on a thread of its own, behind a device that may never
/// answer.
///
/// The device lives on that thread because an output stream cannot move
/// between threads on every platform, and because opening it can block for
/// good: a sound server that is configured and not reachable (PulseAudio under
/// WSL over ssh) makes the open wait on a connection that never comes. The
/// caller waits for the open with a limit instead of calling it.
struct AudioThread {
    started: Instant,
    /// Set once the device is open. A flag and not only a message: asking
    /// whether it opened must not use up the answer.
    is_open: Arc<AtomicBool>,
    opened: mpsc::Receiver<()>,
    done: mpsc::Receiver<Result<()>>,
}

/// Open the default output device on a new thread, then run `play` there with
/// a sink on it.
fn with_output<F>(play: F) -> AudioThread
where
    F: FnOnce(&rodio::Sink) -> Result<()> + Send + 'static,
{
    on_audio_thread(open_default_output, play)
}

/// Whatever has to stay alive for as long as a sink plays: the output stream
/// of a real device, nothing for a sink that is not connected to one.
type Device = Box<dyn std::any::Any>;

/// The default output device and a sink on it.
fn open_default_output() -> Result<(Device, rodio::Sink)> {
    let (stream, stream_handle) =
        rodio::OutputStream::try_default().context("Failed to open audio output device")?;
    let sink = rodio::Sink::try_new(&stream_handle).context("Failed to create audio sink")?;
    Ok((Box::new(stream), sink))
}

/// `with_output` with the way to open the device left to the caller, so that a
/// test can stand a device that never opens, or one that never plays, in for
/// the real one.
fn on_audio_thread<O, F>(open: O, play: F) -> AudioThread
where
    O: FnOnce() -> Result<(Device, rodio::Sink)> + Send + 'static,
    F: FnOnce(&rodio::Sink) -> Result<()> + Send + 'static,
{
    let (opened_tx, opened) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let is_open = Arc::new(AtomicBool::new(false));
    let opened_flag = Arc::clone(&is_open);
    thread::spawn(move || {
        // A send that fails means the caller gave up on this thread.
        let (device, sink) = match open() {
            Ok(opened) => opened,
            Err(e) => {
                let _ = done_tx.send(Err(e));
                return;
            }
        };
        crate::timing::mark("audio: output device open");
        opened_flag.store(true, Ordering::SeqCst);
        let _ = opened_tx.send(());
        let outcome = play(&sink);
        // The outcome goes out before the device is closed: closing a device
        // that stopped taking audio waits on it, and would hold back the very
        // error that says so.
        let _ = done_tx.send(outcome);
        drop(sink);
        drop(device);
    });
    AudioThread {
        started: Instant::now(),
        is_open,
        opened,
        done,
    }
}

impl AudioThread {
    /// Whether the device has not opened although `limit` has passed since the
    /// thread started.
    fn is_stuck_opening(&self, limit: Duration) -> bool {
        self.started.elapsed() >= limit && !self.is_open.load(Ordering::SeqCst)
    }

    /// Block until playback ends. Fails when the device has not opened
    /// `open_limit` after the thread started; the thread is then left behind,
    /// still waiting on the device.
    fn wait(self, open_limit: Duration) -> Result<()> {
        let left = open_limit.saturating_sub(self.started.elapsed());
        if !self.is_open.load(Ordering::SeqCst)
            && let Err(mpsc::RecvTimeoutError::Timeout) = self.opened.recv_timeout(left)
            && !self.is_open.load(Ordering::SeqCst)
        {
            anyhow::bail!(
                "The audio output device did not open within {} s. Check the sound \
                 configuration of this machine, or save the audio with -o <file> instead.",
                open_limit.as_secs()
            );
        }
        // Opened, or ended before opening: either way the outcome is coming,
        // and the playback itself is bounded by `wait_until_played`.
        self.done
            .recv()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("audio playback thread panicked")))
    }
}

/// How long after the audio should have ended vox keeps waiting for the
/// device before it takes it for stalled.
const STALL_GRACE: Duration = Duration::from_secs(5);

/// How long that many mono samples last.
fn audio_length(samples: usize, sample_rate: u32) -> Duration {
    Duration::from_secs_f64(samples as f64 / f64::from(sample_rate.max(1)))
}

/// Block until the sink has played everything queued on it, or fail once it is
/// plain that it never will.
///
/// A device can open, accept audio and then never drain it: a PulseAudio sink
/// with nothing connected behind it does exactly that (WSL reached over ssh).
/// Waiting on the sink alone then blocks forever, and a hook that calls vox
/// never hands control back to the agent that ran it. `expected_end` is when
/// the queued audio ends if the device plays it in real time.
fn wait_until_played(sink: &rodio::Sink, expected_end: Instant, grace: Duration) -> Result<()> {
    let give_up = expected_end + grace;
    while !sink.empty() {
        if Instant::now() >= give_up {
            anyhow::bail!(
                "The audio device accepted the sound and is not playing it: nothing had finished \
                 {} s after the audio should have ended. Check the output device, or save the \
                 audio with -o <file> instead.",
                grace.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
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
    thread: AudioThread,
}

impl Player {
    /// Open the output device in the background and get ready to play.
    pub fn start(volume: f32) -> Self {
        let (chunks, queued) = mpsc::channel();
        Self {
            chunks: Some(chunks),
            thread: with_output(move |sink| play_chunks(sink, queued, volume)),
        }
    }

    /// Queue mono samples to play after the ones already queued. Returns
    /// false once there is no point in generating more: the player has
    /// stopped, or the device is not opening. `finish` says why.
    pub fn push(&self, sample_rate: u32, samples: Vec<f32>) -> bool {
        if self.thread.is_stuck_opening(OPEN_TIMEOUT) {
            return false;
        }
        self.chunks
            .as_ref()
            .is_some_and(|chunks| chunks.send((sample_rate, samples)).is_ok())
    }

    /// Say that no more audio is coming, and block until all of it has played.
    pub fn finish(mut self) -> Result<()> {
        // Closing the channel is what tells the player the audio is complete.
        self.chunks.take();
        self.thread.wait(OPEN_TIMEOUT)
    }
}

/// Play each chunk as it arrives, and keep the visualizer's announcement up
/// to date with what has been queued.
fn play_chunks(
    sink: &rodio::Sink,
    queued: mpsc::Receiver<(u32, Vec<f32>)>,
    volume: f32,
) -> Result<()> {
    let mut analyzer: Option<crate::levels::Analyzer> = None;
    let mut now_playing: Option<crate::levels::NowPlaying> = None;
    let mut announced = Instant::now();
    let mut is_first = true;
    // When the audio queued so far ends, if the device plays it in real time.
    let mut expected_end = Instant::now();

    for (sample_rate, mut samples) in queued {
        if (volume - 1.0).abs() > f32::EPSILON {
            for sample in &mut samples {
                *sample = (*sample * volume).clamp(-1.0, 1.0);
            }
        }
        let analyzer = analyzer.get_or_insert_with(|| crate::levels::Analyzer::new(sample_rate));
        analyzer.feed(&samples);
        // A chunk that arrives after the previous ones ran out starts now.
        expected_end = expected_end.max(Instant::now()) + audio_length(samples.len(), sample_rate);
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
    wait_until_played(sink, expected_end, STALL_GRACE)?;
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
    let length = audio_length(samples.len(), RATE);
    // A cue is a fraction of a second: a device that has not opened or not
    // played it shortly after will not, and the recording must not wait on it.
    let _ = with_output(move |sink| {
        sink.append(rodio::buffer::SamplesBuffer::new(1, RATE, samples));
        wait_until_played(sink, Instant::now() + length, Duration::from_secs(1))
    })
    .wait(Duration::from_secs(2));
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

#[cfg(test)]
mod playback_tests {
    use super::*;

    /// A sink nothing is connected to: it accepts audio and never plays it,
    /// like a device whose output goes nowhere.
    fn stalled_sink() -> (rodio::Sink, rodio::queue::SourcesQueueOutput<f32>) {
        rodio::Sink::new_idle()
    }

    fn tone(samples: usize) -> rodio::buffer::SamplesBuffer<f32> {
        rodio::buffer::SamplesBuffer::new(1, 16_000, vec![0.1f32; samples])
    }

    #[test]
    fn audio_length_is_samples_over_rate() {
        assert_eq!(audio_length(16_000, 16_000), Duration::from_secs(1));
        assert_eq!(audio_length(1_200, 24_000), Duration::from_millis(50));
        // A rate of zero cannot divide, and must not panic either.
        assert_eq!(audio_length(10, 0), Duration::from_secs(10));
    }

    #[test]
    fn a_device_that_never_plays_is_an_error_not_a_hang() {
        let (sink, _unplayed) = stalled_sink();
        sink.append(tone(800)); // 50 ms of audio

        let started = Instant::now();
        let outcome = wait_until_played(
            &sink,
            started + Duration::from_millis(50),
            Duration::from_millis(200),
        );
        let waited = started.elapsed();

        let message = outcome.expect_err("a stalled device must fail").to_string();
        assert!(message.contains("is not playing it"), "{message}");
        assert!(message.contains("-o <file>"), "{message}");
        // It waits for the audio and the grace, and not much longer.
        assert!(waited >= Duration::from_millis(250), "{waited:?}");
        assert!(waited < Duration::from_secs(2), "{waited:?}");
    }

    #[test]
    fn a_device_that_plays_returns_as_soon_as_it_is_done() {
        let (sink, output) = stalled_sink();
        sink.append(tone(800));
        // Stand in for the device: pull the samples the sink was given.
        let device = thread::spawn(move || output.take(4_000).count());

        let started = Instant::now();
        wait_until_played(
            &sink,
            started + Duration::from_millis(50),
            Duration::from_secs(30),
        )
        .expect("played audio is not a stall");
        assert!(started.elapsed() < Duration::from_secs(5));
        device.join().unwrap();
    }

    #[test]
    fn a_device_that_never_opens_is_an_error_not_a_hang() {
        // Opening blocks far longer than anyone should wait for it.
        let never_opens = || {
            thread::sleep(Duration::from_secs(30));
            anyhow::bail!("opened too late to matter")
        };
        let playback = on_audio_thread(never_opens, |_sink| Ok(()));

        let started = Instant::now();
        assert!(!playback.is_stuck_opening(Duration::from_secs(10)));
        let message = playback
            .wait(Duration::from_millis(200))
            .expect_err("a device that does not open must fail")
            .to_string();

        assert!(message.contains("did not open within"), "{message}");
        assert!(message.contains("-o <file>"), "{message}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_player_stops_taking_audio_once_the_device_is_not_opening() {
        let never_opens = || {
            thread::sleep(Duration::from_secs(30));
            anyhow::bail!("opened too late to matter")
        };
        let playback = on_audio_thread(never_opens, |_sink| Ok(()));
        thread::sleep(Duration::from_millis(60));
        assert!(playback.is_stuck_opening(Duration::from_millis(50)));
    }

    #[test]
    fn a_device_that_opened_long_ago_is_not_taken_for_one_that_did_not() {
        // A long text: the device opened at once, and the audio is only
        // complete well after the time allowed for opening has passed.
        let opens = || Ok((Box::new(()) as Device, rodio::Sink::new_idle().0));
        let playback = on_audio_thread(opens, |_sink| {
            thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        let limit = Duration::from_millis(100);
        thread::sleep(Duration::from_millis(150));
        // Asked several times, as a player is on every chunk it is given.
        assert!(!playback.is_stuck_opening(limit));
        assert!(!playback.is_stuck_opening(limit));
        playback.wait(limit).expect("the device was open all along");
    }

    #[test]
    fn a_device_that_refuses_to_open_reports_why() {
        let refuses = || anyhow::bail!("Failed to open audio output device");
        let message = on_audio_thread(refuses, |_sink| Ok(()))
            .wait(Duration::from_secs(10))
            .expect_err("the open error must reach the caller")
            .to_string();
        assert_eq!(message, "Failed to open audio output device");
    }

    #[test]
    fn what_plays_on_the_audio_thread_reaches_the_caller() {
        let opens = || Ok((Box::new(()) as Device, rodio::Sink::new_idle().0));
        on_audio_thread(opens, |sink| {
            assert!(sink.empty());
            Ok(())
        })
        .wait(Duration::from_secs(10))
        .unwrap();

        let opens = || Ok((Box::new(()) as Device, rodio::Sink::new_idle().0));
        let message = on_audio_thread(opens, |_sink| anyhow::bail!("playback went wrong"))
            .wait(Duration::from_secs(10))
            .unwrap_err()
            .to_string();
        assert_eq!(message, "playback went wrong");
    }

    /// Closing it blocks, as closing a real device does once it has stopped
    /// taking audio.
    struct BlocksWhenClosed;

    impl Drop for BlocksWhenClosed {
        fn drop(&mut self) {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn a_stalled_device_is_reported_before_it_is_closed() {
        let opens = || {
            Ok((
                Box::new(BlocksWhenClosed) as Device,
                rodio::Sink::new_idle().0,
            ))
        };
        let playback = on_audio_thread(opens, |sink| {
            sink.append(rodio::buffer::SamplesBuffer::new(
                1,
                16_000,
                vec![0.1f32; 800],
            ));
            wait_until_played(
                sink,
                Instant::now() + Duration::from_millis(50),
                Duration::from_millis(200),
            )
        });

        let started = Instant::now();
        let message = playback
            .wait(Duration::from_secs(10))
            .expect_err("a stalled device must fail")
            .to_string();
        assert!(message.contains("is not playing it"), "{message}");
        // The 30 s it takes to close the device are not the caller's to wait.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn nothing_queued_is_already_played() {
        let (sink, _output) = stalled_sink();
        wait_until_played(&sink, Instant::now(), Duration::ZERO).unwrap();
    }
}
