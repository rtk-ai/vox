#[cfg(target_os = "macos")]
pub mod qwen;
pub mod qwen_native;
#[cfg(target_os = "macos")]
pub mod say;

use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct SpeakOptions {
    pub voice: Option<String>,
    pub lang: Option<String>,
    pub rate: Option<u32>,
    pub gender: Option<String>,
    pub style: Option<String>,
    pub ref_audio: Option<String>,
    pub ref_text: Option<String>,
    pub model: Option<String>,
}

pub trait TtsBackend {
    fn name(&self) -> &str;
    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()>;
    fn list_voices(&self) -> Result<Vec<String>>;
    fn is_available(&self) -> bool;
}

pub fn get_backend(name: &str) -> Result<Box<dyn TtsBackend>> {
    match name {
        #[cfg(target_os = "macos")]
        "say" => Ok(Box::new(say::SayBackend)),
        #[cfg(target_os = "macos")]
        "qwen" => Ok(Box::new(qwen::QwenBackend)),
        "qwen-native" => Ok(Box::new(qwen_native::QwenNativeBackend)),
        #[cfg(not(target_os = "macos"))]
        "say" | "qwen" => {
            anyhow::bail!("Backend '{name}' is only available on macOS. Use 'qwen-native' instead.")
        }
        _ => anyhow::bail!("Unknown backend: {name}"),
    }
}
