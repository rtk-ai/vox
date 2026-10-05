//! Which hardware acceleration this binary was built with.
//!
//! The answer is fixed at compile time by the cargo features `cuda` and
//! `metal`. Only two parts of vox use it: Whisper speech-to-text
//! ([`crate::stt::whisper::best_device`]) and the `qwen-native` backend
//! (`qwen3_tts::auto_device`). The default voices, pocket and piper, run on
//! the CPU in every build.
//!
//! This reports the build, not the device in use: a CUDA or Metal build
//! still falls back to the CPU when the GPU device cannot be created.

/// The acceleration a build of vox was compiled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acceleration {
    /// Built with the `cuda` feature (NVIDIA GPU).
    Cuda,
    /// Built with the `metal` feature (Apple Silicon GPU).
    Metal,
    /// Built with neither feature.
    Cpu,
}

impl Acceleration {
    /// The build kind for a set of enabled features. CUDA wins over Metal
    /// when both are on, in the same order as the device selection in
    /// `stt::whisper::best_device` and `qwen3_tts::auto_device`.
    pub const fn from_features(cuda: bool, metal: bool) -> Self {
        if cuda {
            Self::Cuda
        } else if metal {
            Self::Metal
        } else {
            Self::Cpu
        }
    }

    /// The build kind of the running binary.
    pub const fn current() -> Self {
        Self::from_features(cfg!(feature = "cuda"), cfg!(feature = "metal"))
    }

    /// One line, in plain words, saying what this build accelerates.
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Cuda => "CUDA (NVIDIA GPU): used by Whisper and qwen-native",
            Self::Metal => "Metal (GPU): used by Whisper and qwen-native",
            Self::Cpu => "CPU only",
        }
    }
}

/// One-line description of the running binary's acceleration.
pub const fn describe() -> &'static str {
    Acceleration::current().describe()
}

/// The `acceleration:` line printed by `vox config show` and returned by the
/// MCP `vox_config_show` tool.
pub fn config_line() -> String {
    format!("acceleration: {}", describe())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_feature_is_cpu() {
        assert_eq!(Acceleration::from_features(false, false), Acceleration::Cpu);
    }

    #[test]
    fn metal_feature_is_metal() {
        assert_eq!(
            Acceleration::from_features(false, true),
            Acceleration::Metal
        );
    }

    #[test]
    fn cuda_feature_is_cuda() {
        assert_eq!(Acceleration::from_features(true, false), Acceleration::Cuda);
    }

    #[test]
    fn cuda_wins_over_metal() {
        assert_eq!(Acceleration::from_features(true, true), Acceleration::Cuda);
    }

    #[test]
    fn descriptions_are_the_documented_ones() {
        assert_eq!(
            Acceleration::Cuda.describe(),
            "CUDA (NVIDIA GPU): used by Whisper and qwen-native"
        );
        assert_eq!(
            Acceleration::Metal.describe(),
            "Metal (GPU): used by Whisper and qwen-native"
        );
        assert_eq!(Acceleration::Cpu.describe(), "CPU only");
    }

    /// The description must fit on the single `acceleration:` line.
    #[test]
    fn descriptions_are_single_lines() {
        for kind in [Acceleration::Cuda, Acceleration::Metal, Acceleration::Cpu] {
            let text = kind.describe();
            assert!(!text.is_empty());
            assert!(!text.contains('\n'), "{kind:?}: {text:?}");
        }
    }

    #[test]
    fn current_follows_the_enabled_features() {
        let expected = if cfg!(feature = "cuda") {
            Acceleration::Cuda
        } else if cfg!(feature = "metal") {
            Acceleration::Metal
        } else {
            Acceleration::Cpu
        };
        assert_eq!(Acceleration::current(), expected);
        assert_eq!(describe(), expected.describe());
    }

    #[test]
    fn config_line_is_labelled() {
        assert_eq!(config_line(), format!("acceleration: {}", describe()));
    }
}
