//! Quick test: pipe text through SentenceAccumulator → TTS streaming.
//! Run with: cargo run --example test_streaming [say|qwen-native]
//! macOS only (uses vox::chat which requires macOS).

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example is macOS-only (requires vox::chat).");
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use std::io::{self, Write};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use vox::chat::sentence::{STREAMING_MIN_CHUNK_CHARS, SentenceAccumulator};

    let backend = std::env::args().nth(1).unwrap_or_else(|| "say".into());

    let response = "Bonjour ! Je suis un assistant vocal conçu pour t'aider \
                    dans tes tâches de développement. N'hésite pas à me poser \
                    des questions, je ferai de mon mieux pour y répondre \
                    de manière claire et concise. Je peux aussi lire du code \
                    et expliquer des concepts techniques si tu en as besoin.";

    eprintln!("=== Test streaming pipeline ({backend}) ===");
    eprintln!("Texte: {response}\n");

    let start = Instant::now();

    let (tx, rx) = mpsc::channel::<Option<String>>();

    let backend_clone = backend.clone();
    let tts_handle = thread::spawn(move || -> anyhow::Result<()> {
        match backend_clone.as_str() {
            "say" => run_say_loop(rx, start),
            "qwen-native" => run_qwen_native_loop(rx, start),
            _ => anyhow::bail!("Unknown backend: {backend_clone}. Use 'say' or 'qwen-native'."),
        }
    });

    // Simulate Claude streaming: token by token, ~30ms each
    let mut accumulator = SentenceAccumulator::with_min_chars(STREAMING_MIN_CHUNK_CHARS);
    for word in response.split_inclusive(' ') {
        eprint!("{word}");
        io::stderr().flush()?;

        for sentence in accumulator.push(word) {
            eprintln!("\n  [ACC] Sentence ready at {:?}", start.elapsed());
            tx.send(Some(sentence))?;
        }
        thread::sleep(Duration::from_millis(30));
    }

    if let Some(remaining) = accumulator.flush() {
        eprintln!("\n  [ACC] Flush at {:?}", start.elapsed());
        tx.send(Some(remaining))?;
    }
    tx.send(None)?;

    tts_handle
        .join()
        .map_err(|_| anyhow::anyhow!("TTS thread panicked"))??;

    eprintln!("  Total time: {:?}", start.elapsed());
    Ok(())
}

#[cfg(target_os = "macos")]
fn run_say_loop(
    rx: std::sync::mpsc::Receiver<Option<String>>,
    start: std::time::Instant,
) -> anyhow::Result<()> {
    use std::process::Command;

    let mut first = true;
    while let Ok(Some(sentence)) = rx.recv() {
        if first {
            eprintln!("  [TTS] First say call at {:?}", start.elapsed());
            first = false;
        }
        let _ = Command::new("/usr/bin/say")
            .arg("-v")
            .arg("Thomas")
            .arg(&sentence)
            .status();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const CROSSFADE_MS: usize = 20;

#[cfg(target_os = "macos")]
fn run_qwen_native_loop(
    rx: std::sync::mpsc::Receiver<Option<String>>,
    start: std::time::Instant,
) -> anyhow::Result<()> {
    use rodio::Sink;
    use rodio::buffer::SamplesBuffer;

    use qwen3_tts::Speaker;
    use vox::backend::qwen_native;

    let (_stream, stream_handle) = rodio::OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let lang = qwen_native::parse_language("fr")?;
    let mut first = true;
    let mut prev_tail: Option<(Vec<f32>, u32)> = None;
    let mut is_first_sentence = true;

    while let Ok(Some(sentence)) = rx.recv() {
        qwen_native::with_model(None, |model| {
            let audio =
                model.synthesize_with_voice(&sentence, Speaker::Ryan, lang, None)?;
            let sr = audio.sample_rate;
            let mut samples = audio.samples;
            let overlap = ((sr as usize * CROSSFADE_MS) / 1000).min(samples.len());

            if first {
                eprintln!("  [TTS] First audio ready at {:?} (TTFA)", start.elapsed());
                first = false;
            }

            // Crossfade with previous sentence's tail.
            if let Some((tail, _)) = prev_tail.take() {
                let n = overlap.min(tail.len()).min(samples.len());
                for i in 0..n {
                    let t = i as f32 / n as f32;
                    let fo = (t * std::f32::consts::FRAC_PI_2).cos();
                    let fi = (t * std::f32::consts::FRAC_PI_2).sin();
                    samples[i] = tail[i] * fo + samples[i] * fi;
                }
            } else if is_first_sentence {
                let n = overlap.min(samples.len());
                for (i, s) in samples[..n].iter_mut().enumerate() {
                    let t = i as f32 / n as f32;
                    *s *= (t * std::f32::consts::FRAC_PI_2).sin();
                }
                is_first_sentence = false;
            }

            // Hold back tail for crossfade with next sentence.
            if samples.len() > overlap {
                let split_at = samples.len() - overlap;
                let tail = samples.split_off(split_at);
                sink.append(SamplesBuffer::new(1, sr, samples));
                prev_tail = Some((tail, sr));
            } else {
                prev_tail = Some((samples, sr));
            }

            Ok(())
        })?;
    }

    if let Some((mut tail, sr)) = prev_tail.take() {
                    let n = tail.len();
                    for (i, s) in tail.iter_mut().enumerate().take(n) {
                        let t = i as f32 / n as f32;
                        *s *= (t * std::f32::consts::FRAC_PI_2).cos();
                    }
        sink.append(SamplesBuffer::new(1, sr, tail));
    }
    sink.sleep_until_end();
    Ok(())
}
