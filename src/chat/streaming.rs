use std::io::{self, Write};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};

use super::claude_api::{StreamEvent, stream_claude};
use super::sentence::{STREAMING_MIN_CHUNK_CHARS, SentenceAccumulator};
use super::{ChatConfig, Message, is_exit, record_until_enter, speak_text};
use crate::backend::qwen_native;
use crate::stt;

enum TtsCommand {
    Speak(String),
    Done,
}

/// Crossfade duration in milliseconds between consecutive TTS sentences.
/// 20ms is enough to eliminate clicks without audible blending artifacts.
const CROSSFADE_MS: usize = 20;

/// Apply cosine fade-in to the first `n` samples.
fn fade_in(samples: &mut [f32], n: usize) {
    let n = n.min(samples.len());
    for (i, sample) in samples[..n].iter_mut().enumerate() {
        let t = i as f32 / n as f32;
        *sample *= (t * std::f32::consts::FRAC_PI_2).sin();
    }
}

/// Apply cosine fade-out to the last `n` samples.
fn fade_out(samples: &mut [f32], n: usize) {
    let len = samples.len();
    let n = n.min(len);
    for i in 0..n {
        let t = i as f32 / n as f32;
        samples[len - n + i] *= (t * std::f32::consts::FRAC_PI_2).cos();
    }
}

/// Crossfade previous sentence's tail into current sentence's head (in-place).
/// Blends `prev_tail` (fading out) with the head of `samples` (fading in).
fn crossfade_into(prev_tail: &[f32], samples: &mut [f32], overlap: usize) {
    let overlap = overlap.min(prev_tail.len()).min(samples.len());
    for i in 0..overlap {
        let t = i as f32 / overlap as f32;
        let fade_out_val = (t * std::f32::consts::FRAC_PI_2).cos();
        let fade_in_val = (t * std::f32::consts::FRAC_PI_2).sin();
        samples[i] = prev_tail[i] * fade_out_val + samples[i] * fade_in_val;
    }
}

/// Which TTS strategy to use in the streaming loop.
enum TtsStrategy {
    /// macOS `say` command — instant, no model loading.
    Say { voice: Option<String> },
    /// qwen-native streaming — neural voice, needs model load.
    QwenNative { lang: Option<String> },
    /// qwen-native voice cloning — blocking per sentence.
    VoiceClone {
        voice_clone: crate::db::VoiceClone,
        lang: Option<String>,
    },
}

