use std::sync::Mutex;

use vox::pack;

/// VOX_CONFIG_DIR belongs to the whole process, and tests run in parallel:
/// the tests that set it take this lock first.
static CONFIG_DIR: Mutex<()> = Mutex::new(());

/// Downloads a real pack, so it needs the network: run it with
/// `cargo test --features metal --test pack_test -- --ignored`.
#[test]
#[ignore = "network: downloads a pack from the registry"]
fn installs_a_real_pack_from_the_registry() {
    let _lock = CONFIG_DIR.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("VOX_CONFIG_DIR", tmp.path()) };

    // What an install killed while it downloaded leaves behind: sounds and no
    // manifest. It is not a pack, and installing again must replace it.
    let unfinished = tmp.path().join("packs/peon/sounds");
    std::fs::create_dir_all(&unfinished).unwrap();
    std::fs::write(unfinished.join("left_behind.wav"), b"RIFF").unwrap();
    assert!(pack::list_installed().unwrap().is_empty());

    pack::install("peon").expect("peon must install over an unfinished install");
    assert!(!unfinished.join("left_behind.wav").exists());

    let again = pack::install("peon").unwrap_err().to_string();
    assert!(again.contains("already installed"), "{again}");

    let installed = pack::list_installed().unwrap();
    assert_eq!(installed.len(), 1);
    let manifest = &installed[0];
    assert_eq!(manifest.name, "peon");
    let greeting = &manifest.categories["greeting"].sounds;
    assert!(!greeting.is_empty());

    let sounds = tmp.path().join("packs/peon/sounds");
    for category in manifest.categories.values() {
        for sound in &category.sounds {
            let bytes = std::fs::read(sounds.join(&sound.file)).unwrap();
            assert_eq!(&bytes[..4], b"RIFF", "{} is not a WAV file", sound.file);
        }
    }

    // What `vox pack play greeting` would play is one of those files.
    let (path, line) = pack::pick_sound("peon", Some("greeting")).unwrap();
    assert!(path.is_file(), "{} is missing", path.display());
    assert!(!line.is_empty());

    unsafe { std::env::remove_var("VOX_CONFIG_DIR") };
}

// Both fixtures are copied from the live registry and from the `peon` pack it
// points at (PeonPing/og-packs, tag v1.0.0), cut down to a few entries.
const REGISTRY: &str = include_str!("fixtures/pack_registry.json");
const OPENPEON: &str = include_str!("fixtures/openpeon_peon.json");

#[test]
fn resolves_a_pack_kept_in_a_directory_of_a_shared_repository() {
    assert_eq!(
        pack::resolve_pack_url(REGISTRY, "peon").unwrap(),
        "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon"
    );
}

#[test]
fn resolves_a_pack_alone_in_its_repository() {
    // Its registry entry has "." for a path.
    assert_eq!(
        pack::resolve_pack_url(REGISTRY, "abbot").unwrap(),
        "https://raw.githubusercontent.com/openpeon-stronghold/abbot/v1.2.1"
    );
}

#[test]
fn an_unknown_pack_is_named_in_the_error() {
    let err = pack::resolve_pack_url(REGISTRY, "no_such_pack")
        .unwrap_err()
        .to_string();
    assert!(err.contains("Unknown pack 'no_such_pack'"), "{err}");
    assert!(err.contains("openpeon.com/packs"), "{err}");
}

#[test]
fn a_registry_entry_cannot_point_outside_github() {
    for (repo, git_ref, path) in [
        ("evil.example/x/y", "v1", "."),
        ("owner/repo", "../../other/repo/main", "."),
        ("owner/repo", "v1", "../../other"),
        ("owner/repo?x=", "v1", "."),
        ("", "v1", "."),
        ("owner/repo", "", "."),
    ] {
        let registry = serde_json::json!({ "packs": [{
            "name": "p", "source_repo": repo, "source_ref": git_ref, "source_path": path,
        }]});
        assert!(
            pack::resolve_pack_url(&registry.to_string(), "p").is_err(),
            "must reject {repo:?} {git_ref:?} {path:?}"
        );
    }
}

#[test]
fn an_openpeon_manifest_becomes_the_manifest_vox_stores() {
    let (manifest, files) = pack::from_openpeon("peon", OPENPEON).unwrap();

    assert_eq!(manifest.name, "peon");
    assert_eq!(manifest.display_name, "Orc Peon");

    // The categories keep the names `vox pack play` has always taken.
    let mut categories: Vec<&str> = manifest.categories.keys().map(String::as_str).collect();
    categories.sort_unstable();
    assert_eq!(categories, ["annoyed", "complete", "greeting"]);

    let greeting = &manifest.categories["greeting"].sounds;
    assert_eq!(greeting.len(), 3);
    assert_eq!(greeting[0].file, "PeonReady1.wav");
    assert_eq!(greeting[0].line, "Ready to work?");

    // PeonReady1.wav is in two categories and is downloaded once.
    assert_eq!(files.len(), 12);
    assert_eq!(files["PeonReady1.wav"], "PeonReady1.wav");
}

/// What `install` writes must read back as a pack: it is the format that
/// versions before the registry wrote, and that they still read.
#[test]
fn the_stored_manifest_reads_back() {
    let (manifest, _) = pack::from_openpeon("peon", OPENPEON).unwrap();
    let stored = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        stored["categories"]["greeting"]["sounds"][0],
        serde_json::json!({ "file": "PeonReady1.wav", "line": "Ready to work?" })
    );
    let read: pack::PackManifest = serde_json::from_value(stored).unwrap();
    assert_eq!(read.categories.len(), 3);
}

