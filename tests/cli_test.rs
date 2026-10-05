use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// A config directory and database of the test's own: `clone add` writes the
/// reference into the config directory, which must never be the user's.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn clones_dir(&self) -> PathBuf {
        self.path().join("config").join("clones")
    }

    /// vox without HF_TOKEN, whatever the machine running the tests has set.
    fn vox(&self) -> assert_cmd::Command {
        let mut cmd = cargo_bin_cmd!("vox");
        cmd.env("VOX_CONFIG_DIR", self.path().join("config"))
            .env("VOX_DB_PATH", self.path().join("vox.db"))
            .env_remove("HF_TOKEN");
        cmd
    }

    /// Half a second of a 220 Hz tone, as a 16 kHz mono WAV.
    fn tone_wav(&self, file_name: &str) -> PathBuf {
        let path = self.path().join(file_name);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..8_000 {
            let t = i as f32 / 16_000.0;
            let sample = (t * 220.0 * std::f32::consts::TAU).sin() * 0.5;
            writer
                .write_sample((sample * i16::MAX as f32) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    fn add_clone(&self, name: &str) {
        let audio = self.tone_wav("reference.wav");
        self.vox()
            .args(["clone", "add", name, "--audio"])
            .arg(&audio)
            .assert()
            .success();
    }
}

fn fixture(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file_name)
}

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("vox")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Voice Command"));
}

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("vox")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("vox"));
}

#[test]
fn test_unknown_backend() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "nonexistent", "hello"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown backend"));
}

#[cfg(target_os = "macos")]
#[test]
fn test_list_voices_say() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "say", "--list-voices"])
        .assert()
        .success();
}

#[test]
fn test_list_voices_qwen_native() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["--backend", "qwen-native", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("voice clones"));
}

#[test]
fn test_no_text_no_stdin() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Text cannot be empty")
                .or(predicate::str::contains("No text provided")),
        );
}

/// Rendered to a file: without `-o` this test spoke through the speakers on
/// every run of the suite.
#[cfg(target_os = "macos")]
#[test]
fn test_stdin_pipe() {
    let sandbox = Sandbox::new();
    let out = sandbox.path().join("stdin.wav");
    sandbox
        .vox()
        .args(["--backend", "say", "-o"])
        .arg(&out)
        .write_stdin("Hello from stdin")
        .assert()
        .success();
    assert!(out.is_file());
}

// --- Config subcommand ---

#[test]
fn test_config_show() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backend:"))
        .stdout(predicate::str::contains("(default)"));
}

#[test]
fn test_config_set_and_show() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    // Set
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "fr"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang = fr"));

    // Show
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang:    fr"));
}

#[test]
fn test_config_set_invalid_key() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "invalid", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown preference"));
}

#[test]
fn test_config_reset() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    // Set then reset
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "set", "lang", "fr"])
        .assert()
        .success();

    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "reset"])
        .assert()
        .success()
        .stdout(predicate::str::contains("reset"));

    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("lang:    (default)"));
}

// --- Clone subcommand ---

#[test]
fn test_clone_list_empty() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No voice clones"));
}

#[test]
fn test_clone_add_missing_audio() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "add", "test", "--audio", "/nonexistent.wav"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_clone_add_and_list() {
    let sandbox = Sandbox::new();
    let audio = sandbox.tone_wav("voice.wav");

    // Add
    sandbox
        .vox()
        .args(["clone", "add", "testvoice", "--audio"])
        .arg(&audio)
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    // List
    sandbox
        .vox()
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("testvoice"));
}

