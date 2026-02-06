use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::thread;

use anyhow::{Context, Result};

/// Play a WAV file and block until playback finishes.
pub fn play_wav_blocking(path: &Path) -> Result<()> {
    let (_stream, stream_handle) =
        rodio::OutputStream::try_default().context("Failed to open audio output device")?;
    let file = File::open(path).context("Failed to open WAV file")?;
    let source = rodio::Decoder::new(BufReader::new(file)).context("Failed to decode WAV file")?;
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
