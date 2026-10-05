use std::io::{self, Write};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};

use super::claude_api::{StreamEvent, stream_claude};
use super::sentence::{STREAMING_MIN_CHUNK_CHARS, SentenceAccumulator};
use super::{ChatConfig, Message, is_exit, phrases, record_until_enter, system_prompt};
use crate::backend::say::SayBackend;
use crate::backend::{SpeakOptions, qwen_native};
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

/// How the conversation is spoken. Chosen once, and used for everything vox
/// says in it: the greeting, the replies and the farewell.
#[derive(Debug, Clone)]
enum TtsStrategy {
    /// macOS `say` command — instant, no model loading.
    Say { voice: Option<String> },
    /// qwen-native voice cloning — blocking per sentence, needs model load.
    VoiceClone {
        voice_clone: crate::db::VoiceClone,
        lang: Option<String>,
    },
}

impl TtsStrategy {
    /// A voice clone when the conversation has one, `say` otherwise. Nothing
    /// but a clone is worth loading Qwen3-TTS for: this module only exists on
    /// macOS, where `say` is always there and speaks at once.
    fn choose(
        voice_clone: Option<crate::db::VoiceClone>,
        lang: Option<String>,
        say_voice: Option<String>,
    ) -> Self {
        match voice_clone {
            Some(voice_clone) => TtsStrategy::VoiceClone { voice_clone, lang },
            None => TtsStrategy::Say { voice: say_voice },
        }
    }

    /// Speak what arrives on `rx`, until `Done`.
    fn run(&self, rx: mpsc::Receiver<TtsCommand>) -> Result<()> {
        match self {
            TtsStrategy::Say { voice } => run_tts_say_loop(rx, voice.as_deref()),
            TtsStrategy::VoiceClone { voice_clone, lang } => {
                run_tts_clone_loop(rx, voice_clone, lang.as_deref())
            }
        }
    }

    /// Speak one sentence and wait for it to end.
    fn speak(&self, text: &str) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(TtsCommand::Speak(text.to_string()));
        let _ = tx.send(TtsCommand::Done);
        self.run(rx)
    }
}

/// Run the streaming chat loop: STT -> Claude streaming -> TTS pipelining.
pub fn run_chat_loop(config: ChatConfig) -> Result<()> {
    let mut messages: Vec<Message> = Vec::new();
    let phrases = phrases(config.lang.as_deref());
    let system = system_prompt(config.lang.as_deref());

    // TTS strategy: the voice clone if there is one, else say.
    let say_voice = crate::db::open()
        .ok()
        .and_then(|conn| crate::db::get_preferences(&conn).ok())
        .and_then(|prefs| prefs.voice);
    let strategy = TtsStrategy::choose(config.voice_clone.clone(), config.lang.clone(), say_voice);

    // With a clone this is where Qwen3-TTS loads, before the first turn.
    eprintln!("{}", phrases.greeting);
    strategy.speak(phrases.greeting)?;

    loop {
        eprintln!("\n{}", phrases.press_enter);
        io::stderr().flush()?;

        let samples = record_until_enter()?;

        eprint!("{}", phrases.transcribing);
        io::stderr().flush()?;
        let user_text = stt::transcribe_samples(&samples, config.lang.as_deref())?;
        eprintln!(" \"{user_text}\"");

        if user_text.is_empty() {
            eprintln!("{}", phrases.nothing_heard);
            continue;
        }

        if is_exit(&user_text) {
            eprintln!("{}", phrases.farewell);
            strategy.speak(phrases.farewell)?;
            break;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: user_text,
        });

        // --- Streaming response ---
        eprint!("{}", phrases.thinking);
        io::stderr().flush()?;

        // Channel: main thread -> TTS thread (sentences to speak)
        let (tts_tx, tts_rx) = mpsc::channel::<TtsCommand>();

        // Spawn TTS thread with the chosen strategy
        let tts_strategy = strategy.clone();
        let tts_handle = thread::spawn(move || tts_strategy.run(tts_rx));

        // Channel: Claude stream -> main thread (text deltas)
        let (claude_tx, claude_rx) = mpsc::channel::<StreamEvent>();

        // Spawn Claude streaming in a thread
        let api_key = config.api_key.clone();
        let model = config.model.clone();
        let system = system.clone();
        let msgs = messages.clone();
        let claude_handle = thread::spawn(move || -> Result<()> {
            stream_claude(&api_key, &model, &system, &msgs, claude_tx)
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

    Ok(())
}

/// The `say` invocation for one sentence: the one the `say` backend builds,
/// so that a reply starting with a dash ("- first item") is spoken instead of
/// being refused as an unknown option.
fn say_command(text: &str, voice: Option<&str>) -> Command {
    let opts = SpeakOptions {
        voice: voice.map(String::from),
        ..Default::default()
    };
    SayBackend::build_command(text, &opts)
}

/// TTS thread using macOS `say` command — instant, sentence by sentence.
fn run_tts_say_loop(rx: mpsc::Receiver<TtsCommand>, voice: Option<&str>) -> Result<()> {
    for cmd in rx {
        match cmd {
            TtsCommand::Speak(text) => {
                let _ = say_command(&text, voice).status();
            }
            TtsCommand::Done => break,
        }
    }
    Ok(())
}

/// TTS thread for voice cloning: blocking synthesis per sentence, with crossfade.
fn run_tts_clone_loop(
    rx: mpsc::Receiver<TtsCommand>,
    vc: &crate::db::VoiceClone,
    lang: Option<&str>,
) -> Result<()> {
    use qwen3_tts::AudioBuffer;

    let tts_lang = qwen_native::parse_language(lang.unwrap_or("en"))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn clone_named(name: &str) -> crate::db::VoiceClone {
        crate::db::VoiceClone {
            name: name.to_string(),
            ref_audio: "/tmp/ref.wav".to_string(),
            ref_text: None,
            created_at: String::new(),
        }
    }

    /// The greeting is spoken with the strategy chosen here. Without a clone
    /// it must be `say`, whatever the language: saying hello is no reason to
    /// load, or download, Qwen3-TTS.
    #[test]
    fn without_a_voice_clone_the_strategy_is_say() {
        for lang in [None, Some("en"), Some("fr"), Some("ja")] {
            let strategy =
                TtsStrategy::choose(None, lang.map(String::from), Some("Thomas".to_string()));
            assert!(
                matches!(&strategy, TtsStrategy::Say { voice } if voice.as_deref() == Some("Thomas")),
                "lang {lang:?}: {strategy:?}"
            );
        }
        assert!(matches!(
            TtsStrategy::choose(None, None, None),
            TtsStrategy::Say { voice: None }
        ));
    }

    #[test]
    fn with_a_voice_clone_the_strategy_is_the_clone_in_the_conversation_language() {
        let strategy = TtsStrategy::choose(
            Some(clone_named("patrick")),
            Some("fr".to_string()),
            Some("Thomas".to_string()),
        );
        match strategy {
            TtsStrategy::VoiceClone { voice_clone, lang } => {
                assert_eq!(voice_clone.name, "patrick");
                assert_eq!(lang.as_deref(), Some("fr"));
            }
            other => panic!("expected the voice clone, got {other:?}"),
        }
    }

    #[test]
    fn a_sentence_that_starts_with_a_dash_is_not_read_by_say_as_an_option() {
        let args = |cmd: Command| -> Vec<String> {
            cmd.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args(say_command("- first item", Some("Thomas"))),
            ["-v", "Thomas", "--", "- first item"]
        );
        assert_eq!(args(say_command("Hello.", None)), ["Hello."]);
    }
}