#[test]
fn test_clone_remove() {
    let sandbox = Sandbox::new();

    // Add then remove
    sandbox.add_clone("todel");

    sandbox
        .vox()
        .args(["clone", "remove", "todel"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
}

/// Every format `clone add` accepts must end up as something the cloning
/// backends can read: both open the reference with a WAV reader, so an mp3
/// stored as given was accepted here and then failed at synthesis.
#[test]
fn clone_add_stores_every_accepted_format_as_a_wav() {
    let sandbox = Sandbox::new();
    let sources = [
        ("wav", sandbox.tone_wav("tone.wav")),
        ("mp3", fixture("tone.mp3")),
        ("flac", fixture("tone.flac")),
        ("ogg", fixture("tone.ogg")),
    ];

    for (format, source) in &sources {
        let name = format!("from_{format}");
        sandbox
            .vox()
            .args(["clone", "add", &name, "--audio"])
            .arg(source)
            .assert()
            .success();

        let stored = sandbox.clones_dir().join(format!("{name}.wav"));
        let mut reader = hound::WavReader::open(&stored)
            .unwrap_or_else(|e| panic!("{format}: no WAV at {}: {e}", stored.display()));
        let spec = reader.spec();
        assert_eq!(spec.channels, 1, "{format}: stored as mono");
        assert_eq!(spec.bits_per_sample, 16, "{format}");
        let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        let seconds = samples.len() as f32 / spec.sample_rate as f32;
        assert!(
            (0.4..0.7).contains(&seconds),
            "{format}: the half-second tone became {seconds}s"
        );
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        // The fixtures are ffmpeg's sine source, which peaks at an eighth of
        // full scale (4096).
        assert!(peak > 2_000, "{format}: decoded to near silence ({peak})");

        // The clone points at the stored WAV, not at the file it came from.
        sandbox
            .vox()
            .args(["clone", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                stored.to_string_lossy().to_string(),
            ));
    }
}

/// A path typed relative to the current directory used to be stored as typed,
/// so the clone only worked from that directory.
#[test]
fn clone_add_stores_an_absolute_path_for_a_relative_one() {
    let sandbox = Sandbox::new();
    sandbox.tone_wav("relative.wav");

    sandbox
        .vox()
        .current_dir(sandbox.path())
        .args(["clone", "add", "rel", "--audio", "relative.wav"])
        .assert()
        .success();

    let stored = sandbox.clones_dir().join("rel.wav");
    assert!(stored.is_absolute() && stored.exists());
    sandbox
        .vox()
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "rel: {}",
            stored.display()
        )));
}

