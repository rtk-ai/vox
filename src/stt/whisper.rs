//! Whisper inference on candle, adapted from the candle whisper example.
//!
//! Greedy decoding with temperature fallback, optional language detection,
//! 30-second windows. Weights come from the Hugging Face hub (safetensors).

use anyhow::{Error as E, Result, anyhow};
use candle_core::{D, Device, IndexOp, Tensor};
use candle_nn::{VarBuilder, ops::softmax};
use candle_transformers::models::whisper::{self as m, Config, audio, model::Whisper};
use hf_hub::{Repo, RepoType, api::sync::Api};
use tokenizers::Tokenizer;

/// Language codes understood by multilingual Whisper checkpoints.
pub const LANGUAGES: [(&str, &str); 99] = [
    ("en", "english"),
    ("zh", "chinese"),
    ("de", "german"),
    ("es", "spanish"),
    ("ru", "russian"),
    ("ko", "korean"),
    ("fr", "french"),
    ("ja", "japanese"),
    ("pt", "portuguese"),
    ("tr", "turkish"),
    ("pl", "polish"),
    ("ca", "catalan"),
    ("nl", "dutch"),
    ("ar", "arabic"),
    ("sv", "swedish"),
    ("it", "italian"),
    ("id", "indonesian"),
    ("hi", "hindi"),
    ("fi", "finnish"),
    ("vi", "vietnamese"),
    ("he", "hebrew"),
    ("uk", "ukrainian"),
    ("el", "greek"),
    ("ms", "malay"),
    ("cs", "czech"),
    ("ro", "romanian"),
    ("da", "danish"),
    ("hu", "hungarian"),
    ("ta", "tamil"),
    ("no", "norwegian"),
    ("th", "thai"),
    ("ur", "urdu"),
    ("hr", "croatian"),
    ("bg", "bulgarian"),
    ("lt", "lithuanian"),
    ("la", "latin"),
    ("mi", "maori"),
    ("ml", "malayalam"),
    ("cy", "welsh"),
    ("sk", "slovak"),
    ("te", "telugu"),
    ("fa", "persian"),
    ("lv", "latvian"),
    ("bn", "bengali"),
    ("sr", "serbian"),
    ("az", "azerbaijani"),
    ("sl", "slovenian"),
    ("kn", "kannada"),
    ("et", "estonian"),
    ("mk", "macedonian"),
    ("br", "breton"),
    ("eu", "basque"),
    ("is", "icelandic"),
    ("hy", "armenian"),
    ("ne", "nepali"),
    ("mn", "mongolian"),
    ("bs", "bosnian"),
    ("kk", "kazakh"),
    ("sq", "albanian"),
    ("sw", "swahili"),
    ("gl", "galician"),
    ("mr", "marathi"),
    ("pa", "punjabi"),
    ("si", "sinhala"),
    ("km", "khmer"),
    ("sn", "shona"),
    ("yo", "yoruba"),
    ("so", "somali"),
    ("af", "afrikaans"),
    ("oc", "occitan"),
    ("ka", "georgian"),
    ("be", "belarusian"),
    ("tg", "tajik"),
    ("sd", "sindhi"),
    ("gu", "gujarati"),
    ("am", "amharic"),
    ("yi", "yiddish"),
    ("lo", "lao"),
    ("uz", "uzbek"),
    ("fo", "faroese"),
    ("ht", "haitian creole"),
    ("ps", "pashto"),
    ("tk", "turkmen"),
    ("nn", "nynorsk"),
    ("mt", "maltese"),
    ("sa", "sanskrit"),
    ("lb", "luxembourgish"),
    ("my", "myanmar"),
    ("bo", "tibetan"),
    ("tl", "tagalog"),
    ("mg", "malagasy"),
    ("as", "assamese"),
    ("tt", "tatar"),
    ("haw", "hawaiian"),
    ("ln", "lingala"),
    ("ha", "hausa"),
    ("ba", "bashkir"),
    ("jw", "javanese"),
    ("su", "sundanese"),
];

/// Pick the best available device: CUDA, then Metal, then CPU.
pub fn best_device() -> Device {
    #[cfg(feature = "cuda")]
    if let Ok(d) = Device::new_cuda(0) {
        return d;
    }
    #[cfg(feature = "metal")]
    if let Ok(d) = Device::new_metal(0) {
        return d;
    }
    Device::Cpu
}

fn mel_filters(num_mel_bins: usize) -> Result<Vec<f32>> {
    let bytes: &[u8] = match num_mel_bins {
        80 => include_bytes!("melfilters.bytes"),
        128 => include_bytes!("melfilters128.bytes"),
        n => anyhow::bail!("unexpected num_mel_bins {n}"),
    };
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

fn token_id(tokenizer: &Tokenizer, token: &str) -> Result<u32> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| anyhow!("no token-id for {token}"))
}

