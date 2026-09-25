//! Pack names reach the filesystem: `packs_dir().join(name)`, and `remove`
//! follows that with `fs::remove_dir_all`. An unvalidated name escapes the
//! packs directory entirely, because `join` with an absolute path discards the
//! base and `..` is resolved by the OS.
//!
//! These tests assert the guard holds, and — for the destructive path — that a
//! directory outside the packs dir really is still there afterwards.

use vox::pack;

#[test]
fn rejects_path_traversal() {
    for name in [
        "../escape",
        "../../escape",
        "a/../../b",
        "sub/dir",
        "..",
        ".",
        "",
    ] {
        assert!(
            pack::validate_pack_name(name).is_err(),
            "must reject {name:?}"
        );
    }
}

#[test]
fn rejects_absolute_paths() {
    for name in ["/etc", "/Users/someone/Documents", "//tmp"] {
        assert!(
            pack::validate_pack_name(name).is_err(),
            "must reject {name:?}"
        );
    }
}

#[test]
fn rejects_separators_and_nul() {
    for name in ["a\\b", "a\0b", "pack/", "/pack"] {
        assert!(
            pack::validate_pack_name(name).is_err(),
            "must reject {name:?}"
        );
    }
}

#[test]
fn accepts_the_real_pack_names() {
    for name in pack::list_available() {
        assert!(
            pack::validate_pack_name(name).is_ok(),
            "must accept the shipped pack {name}"
        );
    }
    for name in ["my_pack", "pack-2", "Pack.v2"] {
        assert!(
            pack::validate_pack_name(name).is_ok(),
            "must accept {name:?}"
        );
    }
}

/// The finding that mattered: `vox pack remove ../../something` used to delete
/// a directory outside the packs dir. Prove the target survives.
#[test]
fn remove_cannot_delete_outside_the_packs_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let packs = tmp.path().join("packs");
    let victim = tmp.path().join("precious");
    std::fs::create_dir_all(packs.join("peon")).unwrap();
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("data.txt"), b"do not delete").unwrap();

    unsafe { std::env::set_var("VOX_CONFIG_DIR", tmp.path()) };

    // Sanity check that this test can actually fail: without the guard,
    // packs_dir().join("../precious") resolves to the victim directory.
    assert!(packs.join("../precious").join("data.txt").exists());

    for attack in ["../precious", "../../precious"] {
        assert!(
            pack::remove(attack).is_err(),
            "remove({attack:?}) must be rejected, not silently ignored"
        );
        assert!(
            victim.join("data.txt").exists(),
            "remove({attack:?}) destroyed a directory outside the packs dir"
        );
    }

    let abs = victim.to_string_lossy().to_string();
    assert!(
        pack::remove(&abs).is_err(),
        "absolute path must be rejected"
    );
    assert!(
        victim.join("data.txt").exists(),
        "remove(<absolute path>) destroyed the target"
    );

    // A legitimate name still works.
    assert!(pack::remove("peon").unwrap(), "must remove a real pack");
    assert!(!packs.join("peon").exists());

    unsafe { std::env::remove_var("VOX_CONFIG_DIR") };
}