/// Removing a clone removes the copy vox made of its reference, and never a
/// file that is not that copy.
#[test]
fn clone_remove_deletes_the_reference_vox_kept_and_nothing_else() {
    let sandbox = Sandbox::new();
    let original = sandbox.tone_wav("mine.wav");

    sandbox
        .vox()
        .args(["clone", "add", "gone", "--audio"])
        .arg(&original)
        .assert()
        .success();
    let kept = sandbox.clones_dir().join("gone.wav");
    assert!(kept.exists());

    sandbox
        .vox()
        .args(["clone", "remove", "gone"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Voice clone 'gone' removed."));
    assert!(!kept.exists(), "the reference outlived its clone");
    assert!(original.exists(), "the user's own file was deleted");

    // A second remove finds nothing, and says so.
    sandbox
        .vox()
        .args(["clone", "remove", "gone"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not found"));
}

/// An accepted extension on a file that holds no audio is refused when it is
/// added, and leaves neither a clone nor a file behind.
#[test]
fn clone_add_rejects_what_cannot_be_decoded() {
    let sandbox = Sandbox::new();
    let empty = sandbox.path().join("empty.wav");
    std::fs::write(&empty, b"").unwrap();
    let garbage = sandbox.path().join("garbage.mp3");
    std::fs::write(&garbage, b"this is not an mp3 file, only text").unwrap();

    for source in [&empty, &garbage] {
        sandbox
            .vox()
            .args(["clone", "add", "broken", "--audio"])
            .arg(source)
            .assert()
            .failure()
            .stderr(predicate::str::contains("Could not decode"));
    }

    sandbox
        .vox()
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No voice clones"));
    let leftovers = std::fs::read_dir(sandbox.clones_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        leftovers, 0,
        "a failed add left files in the clones directory"
    );
}

/// `.m4a` needs a decoder this build does not have: say so at `clone add`.
#[test]
fn clone_add_rejects_m4a_by_name() {
    let sandbox = Sandbox::new();
    let m4a = sandbox.path().join("voice.m4a");
    std::fs::write(&m4a, b"").unwrap();

    sandbox
        .vox()
        .args(["clone", "add", "aac", "--audio"])
        .arg(&m4a)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsupported audio format: .m4a"));
}

/// The reference is stored under the clone's name, so a second add under a
/// taken name must fail before it writes, not after.
#[test]
fn clone_add_under_a_taken_name_keeps_the_first_reference() {
    let sandbox = Sandbox::new();
    sandbox.add_clone("mine");
    let stored = sandbox.clones_dir().join("mine.wav");
    let before = std::fs::read(&stored).unwrap();

    sandbox
        .vox()
        .args(["clone", "add", "mine", "--audio"])
        .arg(fixture("tone.flac"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    assert_eq!(std::fs::read(&stored).unwrap(), before);
}

/// A name that differs only by case is the same file on a case-insensitive
/// file system: adding `Mine` replaced the reference of `mine`.
#[test]
fn clone_add_under_a_name_that_differs_by_case_keeps_the_first_reference() {
    let sandbox = Sandbox::new();
    sandbox.add_clone("mine");
    let stored = sandbox.clones_dir().join("mine.wav");
    let before = std::fs::read(&stored).unwrap();

    sandbox
        .vox()
        .args(["clone", "add", "Mine", "--audio"])
        .arg(fixture("tone.flac"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("'mine' already exists"));

    assert_eq!(std::fs::read(&stored).unwrap(), before);
    sandbox
        .vox()
        .args(["clone", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Mine:").not());
}

/// The name becomes a file name in the clones directory: a path as a name
/// would write outside it.
#[test]
fn clone_add_rejects_a_name_that_is_a_path() {
    let sandbox = Sandbox::new();
    let audio = sandbox.tone_wav("voice.wav");

    for name in ["../escape", "nested/name", ".."] {
        sandbox
            .vox()
            .args(["clone", "add", name, "--audio"])
            .arg(&audio)
            .assert()
            .failure()
            .stderr(predicate::str::contains("Invalid clone name"));
    }
    assert!(!sandbox.path().join("config").join("escape.wav").exists());
}

// --- Backend selection ---

/// `-b pocket` names the default backend, and used to be read as "no flag":
/// the language default (piper for French) then replaced it.
#[test]
fn explicit_default_backend_beats_the_language_default() {
    let sandbox = Sandbox::new();
    sandbox
        .vox()
        .args(["-b", "pocket", "-l", "fr", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alba"))
        .stdout(predicate::str::contains("fr_FR").not());

    // Without the flag, French still goes to piper.
    sandbox
        .vox()
        .args(["-l", "fr", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fr_FR"));
}

#[test]
fn explicit_default_backend_beats_the_stored_backend() {
    let sandbox = Sandbox::new();
    sandbox
        .vox()
        .args(["config", "set", "backend", "piper"])
        .assert()
        .success();

    sandbox
        .vox()
        .args(["-b", "pocket", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alba"));

    // Without the flag, the stored backend applies.
    sandbox
        .vox()
        .args(["--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("en_US"));
}

#[test]
fn help_still_names_the_default_backend() {
    cargo_bin_cmd!("vox")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("[default: pocket"));
}

/// A clone on the default backend used to stay on pocket, which cannot clone
/// without HF_TOKEN and its gated weights.
#[test]
fn clone_voice_goes_to_a_backend_that_can_clone() {
    let sandbox = Sandbox::new();
    sandbox.add_clone("me");
    let qwen_voices = "voice clones";

    // No token: pocket cannot clone, so qwen-native speaks.
    sandbox
        .vox()
        .args(["-v", "me", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains(qwen_voices));

    // A token: pocket can clone, and stays.
    sandbox
        .vox()
        .env("HF_TOKEN", "hf_test")
        .args(["-v", "me", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alba"));

    // A backend named on the command line is respected either way.
    sandbox
        .vox()
        .args(["-b", "pocket", "-v", "me", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alba"));
    sandbox
        .vox()
        .env("HF_TOKEN", "hf_test")
        .args(["-b", "qwen-native", "-v", "me", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains(qwen_voices));
}

/// A backend that cannot clone, named explicitly, is respected, and says that
/// the clone is not used rather than dropping it silently.
#[test]
fn explicit_backend_without_cloning_says_the_clone_is_ignored() {
    let sandbox = Sandbox::new();
    sandbox.add_clone("me");

    sandbox
        .vox()
        .args(["-b", "piper", "-v", "me", "--list-voices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("en_US"))
        .stderr(predicate::str::contains(
            "piper backend cannot clone voices",
        ));
}

// --- Flags no backend reads ---

#[test]
fn gender_and_style_say_they_have_no_effect() {
    let sandbox = Sandbox::new();
    let note = "--gender and --style have no effect yet";

    for flag in [["--gender", "feminine"], ["--style", "calm"]] {
        sandbox
            .vox()
            .args(flag)
            .arg("--list-voices")
            .assert()
            .success()
            .stderr(predicate::str::contains(note));
    }

    // Still accepted as preferences, and silent when neither flag is passed.
    sandbox
        .vox()
        .args(["config", "set", "style", "calm"])
        .assert()
        .success();
    sandbox
        .vox()
        .arg("--list-voices")
        .assert()
        .success()
        .stderr(predicate::str::contains(note).not());
}

#[test]
fn help_says_gender_and_style_have_no_effect() {
    let help = cargo_bin_cmd!("vox").arg("--help").output().unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    for flag in ["--gender", "--style"] {
        let line = help
            .lines()
            .find(|line| line.contains(flag))
            .unwrap_or_else(|| panic!("{flag} missing from --help"));
        assert!(line.contains("No effect yet"), "{line}");
    }

    cargo_bin_cmd!("vox")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "gender and style are accepted but have no effect yet",
        ));
}

#[test]
fn test_clone_remove_not_found() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["clone", "remove", "ghost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not found"));
}

// --- Stats subcommand ---

/// `vox stats` reports characters; four "é" are four characters, eight bytes.
/// `say -o` renders to a file without playing, so this needs no audio device.
#[cfg(target_os = "macos")]
#[test]
fn stats_count_characters_not_bytes() {
    let sandbox = Sandbox::new();
    sandbox
        .vox()
        .args(["-b", "say", "-o"])
        .arg(sandbox.path().join("out.aiff"))
        .arg("éééé")
        .assert()
        .success();

    sandbox
        .vox()
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total characters:   4"));
}

#[test]
fn test_stats_empty() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    cargo_bin_cmd!("vox")
        .env("VOX_DB_PATH", tmp.path())
        .args(["stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No usage recorded yet."));
}

// --- Init subcommand ---

/// `vox init -m cli` in `project`, with a home and a database of its own:
/// init reads the stored language, which must not be the user's.
fn init_in(project: &Path) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("vox");
    cmd.args(["init", "-m", "cli"])
        .current_dir(project)
        .env("HOME", project.join(".vox-test-home"))
        .env("VOX_CONFIG_DIR", project.join(".vox-test-config"));
    cmd
}

#[test]
fn test_init_creates_files() {
    let dir = tempfile::tempdir().unwrap();
    init_in(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("CLAUDE.md configured"))
        .stdout(predicate::str::contains("settings.json configured"));

    assert!(dir.path().join("CLAUDE.md").exists());
    assert!(dir.path().join(".claude/settings.json").exists());
}

#[test]
fn test_init_idempotent() {
    let dir = tempfile::tempdir().unwrap();

    // First run
    init_in(dir.path()).assert().success();

    // Second run
    init_in(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already configured"));
}

/// `vox init` (MCP mode) with an empty home of its own. Unix only: that is
/// where the home directory follows `HOME`.
#[cfg(unix)]
fn mcp_init_in(sandbox: &Sandbox) -> assert_cmd::Command {
    let mut cmd = sandbox.vox();
    cmd.arg("init")
        .current_dir(sandbox.path())
        .env("HOME", sandbox.path().join("home"));
    cmd
}

/// `vox init` used to create a configuration file for each of the 14 tools
/// and then ask to restart Claude Code and Claude Desktop, installed or not.
#[cfg(unix)]
#[test]
fn init_leaves_an_empty_home_empty_and_names_no_tool_to_restart() {
    let sandbox = Sandbox::new();
    let home = sandbox.path().join("home");
    std::fs::create_dir(&home).unwrap();

    let output = mcp_init_in(&sandbox).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(
        stdout.matches("not installed, skipped").count(),
        14,
        "{stdout}"
    );
    assert!(
        stdout.contains("No supported AI tool was found"),
        "{stdout}"
    );
    assert!(!stdout.contains("Restart "), "{stdout}");
    let left: Vec<_> = std::fs::read_dir(&home).unwrap().collect();
    assert!(left.is_empty(), "init created {left:?}");
}

#[cfg(unix)]
#[test]
fn init_configures_the_tool_it_finds_and_asks_to_restart_that_one() {
    let sandbox = Sandbox::new();
    let home = sandbox.path().join("home");
    std::fs::create_dir_all(home.join(".cursor")).unwrap();

    mcp_init_in(&sandbox)
        .assert()
        .success()
        .stdout(predicate::str::contains("Restart Cursor to activate."))
        .stdout(predicate::str::contains(
            "Claude Desktop       not installed, skipped",
        ));
    assert!(home.join(".cursor/mcp.json").is_file());
    assert!(!home.join(".claude.json").exists());

    mcp_init_in(&sandbox)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Cursor               already configured",
        ))
        .stdout(predicate::str::contains("Restart ").not());
}

// --- Bench subcommand ---

/// Only the help is run here: the command itself loads every model, and what
/// it does with them is covered by unit tests in `main.rs`.
#[test]
fn bench_help_says_it_stores_nothing_without_set() {
    cargo_bin_cmd!("vox")
        .args(["bench", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--set"))
        .stdout(predicate::str::contains("nothing is played"));
}

// --- Help subcommands ---

#[test]
fn test_clone_help() {
    cargo_bin_cmd!("vox")
        .args(["clone", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("voice clones"));
}

#[test]
fn test_config_help() {
    cargo_bin_cmd!("vox")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("preferences"));
}

/// The help of `vox config set` used to list seven of the nine keys. It is
/// now written from the list the setter checks against.
#[test]
fn config_help_names_every_key_the_setter_accepts() {
    for args in [
        ["config", "--help"].as_slice(),
        &["config", "set", "--help"],
    ] {
        let help = cargo_bin_cmd!("vox").args(args).output().unwrap();
        let help = String::from_utf8(help.stdout).unwrap();
        let line = help
            .lines()
            .find(|line| line.contains("Set a preference"))
            .unwrap_or_else(|| panic!("no `Set a preference` line in {help}"));
        for key in vox::db::PREFERENCE_KEYS {
            assert!(line.contains(key), "{args:?}: {key} missing from {line:?}");
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn chat_help_names_the_model_variable() {
    cargo_bin_cmd!("vox")
        .args(["chat", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VOX_CHAT_MODEL"))
        .stdout(predicate::str::contains("claude-sonnet-4-20250514").not());
}