/// Tiny deterministic RNG for temperature sampling (avoids a rand dependency).
struct XorShift(u64);

impl XorShift {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

struct Decoded {
    text: String,
    avg_logprob: f64,
    no_speech_prob: f64,
}

pub struct WhisperStt {
    model_id: String,
    model: Whisper,
    tokenizer: Tokenizer,
    device: Device,
    mel_filters: Vec<f32>,
    suppress_tokens: Tensor,
    sot_token: u32,
    transcribe_token: u32,
    eot_token: u32,
    no_speech_token: u32,
    no_timestamps_token: u32,
    multilingual: bool,
    rng: XorShift,
}

impl WhisperStt {
    /// Download (if needed) and load a Whisper checkpoint from the HF hub.
    pub fn load(model_id: &str) -> Result<Self> {
        let api = Api::new()?;
        let repo = api.repo(Repo::new(model_id.to_string(), RepoType::Model));
        let config_path = repo.get("config.json")?;
        let tokenizer_path = repo.get("tokenizer.json")?;
        let weights_path = repo.get("model.safetensors")?;

        let config: Config = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;
        let device = best_device();
        let mel_filters = mel_filters(config.num_mel_bins)?;

        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], m::DTYPE, &device)? };
        let model = Whisper::load(&vb, config)?;

        let no_timestamps_token = token_id(&tokenizer, m::NO_TIMESTAMPS_TOKEN)?;
        let suppress: Vec<f32> = (0..model.config.vocab_size as u32)
            .map(|i| {
                if model.config.suppress_tokens.contains(&i) {
                    f32::NEG_INFINITY
                } else {
                    0f32
                }
            })
            .collect();
        let suppress_tokens = Tensor::new(suppress.as_slice(), &device)?;
        let sot_token = token_id(&tokenizer, m::SOT_TOKEN)?;
        let transcribe_token = token_id(&tokenizer, m::TRANSCRIBE_TOKEN)?;
        let eot_token = token_id(&tokenizer, m::EOT_TOKEN)?;
        let no_speech_token = m::NO_SPEECH_TOKENS
            .iter()
            .find_map(|t| token_id(&tokenizer, t).ok())
            .ok_or_else(|| anyhow!("unable to find the no-speech token"))?;
        let multilingual = token_id(&tokenizer, "<|fr|>").is_ok();

        Ok(Self {
            model_id: model_id.to_string(),
            model,
            tokenizer,
            device,
            mel_filters,
            suppress_tokens,
            sot_token,
            transcribe_token,
            eot_token,
            no_speech_token,
            no_timestamps_token,
            multilingual,
            rng: XorShift(0x9E37_79B9_7F4A_7C15),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn is_multilingual(&self) -> bool {
        self.multilingual
    }

    /// Transcribe 16 kHz mono PCM. `lang = None` auto-detects on multilingual models.
    pub fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>) -> Result<String> {
        let cfg = self.model.config.clone();
        let mel = audio::pcm_to_mel(&cfg, pcm, &self.mel_filters);
        let mel_len = mel.len();
        let mel = Tensor::from_vec(
            mel,
            (1, cfg.num_mel_bins, mel_len / cfg.num_mel_bins),
            &self.device,
        )?;

        let language_token = match (self.multilingual, lang) {
            (true, Some(l)) => Some(token_id(
                &self.tokenizer,
                &format!("<|{}|>", l.trim().to_lowercase()),
            )?),
            (true, None) => Some(self.detect_language(&mel)?),
            (false, _) => None,
        };

        let (_, _, content_frames) = mel.dims3()?;
        let mut seek = 0;
        let mut out = String::new();
        while seek < content_frames {
            let segment_size = usize::min(content_frames - seek, m::N_FRAMES);
            let segment = mel.narrow(2, seek, segment_size)?;
            seek += segment_size;
            let dr = self.decode_with_fallback(&segment, language_token)?;
            if dr.no_speech_prob > m::NO_SPEECH_THRESHOLD && dr.avg_logprob < m::LOGPROB_THRESHOLD {
                continue;
            }
            let t = dr.text.trim();
            if !t.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(t);
            }
        }
        Ok(out)
    }

    fn detect_language(&mut self, mel: &Tensor) -> Result<u32> {
        let (_b, _n, seq_len) = mel.dims3()?;
        let mel = mel.narrow(
            2,
            0,
            usize::min(seq_len, self.model.config.max_source_positions),
        )?;
        let ids = LANGUAGES
            .iter()
            .map(|(code, _)| token_id(&self.tokenizer, &format!("<|{code}|>")))
            .collect::<Result<Vec<_>>>()?;
        let audio_features = self.model.encoder.forward(&mel, true)?;
        let tokens = Tensor::new(&[[self.sot_token]], &self.device)?;
        let ids_t = Tensor::new(ids.as_slice(), &self.device)?;
        let ys = self.model.decoder.forward(&tokens, &audio_features, true)?;
        let logits = self.model.decoder.final_linear(&ys.i(..1)?)?.i(0)?.i(0)?;
        let logits = logits.index_select(&ids_t, 0)?;
        let probs = softmax(&logits, D::Minus1)?.to_vec1::<f32>()?;
        let best = probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i)
            .unwrap_or(0);
        Ok(ids[best])
    }

    fn decode_with_fallback(
        &mut self,
        segment: &Tensor,
        language_token: Option<u32>,
    ) -> Result<Decoded> {
        let mut last = None;
        for &t in m::TEMPERATURES.iter() {
            match self.decode(segment, t, language_token) {
                Ok(dr) => {
                    let needs_fallback = dr.avg_logprob < m::LOGPROB_THRESHOLD;
                    if !needs_fallback || dr.no_speech_prob > m::NO_SPEECH_THRESHOLD {
                        return Ok(dr);
                    }
                    last = Some(dr);
                }
                Err(e) => {
                    if last.is_none() {
                        return Err(e);
                    }
                }
            }
        }
        last.ok_or_else(|| anyhow!("decoding failed"))
    }

    fn decode(&mut self, mel: &Tensor, t: f64, language_token: Option<u32>) -> Result<Decoded> {
        let audio_features = self.model.encoder.forward(mel, true)?;
        let sample_len = self.model.config.max_target_positions / 2;
        let mut sum_logprob = 0f64;
        let mut no_speech_prob = f64::NAN;
        let mut tokens = vec![self.sot_token];
        if let Some(l) = language_token {
            tokens.push(l);
        }
        tokens.push(self.transcribe_token);
        tokens.push(self.no_timestamps_token);
        let prompt_len = tokens.len();

        for i in 0..sample_len {
            let tokens_t = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let ys = self
                .model
                .decoder
                .forward(&tokens_t, &audio_features, i == 0)?;
            if i == 0 {
                let logits = self.model.decoder.final_linear(&ys.i(..1)?)?.i(0)?.i(0)?;
                no_speech_prob = softmax(&logits, 0)?
                    .i(self.no_speech_token as usize)?
                    .to_scalar::<f32>()? as f64;
            }
            let (_, seq_len, _) = ys.dims3()?;
            let logits = self
                .model
                .decoder
                .final_linear(&ys.i((..1, seq_len - 1..))?)?
                .i(0)?
                .i(0)?;
            let logits = logits.broadcast_add(&self.suppress_tokens)?;
            let next_token = if t > 0f64 {
                let prs: Vec<f32> = softmax(&(&logits / t)?, 0)?.to_vec1()?;
                let r = self.rng.next_f32();
                let mut acc = 0f32;
                let mut chosen = prs.len() as u32 - 1;
                for (i, p) in prs.iter().enumerate() {
                    acc += p;
                    if acc >= r {
                        chosen = i as u32;
                        break;
                    }
                }
                chosen
            } else {
                let v: Vec<f32> = logits.to_vec1()?;
                v.iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map(|(i, _)| i as u32)
                    .unwrap_or(self.eot_token)
            };
            tokens.push(next_token);
            let prob = softmax(&logits, D::Minus1)?
                .i(next_token as usize)?
                .to_scalar::<f32>()? as f64;
            if next_token == self.eot_token || tokens.len() > self.model.config.max_target_positions
            {
                break;
            }
            sum_logprob += prob.ln();
        }
        let generated = &tokens[prompt_len..];
        let text = self.tokenizer.decode(generated, true).map_err(E::msg)?;
        let avg_logprob = sum_logprob / generated.len().max(1) as f64;
        Ok(Decoded {
            text,
            avg_logprob,
            no_speech_prob,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_filter_tables_have_expected_shape() {
        assert_eq!(mel_filters(80).unwrap().len(), 80 * 201);
        assert_eq!(mel_filters(128).unwrap().len(), 128 * 201);
        assert!(mel_filters(64).is_err());
    }

    #[test]
    fn languages_are_unique() {
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len());
    }

    #[test]
    fn xorshift_in_unit_range() {
        let mut r = XorShift(42);
        for _ in 0..1000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
