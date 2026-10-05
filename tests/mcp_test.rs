//! The MCP server (`vox serve`) driven over stdio, the way an agent drives it.
//!
//! Only tools that produce no sound are called here: `vox_speak` needs an
//! audio device, so what it resolves to is covered by unit tests in `mcp.rs`.

use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};

/// A config directory and database of the test's own.
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

    fn vox(&self) -> assert_cmd::Command {
        let mut cmd = cargo_bin_cmd!("vox");
        cmd.env("VOX_CONFIG_DIR", self.path().join("config"))
            .env("VOX_DB_PATH", self.path().join("vox.db"))
            .env_remove("HF_TOKEN");
        cmd
    }

    fn config_set(&self, key: &str, value: &str) {
        self.vox()
            .args(["config", "set", key, value])
            .assert()
            .success();
    }

    /// Send one request to a fresh server and return its `result`.
    fn request(&self, method: &str, params: Value) -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let output = self
            .vox()
            .arg("serve")
            .write_stdin(format!("{request}\n"))
            .output()
            .unwrap();
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        response["result"].clone()
    }

    /// Call a tool and return its text, and whether it reported an error.
    fn call(&self, tool: &str, arguments: Value) -> (String, bool) {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        (
            result["content"][0]["text"].as_str().unwrap().to_string(),
            result["isError"].as_bool().unwrap_or(false),
        )
    }
}

/// An agent takes the descriptions literally: a standard build has no kokoro
/// backend, and used to advertise it as the fastest one.
#[test]
fn tool_descriptions_name_only_the_backends_of_this_build() {
    let tools = Sandbox::new().request("tools/list", json!({}));
    let tools = tools["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 14);

    for tool in ["vox_speak", "vox_list_voices"] {
        let definition = tools.iter().find(|t| t["name"] == tool).unwrap();
        let description = definition["inputSchema"]["properties"]["backend"]["description"]
            .as_str()
            .unwrap();
        assert_eq!(
            description.contains("kokoro"),
            cfg!(feature = "kokoro"),
            "{tool}: {description}"
        );
        assert_eq!(
            description.contains("say"),
            cfg!(target_os = "macos"),
            "{tool}: {description}"
        );
        for backend in ["pocket", "piper", "qwen-native"] {
            assert!(description.contains(backend), "{tool}: {description}");
        }
    }
}

/// `gender` and `style` are read by no backend, and the descriptions say so.
#[test]
fn gender_and_style_are_described_as_having_no_effect() {
    let tools = Sandbox::new().request("tools/list", json!({}));
    let tools = tools["tools"].as_array().unwrap();
    let speak = tools.iter().find(|t| t["name"] == "vox_speak").unwrap();
    for parameter in ["gender", "style"] {
        let description = speak["inputSchema"]["properties"][parameter]["description"]
            .as_str()
            .unwrap();
        assert!(
            description.contains("No effect yet"),
            "{parameter}: {description}"
        );
    }
    assert!(!speak["description"].as_str().unwrap().contains("styles"));

    let set = tools
        .iter()
        .find(|t| t["name"] == "vox_config_set")
        .unwrap();
    let keys = set["inputSchema"]["properties"]["key"]["description"]
        .as_str()
        .unwrap();
    assert!(keys.contains("gender and style are accepted but have no effect yet"));
    // Written from the list the setter checks against: it used to name seven
    // of the nine keys.
    for key in vox::db::PREFERENCE_KEYS {
        assert!(keys.contains(key), "{key} missing from {keys:?}");
    }
}

/// `vox_config_show` and `vox config show` print the same lines: the tool
/// used to leave `stt_model` out.
#[test]
fn config_show_matches_the_command_line() {
    let sandbox = Sandbox::new();
    sandbox.config_set("stt_model", "openai/whisper-tiny");
    sandbox.config_set("lang", "fr");

    let (text, is_error) = sandbox.call("vox_config_show", json!({}));
    assert!(!is_error);
    assert!(text.contains("stt_model: openai/whisper-tiny"), "{text}");

    let cli = sandbox.vox().args(["config", "show"]).output().unwrap();
    assert_eq!(
        text.trim_end(),
        String::from_utf8(cli.stdout).unwrap().trim_end()
    );
}

/// Without an argument, `vox_list_voices` lists the voices of the backend
/// `vox_speak` would use, not always pocket's.
#[test]
fn list_voices_follows_the_stored_backend_and_language() {
    let sandbox = Sandbox::new();
    let (text, _) = sandbox.call("vox_list_voices", json!({}));
    assert!(text.contains("alba"), "fresh database, pocket: {text}");

    sandbox.config_set("lang", "fr");
    let (text, _) = sandbox.call("vox_list_voices", json!({}));
    assert!(text.contains("fr_FR"), "French defaults to piper: {text}");

    sandbox.config_set("backend", "qwen-native");
    let (text, _) = sandbox.call("vox_list_voices", json!({}));
    assert!(text.contains("voice clones"), "stored backend: {text}");

    // An argument still wins over both.
    let (text, _) = sandbox.call("vox_list_voices", json!({"backend": "pocket"}));
    assert!(text.contains("alba"), "{text}");
}

/// A stored voice that is a clone moves `vox_speak` to a backend that can
/// clone, so `vox_list_voices` has to follow: it listed pocket's voices.
#[test]
fn list_voices_follows_a_stored_clone_voice() {
    let sandbox = Sandbox::new();
    let mp3 = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.mp3");
    let (text, is_error) = sandbox.call("vox_clone_add", json!({"name": "me", "audio": mp3}));
    assert!(!is_error, "{text}");
    sandbox.config_set("voice", "me");

    // No HF_TOKEN in the sandbox: pocket cannot clone, qwen-native speaks.
    let (text, _) = sandbox.call("vox_list_voices", json!({}));
    assert!(text.contains("voice clones"), "{text}");
    let cli = sandbox.vox().arg("--list-voices").output().unwrap();
    assert_eq!(
        text.trim_end(),
        String::from_utf8(cli.stdout).unwrap().trim_end()
    );
}

/// `vox_clone_add` stores the reference the way `vox clone add` does: as a
/// WAV in the clones directory, whatever was given.
#[test]
fn clone_add_converts_the_reference_like_the_command_line() {
    let sandbox = Sandbox::new();
    let mp3 = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.mp3");

    let (text, is_error) = sandbox.call("vox_clone_add", json!({"name": "agent", "audio": mp3}));
    assert!(!is_error, "{text}");

    let stored = sandbox.path().join("config/clones/agent.wav");
    let reader = hound::WavReader::open(&stored).unwrap();
    assert_eq!(reader.spec().channels, 1);
    assert!(reader.duration() > 0);
    let (list, _) = sandbox.call("vox_clone_list", json!({}));
    assert!(list.contains(&*stored.to_string_lossy()), "{list}");

    // A name that is a path would be a file write outside the clones directory.
    let (text, is_error) = sandbox.call(
        "vox_clone_add",
        json!({"name": "../../agent", "audio": mp3}),
    );
    assert!(is_error && text.contains("Invalid clone name"), "{text}");
    assert!(!sandbox.path().join("agent.wav").exists());
}