/// Run the streaming chat loop: STT -> Claude streaming -> TTS pipelining.
pub fn run_chat_loop(config: ChatConfig) -> Result<()> {
    let mut messages: Vec<Message> = Vec::new();
    let greeting = "Bonjour, je t'écoute.";

    // Determine TTS strategy: voice clone > say (macOS, fast) > qwen-native
    let strategy = if let Some(vc) = config.voice_clone.clone() {
        TtsStrategy::VoiceClone {
            voice_clone: vc,
            lang: config.lang.clone(),
        }
    } else if cfg!(target_os = "macos") {
        // Use say backend on macOS for instant TTS
        let voice = crate::db::open()
            .ok()
            .and_then(|conn| crate::db::get_preferences(&conn).ok())
            .and_then(|prefs| prefs.voice);
        TtsStrategy::Say { voice }
    } else {
        TtsStrategy::QwenNative {
            lang: config.lang.clone(),
        }
    };

    // Pre-load qwen-native model in background while greeting plays.
    let needs_qwen = matches!(
        strategy,
        TtsStrategy::QwenNative { .. } | TtsStrategy::VoiceClone { .. }
    );
    let preload_handle = if needs_qwen {
        Some(thread::spawn(|| qwen_native::preload_model(None)))
    } else {
        None
    };

    eprintln!("{greeting}");
    speak_text(greeting, &config)?;

    // Wait for model to finish loading (usually done by now).
    if let Some(handle) = preload_handle
        && let Err(e) = handle
            .join()
            .map_err(|_| anyhow::anyhow!("preload thread panicked"))?
    {
        eprintln!("Warning: model preload failed: {e}");
    }

    let tmp_dir = std::env::temp_dir();
    let audio_path = tmp_dir.join("vox_chat_input.wav");
    let audio_str = audio_path.to_string_lossy().to_string();

    loop {
        eprintln!("\n[Appuie sur Enter quand tu as fini de parler]");
        io::stderr().flush()?;

        record_until_enter(&audio_str)?;

        eprint!("Transcription...");
        io::stderr().flush()?;
        let user_text = stt::transcribe(&audio_str, config.lang.as_deref())?;
        eprintln!(" \"{user_text}\"");

        if user_text.is_empty() {
            eprintln!("(rien détecté, réessaie)");
            continue;
        }

        if is_exit(&user_text) {
            let farewell = "Au revoir !";
            eprintln!("{farewell}");
            speak_text(farewell, &config)?;
            break;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: user_text,
        });

        // --- Streaming response ---
        eprint!("Réflexion...");
        io::stderr().flush()?;

        // Channel: main thread -> TTS thread (sentences to speak)
        let (tts_tx, tts_rx) = mpsc::channel::<TtsCommand>();

        // Spawn TTS thread with the chosen strategy
        let tts_handle = match &strategy {
            TtsStrategy::Say { voice } => {
                let voice = voice.clone();
                thread::spawn(move || -> Result<()> { run_tts_say_loop(tts_rx, voice.as_deref()) })
            }
            TtsStrategy::QwenNative { lang } => {
                let lang = lang.clone();
                thread::spawn(move || -> Result<()> {
                    run_tts_streaming_loop(tts_rx, lang.as_deref())
                })
            }
            TtsStrategy::VoiceClone { voice_clone, lang } => {
                let vc = voice_clone.clone();
                let lang = lang.clone();
                thread::spawn(move || -> Result<()> { run_tts_clone_loop(tts_rx, Some(vc), lang) })
            }
        };

        // Channel: Claude stream -> main thread (text deltas)
        let (claude_tx, claude_rx) = mpsc::channel::<StreamEvent>();

        // Spawn Claude streaming in a thread
        let api_key = config.api_key.clone();
        let model = config.model.clone();
        let msgs = messages.clone();
        let claude_handle = thread::spawn(move || -> Result<()> {
            stream_claude(&api_key, &model, &msgs, claude_tx)
        });

        // Accumulate sentences from Claude stream and send to TTS
        // Use lower threshold (60 chars) for faster first-sentence delivery.
        let mut accumulator = SentenceAccumulator::with_min_chars(STREAMING_MIN_CHUNK_CHARS);
        let mut full_reply = String::new();
        let mut first_token = true;

        for event in claude_rx {
            match event {
                StreamEvent::TextDelta(text) => {
                    if first_token {
                        eprintln!(" OK");
                        first_token = false;
                    }
                    full_reply.push_str(&text);
                    eprint!("{text}");
                    io::stderr().flush()?;

                    for sentence in accumulator.push(&text) {
                        let _ = tts_tx.send(TtsCommand::Speak(sentence));
                    }
                }
                StreamEvent::Done => {
                    if let Some(remaining) = accumulator.flush() {
                        let _ = tts_tx.send(TtsCommand::Speak(remaining));
                    }
                    let _ = tts_tx.send(TtsCommand::Done);
                    break;
                }
            }
        }
        eprintln!(); // newline after streamed text

        // Wait for Claude thread to finish
        if let Err(e) = claude_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Claude thread panicked"))?
        {
            eprintln!("Claude API error: {e}");
            let _ = tts_tx.send(TtsCommand::Done);
        }

        // Wait for TTS to finish playback
        if let Err(e) = tts_handle
            .join()
            .map_err(|_| anyhow::anyhow!("TTS thread panicked"))?
        {
            eprintln!("TTS error: {e}");
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: full_reply,
        });
    }

    let _ = std::fs::remove_file(&audio_path);
    Ok(())
}

/// TTS thread using macOS `say` command — instant, sentence by sentence.
fn run_tts_say_loop(rx: mpsc::Receiver<TtsCommand>, voice: Option<&str>) -> Result<()> {
    for cmd in rx {
        match cmd {
            TtsCommand::Speak(text) => {
                let mut cmd = Command::new("/usr/bin/say");
                if let Some(v) = voice {
                    cmd.arg("-v").arg(v);
                }
                cmd.arg(&text);
                let _ = cmd.status();
            }
            TtsCommand::Done => break,
        }
    }
    Ok(())
}

