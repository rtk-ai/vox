pub mod claude_api;
pub mod sentence;
pub mod streaming;

use anyhow::Result;
use serde::Serialize;

use crate::db;

pub const DEFAULT_MODEL: &str = "claude-haiku-4-5";
pub const MAX_TOKENS: u32 = 1024;
pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2023-06-01";

const EXIT_WORDS: &[&str] = &[
    "quit",
    "exit",
    "au revoir",
    "bye",
    "goodbye",
    "stop",
    "arrête",
    "arrete",
];

pub struct ChatConfig {
    pub voice_clone: Option<db::VoiceClone>,
    pub lang: Option<String>,
    pub api_key: String,
    pub model: String,
}

#[derive(Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            voice_clone: None,
            lang: None,
            api_key: String::new(),
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

pub fn is_exit(text: &str) -> bool {
    let lower = text.to_lowercase();
    let trimmed = lower.trim().trim_end_matches(['.', '!', '?']);
    EXIT_WORDS.contains(&trimmed)
}

/// Record from the microphone until the user presses Enter. Returns 16 kHz mono samples.
pub fn record_until_enter() -> Result<Vec<f32>> {
    crate::mic::record(&crate::mic::RecordOptions::until_enter())
}

/// What vox itself says and prints during a conversation, as opposed to what
/// Claude answers.
#[derive(Debug, PartialEq)]
pub struct Phrases {
    pub greeting: &'static str,
    pub farewell: &'static str,
    pub press_enter: &'static str,
    pub transcribing: &'static str,
    pub nothing_heard: &'static str,
    pub thinking: &'static str,
}

const ENGLISH: Phrases = Phrases {
    greeting: "Hello, I'm listening.",
    farewell: "Goodbye!",
    press_enter: "[Press Enter when you have finished speaking]",
    transcribing: "Transcribing...",
    nothing_heard: "(nothing heard, try again)",
    thinking: "Thinking...",
};

const FRENCH: Phrases = Phrases {
    greeting: "Bonjour, je t'écoute.",
    farewell: "Au revoir !",
    press_enter: "[Appuie sur Enter quand tu as fini de parler]",
    transcribing: "Transcription...",
    nothing_heard: "(rien détecté, réessaie)",
    thinking: "Réflexion...",
};

const ENGLISH_SYSTEM_PROMPT: &str =
    "You are a voice assistant. Answer concisely and naturally, as in a spoken conversation.";
const FRENCH_SYSTEM_PROMPT: &str = "Tu es un assistant vocal. Réponds de manière concise et naturelle, comme dans une conversation orale.";

/// The conversation language as a code vox supports. `vox chat` passes `-l`
/// or the stored preference through unchecked, so `fr-FR` and `FR` are read
/// as French, and a code vox does not know counts as unset.
fn conversation_lang(lang: Option<&str>) -> Option<String> {
    lang.and_then(crate::lang::normalize_locale)
}

/// The phrases for a conversation in `lang`: French for `fr`, English for
/// everything else. Only these two are translated; another language still
/// gets its replies in that language, through [`system_prompt`].
pub fn phrases(lang: Option<&str>) -> &'static Phrases {
    match conversation_lang(lang).as_deref() {
        Some("fr") => &FRENCH,
        _ => &ENGLISH,
    }
}

/// The system prompt for a conversation in `lang`. Claude tends to answer in
/// the language of the prompt, so a language without its own prompt gets the
/// English one plus an explicit instruction.
pub fn system_prompt(lang: Option<&str>) -> String {
    let lang = conversation_lang(lang);
    match lang.as_deref() {
        Some("fr") => FRENCH_SYSTEM_PROMPT.to_string(),
        None | Some("en") => ENGLISH_SYSTEM_PROMPT.to_string(),
        Some(code) => match crate::lang::lang_name(code) {
            Some(name) => format!("{ENGLISH_SYSTEM_PROMPT} Answer in {name}."),
            None => ENGLISH_SYSTEM_PROMPT.to_string(),
        },
    }
}

pub use claude_api::{ClaudeRequest, build_claude_request, parse_claude_response};
pub use streaming::run_chat_loop;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SUPPORTED_LANGS;

    #[test]
    fn phrases_are_english_when_the_language_is_english_or_unset() {
        for lang in [None, Some("en"), Some("en-US"), Some("")] {
            assert_eq!(phrases(lang), &ENGLISH, "lang {lang:?}");
        }
        assert_eq!(phrases(None).greeting, "Hello, I'm listening.");
        assert_eq!(phrases(None).farewell, "Goodbye!");
    }

    #[test]
    fn phrases_keep_the_french_wording_for_fr() {
        for lang in ["fr", "FR", "fr-FR"] {
            assert_eq!(phrases(Some(lang)), &FRENCH, "lang {lang}");
        }
        let fr = phrases(Some("fr"));
        assert_eq!(fr.greeting, "Bonjour, je t'écoute.");
        assert_eq!(fr.farewell, "Au revoir !");
        assert_eq!(
            fr.press_enter,
            "[Appuie sur Enter quand tu as fini de parler]"
        );
        assert_eq!(fr.transcribing, "Transcription...");
        assert_eq!(fr.nothing_heard, "(rien détecté, réessaie)");
        assert_eq!(fr.thinking, "Réflexion...");
    }

    #[test]
    fn phrases_are_english_for_any_other_language() {
        for lang in SUPPORTED_LANGS.iter().filter(|l| **l != "fr") {
            assert_eq!(phrases(Some(lang)), &ENGLISH, "lang {lang}");
        }
        assert_eq!(phrases(Some("klingon")), &ENGLISH);
    }

    #[test]
    fn system_prompt_is_english_when_the_language_is_english_or_unset() {
        for lang in [None, Some("en"), Some("")] {
            assert_eq!(system_prompt(lang), ENGLISH_SYSTEM_PROMPT, "lang {lang:?}");
        }
    }

    #[test]
    fn system_prompt_keeps_the_french_wording_for_fr() {
        assert_eq!(
            system_prompt(Some("fr")),
            "Tu es un assistant vocal. Réponds de manière concise et naturelle, comme dans une conversation orale."
        );
    }

    #[test]
    fn system_prompt_asks_for_any_other_language_by_name() {
        assert_eq!(
            system_prompt(Some("es")),
            format!("{ENGLISH_SYSTEM_PROMPT} Answer in Spanish.")
        );
        for lang in SUPPORTED_LANGS.iter().filter(|l| !["en", "fr"].contains(l)) {
            let name = crate::lang::lang_name(lang).expect("supported language has a name");
            let prompt = system_prompt(Some(lang));
            assert!(prompt.starts_with(ENGLISH_SYSTEM_PROMPT), "lang {lang}");
            assert!(
                prompt.ends_with(&format!(" Answer in {name}.")),
                "lang {lang}: {prompt}"
            );
        }
    }

    #[test]
    fn system_prompt_does_not_name_a_language_vox_does_not_know() {
        assert_eq!(system_prompt(Some("klingon")), ENGLISH_SYSTEM_PROMPT);
    }
}
