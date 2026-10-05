//! macOS `say` backend — system TTS via /usr/bin/say.
//!
//! Near-zero latency, uses Apple's built-in voices. No voice cloning support.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use super::{SpeakOptions, TtsBackend};
use crate::audio;

pub struct SayBackend;

impl SayBackend {
    /// The `say` invocation for `opts`, as given. With another output name
    /// than `.wav`, `say` picks the format itself and appends `.aiff` to a
    /// name without extension: `speak` only ever hands it a `.wav` one.
    pub fn build_command(text: &str, opts: &SpeakOptions) -> Command {
        let mut cmd = Command::new("/usr/bin/say");
        if let Some(ref voice) = opts.voice {
            cmd.arg("-v").arg(voice);
        }
        if let Some(rate) = opts.rate {
            cmd.arg("-r").arg(rate.to_string());
        }
        if let Some(ref out) = opts.output {
            // say writes AIFF by default; ask for 16-bit PCM so a .wav name
            // holds real WAVE data instead of an AIFF with the wrong suffix.
            cmd.arg("-o").arg(out);
            if out
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            {
                cmd.arg("--file-format=WAVE")
                    .arg("--data-format=LEI16@22050");
            }
        }
        // `say` reads an argument that starts with a dash as an option: a
        // text like "- first item" was refused, and "-o/some/path" chose
        // where to write. `--` ends the options.
        if text.starts_with('-') {
            cmd.arg("--");
        }
        cmd.arg(text);
        cmd
    }

    /// Whether vox has to render a WAV and handle it itself, rather than let
    /// `say` play. Saving does, and so does a volume: `say` has no flag to
    /// scale what it plays. Everything else stays a plain `say "text"`, which
    /// is heard as it is generated instead of after the whole render.
    fn renders_first(opts: &SpeakOptions) -> bool {
        opts.output.is_some() || (opts.volume - 1.0).abs() > f32::EPSILON
    }

    /// Render `text` into `wav`, which must carry a `.wav` name, at the
    /// volume asked.
    fn render(text: &str, opts: &SpeakOptions, wav: &Path) -> Result<()> {
        let to_wav = SpeakOptions {
            output: Some(wav.to_path_buf()),
            ..opts.clone()
        };
        run(Self::build_command(text, &to_wav))?;
        audio::apply_wav_gain(wav, opts.volume)
    }
}

fn run(mut cmd: Command) -> Result<()> {
    let status = cmd.status().context("Failed to run /usr/bin/say")?;
    if !status.success() {
        anyhow::bail!("say exited with status {status}");
    }
    Ok(())
}

impl TtsBackend for SayBackend {
    fn name(&self) -> &str {
        "say"
    }

    fn speak(&self, text: &str, opts: &SpeakOptions) -> Result<()> {
        if !Self::renders_first(opts) {
            return run(Self::build_command(text, opts));
        }
        // The user's path never reaches `say`: it would not create the parent
        // directory, and would choose the format from the name.
        let dir = tempfile::tempdir().context("failed to create temp directory")?;
        let rendered = dir.path().join("say.wav");
        Self::render(text, opts, &rendered)?;
        audio::deliver(&rendered, opts.output.as_deref())
    }

    fn list_voices(&self) -> Result<Vec<String>> {
        let output = Command::new("/usr/bin/say")
            .arg("-v")
            .arg("?")
            .output()
            .context("Failed to list voices")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let voices: Vec<String> = stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(String::from)
            .collect();
        Ok(voices)
    }

    fn is_available(&self) -> bool {
        std::path::Path::new("/usr/bin/say").exists()
    }
}

/// These run `/usr/bin/say -o <file>`, which renders without playing.
#[cfg(test)]
mod tests {
    use super::*;

    fn save(out: &Path, volume: f32) {
        let opts = SpeakOptions {
            output: Some(out.to_path_buf()),
            volume,
            ..Default::default()
        };
        SayBackend.speak("hello", &opts).unwrap();
    }

    fn peak(wav: &Path) -> i32 {
        hound::WavReader::open(wav)
            .unwrap_or_else(|e| panic!("{} is not a WAV: {e}", wav.display()))
            .samples::<i32>()
            .map(|s| s.unwrap().abs())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn output_creates_the_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("missing/dir/out.wav");
        save(&out, 1.0);
        assert!(peak(&out) > 0, "the saved file must hold audio");
    }

    #[test]
    fn output_is_a_wav_under_the_name_given_whatever_the_name() {
        for name in ["out.wav", "noext", "out.mp3", "out.m4a", "out.xyz"] {
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join(name);
            save(&out, 1.0);

            let written: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(written, [name], "one file, under the name given");
            assert!(peak(&out) > 0, "{name} must be a WAV that holds audio");
        }
    }

    #[test]
    fn volume_scales_the_saved_audio() {
        let dir = tempfile::tempdir().unwrap();
        let (full, quiet) = (dir.path().join("full.wav"), dir.path().join("quiet.wav"));
        save(&full, 1.0);
        save(&quiet, 0.25);

        let ratio = peak(&quiet) as f32 / peak(&full) as f32;
        assert!(
            (0.2..0.3).contains(&ratio),
            "--volume 0.25 must quarter the peak, got a ratio of {ratio}"
        );
    }

    /// `say` reads an argument that starts with a dash as an option, so a
    /// list item or a negative number was refused, read as a file to speak
    /// (`-f`), or taken as the place to write (`-o`).
    #[test]
    fn text_starting_with_a_dash_is_spoken_not_read_as_an_option() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere.aiff");
        let as_output = format!("-o{}", elsewhere.display());
        let texts = [
            "- a list item",
            "-five degrees",
            "-v Samantha",
            "--rate=90 words",
            as_output.as_str(),
        ];
        for (i, text) in texts.into_iter().enumerate() {
            let out = dir.path().join(format!("{i}.wav"));
            let opts = SpeakOptions {
                output: Some(out.clone()),
                ..Default::default()
            };
            SayBackend
                .speak(text, &opts)
                .unwrap_or_else(|e| panic!("{text:?} was not spoken: {e:#}"));
            assert!(peak(&out) > 0, "{text:?} must be spoken, not obeyed");
        }
        assert!(!elsewhere.exists(), "the text must not choose the output");
    }

    /// Without `-o`, a volume is rendered then played by vox, and that render
    /// is what `audio::deliver` hands to the player: it must decode.
    #[test]
    fn a_volume_is_rendered_into_something_the_player_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = dir.path().join("say.wav");
        let opts = SpeakOptions {
            volume: 0.5,
            ..Default::default()
        };
        assert!(SayBackend::renders_first(&opts));
        SayBackend::render("hello", &opts, &rendered).unwrap();

        let file = std::io::BufReader::new(std::fs::File::open(&rendered).unwrap());
        assert!(rodio::Decoder::new(file).unwrap().count() > 0);
    }

    /// Plain speech must stay a direct `say "text"`: rendering first would
    /// delay the first word by the length of the whole synthesis.
    #[test]
    fn default_volume_without_output_lets_say_play_directly() {
        assert!(!SayBackend::renders_first(&SpeakOptions::default()));
    }
}