/// TTS thread using qwen-native: blocking synthesis per sentence with crossfade.
///
/// Uses `synthesize_with_voice()` to generate each sentence as a complete audio
/// buffer. Each sentence plays smoothly (no intra-sentence gaps). While the sink
/// plays sentence N, synthesis of sentence N+1 starts immediately, minimizing
/// inter-sentence gaps. Cosine crossfade eliminates clicks at boundaries.
fn run_tts_streaming_loop(rx: mpsc::Receiver<TtsCommand>, lang: Option<&str>) -> Result<()> {
    use qwen3_tts::Speaker;

    let (_stream, stream_handle) =
        OutputStream::try_default().context("Failed to open audio output device")?;
    let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;

    let tts_lang = qwen_native::parse_language(lang.unwrap_or("en"))?;

    let mut prev_tail: Option<(Vec<f32>, u32)> = None;
    let mut is_first_sentence = true;

    for cmd in rx {
        match cmd {
            TtsCommand::Speak(text) => {
                qwen_native::with_model(None, |model| {
                    let audio =
                        model.synthesize_with_voice(&text, Speaker::Ryan, tts_lang, None)?;
                    let sr = audio.sample_rate;
                    let mut samples = audio.samples;
                    let overlap = ((sr as usize * CROSSFADE_MS) / 1000).min(samples.len());

                    // Crossfade with previous sentence's tail.
                    if let Some((tail, _)) = prev_tail.take() {
                        crossfade_into(&tail, &mut samples, overlap);
                    } else if is_first_sentence {
                        fade_in(&mut samples, overlap);
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
            TtsCommand::Done => {
                // Flush remaining tail with fade-out.
                if let Some((mut tail, sr)) = prev_tail.take() {
                    let n = tail.len();
                    fade_out(&mut tail, n);
                    sink.append(SamplesBuffer::new(1, sr, tail));
                }
                break;
            }
        }
    }

    // Wait for all queued audio to finish playing.
    sink.sleep_until_end();
    Ok(())
}

/// TTS thread for voice cloning: blocking synthesis per sentence, with crossfade.
fn run_tts_clone_loop(
    rx: mpsc::Receiver<TtsCommand>,
    voice_clone: Option<crate::db::VoiceClone>,
    lang: Option<String>,
) -> Result<()> {
    use qwen3_tts::AudioBuffer;

    let vc = voice_clone.context("voice clone config missing")?;
    let tts_lang = qwen_native::parse_language(lang.as_deref().unwrap_or("en"))?;

    let (_stream, stream_handle) =
        OutputStream::try_default().context("Failed to open audio output device")?;
    let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;

    let mut prev_tail: Option<(Vec<f32>, u32)> = None;
    let mut is_first_sentence = true;

    for cmd in rx {
        match cmd {
            TtsCommand::Speak(text) => {
                qwen_native::with_model(None, |model| {
                    let ref_audio = AudioBuffer::load(&vc.ref_audio).with_context(|| {
                        format!("failed to load reference audio: {}", vc.ref_audio)
                    })?;
                    let prompt =
                        model.create_voice_clone_prompt(&ref_audio, vc.ref_text.as_deref())?;
                    let audio = model.synthesize_voice_clone(&text, &prompt, tts_lang, None)?;
                    let sr = audio.sample_rate as u32;
                    let mut samples = audio.samples;
                    let overlap = ((sr as usize * CROSSFADE_MS) / 1000).min(samples.len());

                    // Crossfade with previous sentence's tail.
                    if let Some((tail, _)) = prev_tail.take() {
                        crossfade_into(&tail, &mut samples, overlap);
                    } else if is_first_sentence {
                        fade_in(&mut samples, overlap);
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
            TtsCommand::Done => {
                if let Some((mut tail, sr)) = prev_tail.take() {
                    let n = tail.len();
                    fade_out(&mut tail, n);
                    sink.append(SamplesBuffer::new(1, sr, tail));
                }
                break;
            }
        }
    }

    sink.sleep_until_end();
    Ok(())
}