fn openpeon_with(name: &str, category: &str, files: &[&str]) -> String {
    let sounds: Vec<_> = files
        .iter()
        .map(|file| serde_json::json!({ "file": file, "label": "a line" }))
        .collect();
    serde_json::json!({
        "cesp_version": "1.0",
        "name": name,
        "display_name": "A pack",
        "version": "1.0.0",
        "categories": { category: { "sounds": sounds } },
    })
    .to_string()
}

/// Shapes found in published packs that the specification does not allow:
/// bare file names (anchorman_brick), "./sounds/" (yasuo-pack), sub-directories
/// (gabriel-ultrakill) and punctuation (ccg_us_dozer).
#[test]
fn file_paths_of_published_packs_are_understood() {
    let json = openpeon_with(
        "p",
        "session.start",
        &[
            "loud_noises.mp3",
            "./sounds/Choryon.wav",
            "sounds/start/machine.mp3",
            "sounds/New_construction?.mp3",
        ],
    );
    let (manifest, files) = pack::from_openpeon("p", &json).unwrap();

    let stored: Vec<&str> = manifest.categories["greeting"]
        .sounds
        .iter()
        .map(|sound| sound.file.as_str())
        .collect();
    assert_eq!(
        stored,
        [
            "loud_noises.mp3",
            "Choryon.wav",
            "start_machine.mp3",
            "New_construction?.mp3"
        ]
    );
    // A sub-directory is kept in the path to download, not on disk.
    assert_eq!(files["start_machine.mp3"], "start/machine.mp3");
}

#[test]
fn the_registry_name_wins_over_the_manifest_name() {
    // arc_raiders calls itself "arc-raiders" in its manifest; the directory,
    // `vox pack set` and `vox pack list` all go by the registry name.
    let json = openpeon_with("arc-raiders", "task.error", &["sounds/a.mp3"]);
    let (manifest, _) = pack::from_openpeon("arc_raiders", &json).unwrap();
    assert_eq!(manifest.name, "arc_raiders");
}

#[test]
fn a_category_vox_has_no_name_for_keeps_its_own() {
    let json = openpeon_with("p", "session.end", &["sounds/bye.mp3"]);
    let (manifest, _) = pack::from_openpeon("p", &json).unwrap();
    assert!(manifest.categories.contains_key("session.end"));
}

#[test]
fn a_manifest_cannot_write_outside_the_sounds_directory() {
    for file in [
        "sounds/../../escape.wav",
        "sounds/..",
        "sounds/a\\..\\b.wav",
        "sounds//a.wav",
        "sounds/",
    ] {
        let json = openpeon_with("p", "session.start", &[file]);
        assert!(
            pack::from_openpeon("p", &json).is_err(),
            "must reject {file:?}"
        );
    }
}

#[test]
fn a_manifest_without_sounds_is_refused() {
    let json = openpeon_with("p", "session.start", &[]);
    let err = pack::from_openpeon("p", &json).unwrap_err().to_string();
    assert!(err.contains("has no sounds"), "{err}");
}

/// stellaris_evil_corp_advisor publishes its manifest with one.
#[test]
fn a_manifest_that_starts_with_a_byte_order_mark_is_read() {
    let json = format!(
        "\u{feff}{}",
        openpeon_with("p", "session.start", &["sounds/a.mp3"])
    );
    let (manifest, files) = pack::from_openpeon("p", &json).unwrap();
    assert_eq!(manifest.categories["greeting"].sounds[0].file, "a.mp3");
    assert_eq!(files.len(), 1);
}

/// A pack installed before the registry existed: the head of the `peon`
/// manifest as tonyyont/peon-ping published it.
const LEGACY_MANIFEST: &str = r#"{
      "name": "peon",
      "display_name": "Orc Peon",
      "source_url": "https://sounds.spriters-resource.com/media/assets/422/425494.zip",
      "source_subfolder": "Orc/Peon",
      "categories": {
        "greeting": {
          "sounds": [
            { "file": "PeonReady1.wav", "line": "Ready to work?" },
            { "file": "PeonWhat1.wav", "line": "Yes?" }
          ]
        }
      }
    }"#;

#[test]
fn a_manifest_written_by_an_older_version_still_loads() {
    let manifest: pack::PackManifest = serde_json::from_str(LEGACY_MANIFEST).unwrap();
    assert_eq!(manifest.categories["greeting"].sounds[1].line, "Yes?");
}

/// The directory an older version left on disk is still a pack: it is listed,
/// and a category resolves to one of its files.
#[test]
fn a_pack_installed_by_an_older_version_is_listed_and_resolves_a_sound() {
    let _lock = CONFIG_DIR.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let sounds = tmp.path().join("packs/peon/sounds");
    std::fs::create_dir_all(&sounds).unwrap();
    std::fs::write(tmp.path().join("packs/peon/manifest.json"), LEGACY_MANIFEST).unwrap();
    for file in ["PeonReady1.wav", "PeonWhat1.wav"] {
        std::fs::write(sounds.join(file), b"RIFF").unwrap();
    }
    // A directory with sounds and no manifest is an install that never ended.
    std::fs::create_dir_all(tmp.path().join("packs/unfinished/sounds")).unwrap();

    unsafe { std::env::set_var("VOX_CONFIG_DIR", tmp.path()) };
    let installed = pack::list_installed();
    let picked = pack::pick_sound("peon", None);
    let unfinished = pack::load_manifest("unfinished");
    unsafe { std::env::remove_var("VOX_CONFIG_DIR") };

    let names: Vec<String> = installed.unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["peon"]);
    let (path, line) = picked.unwrap();
    assert!(
        path.starts_with(&sounds) && path.is_file(),
        "{}",
        path.display()
    );
    assert!(
        ["Ready to work?", "Yes?"].contains(&line.as_str()),
        "{line}"
    );
    assert!(unfinished.is_err());
}
