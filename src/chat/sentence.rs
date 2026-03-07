/// Default minimum chunk size (matches backend/qwen.rs for subprocess TTS).
const DEFAULT_MIN_CHUNK_CHARS: usize = 120;

/// Lower threshold for streaming chat — smaller chunks = lower latency.
pub const STREAMING_MIN_CHUNK_CHARS: usize = 60;

/// Accumulates streaming text deltas and emits complete sentences.
///
/// A sentence boundary is detected when a terminal punctuation mark (`.!?;`)
/// appears and the accumulated buffer is at least `min_chars` long.
pub struct SentenceAccumulator {
    buffer: String,
    min_chars: usize,
}

impl Default for SentenceAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl SentenceAccumulator {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            min_chars: DEFAULT_MIN_CHUNK_CHARS,
        }
    }

    /// Create with a custom minimum chunk size (use `STREAMING_MIN_CHUNK_CHARS` for chat).
    pub fn with_min_chars(min_chars: usize) -> Self {
        Self {
            buffer: String::new(),
            min_chars,
        }
    }

    /// Push a text delta. Returns any complete sentences ready to be spoken.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        for ch in text.chars() {
            self.buffer.push(ch);
            if matches!(ch, '.' | '!' | '?' | ';') && self.buffer.trim().len() >= self.min_chars {
                let sentence = self.buffer.trim().to_string();
                if !sentence.is_empty() {
                    sentences.push(sentence);
                }
                self.buffer.clear();
            }
        }
        sentences
    }

    /// Flush any remaining text (call when Claude is done).
    pub fn flush(&mut self) -> Option<String> {
        let text = self.buffer.trim().to_string();
        self.buffer.clear();
        if text.is_empty() { None } else { Some(text) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_sentences_merged() {
        let mut acc = SentenceAccumulator::new();
        // Short sentence shouldn't emit
        let result = acc.push("Hello. World. ");
        assert!(result.is_empty());
        // Flush returns the accumulated text
        let flushed = acc.flush();
        assert_eq!(flushed, Some("Hello. World.".to_string()));
    }

    #[test]
    fn test_long_sentence_emits() {
        let mut acc = SentenceAccumulator::new();
        let long = "A".repeat(120) + ".";
        let result = acc.push(&long);
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with('.'));
    }

    #[test]
    fn test_multiple_long_sentences() {
        let mut acc = SentenceAccumulator::new();
        let text = format!("{}. {}. Remaining", "A".repeat(120), "B".repeat(130));
        let result = acc.push(&text);
        assert_eq!(result.len(), 2);
        let flushed = acc.flush();
        assert_eq!(flushed, Some("Remaining".to_string()));
    }

    #[test]
    fn test_incremental_deltas() {
        let mut acc = SentenceAccumulator::new();
        let sentence = "A".repeat(118);
        assert!(acc.push(&sentence).is_empty());
        // Adding more text with punctuation should emit
        let result = acc.push("BB.");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_flush_empty() {
        let mut acc = SentenceAccumulator::new();
        assert_eq!(acc.flush(), None);
    }

    #[test]
    fn test_exclamation_and_question() {
        let mut acc = SentenceAccumulator::new();
        let long = "A".repeat(120) + "!";
        let result = acc.push(&long);
        assert_eq!(result.len(), 1);

        let long = "B".repeat(120) + "?";
        let result = acc.push(&long);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_streaming_threshold() {
        let mut acc = SentenceAccumulator::with_min_chars(STREAMING_MIN_CHUNK_CHARS);
        let long = "A".repeat(60) + ".";
        let result = acc.push(&long);
        assert_eq!(result.len(), 1);

        // Below threshold — shouldn't emit
        let short = "A".repeat(50) + ".";
        let result = acc.push(&short);
        assert!(result.is_empty());
    }
}
