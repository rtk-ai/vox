use std::fs;

use vox::init;

#[test]
fn test_claude_md_block_contains_markers() {
    let block = init::claude_md_block(Some("en"));
    assert!(block.contains("<!-- vox:start -->"));
    assert!(block.contains("<!-- vox:end -->"));
    assert!(block.contains("vox -l en"));
}

#[test]
fn test_claude_md_has_vox_true() {
    let content = "Some text\n<!-- vox:start -->\nstuff\n<!-- vox:end -->\n";
    assert!(init::claude_md_has_vox(content));
}

#[test]
fn test_claude_md_has_vox_false() {
    let content = "# My project\nSome instructions.\n";
    assert!(!init::claude_md_has_vox(content));
}

#[test]
fn test_claude_md_has_vox_empty() {
    assert!(!init::claude_md_has_vox(""));
}

#[test]
fn test_build_settings_fresh() {
    let result = init::build_settings(None, Some("en")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(parsed["hooks"]["Stop"].is_array());
    let stop = &parsed["hooks"]["Stop"][0];
    assert_eq!(stop["hooks"][0]["command"], "vox -l en \"Done.\"");
}

#[test]
fn test_build_settings_merge_existing() {
    let existing = r#"{"permissions": {"allow": ["vox"]}}"#;
    let result = init::build_settings(Some(existing), Some("en")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    // Existing key preserved
    assert!(parsed["permissions"]["allow"].is_array());
    // Hook added
    assert!(parsed["hooks"]["Stop"].is_array());
    assert_eq!(
        parsed["hooks"]["Stop"][0]["hooks"][0]["command"],
        "vox -l en \"Done.\""
    );
}

#[test]
fn test_build_settings_idempotent() {
    // First build
    let first = init::build_settings(None, Some("en")).unwrap();
    // Second build with first as existing — should not duplicate
    let second = init::build_settings(Some(&first), Some("en")).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&second).unwrap();
    let stop = parsed["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1, "Should not duplicate the vox hook");
}

#[test]
fn test_has_vox_hook_true() {
    let settings: serde_json::Value = serde_json::from_str(
        r#"{"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"vox -b say \"done\""}]}]}}"#,
    ).unwrap();
    assert!(init::has_vox_hook(&settings));
}

#[test]
fn test_has_vox_hook_false() {
    let settings: serde_json::Value = serde_json::from_str(r#"{"hooks":{"Stop":[]}}"#).unwrap();
    assert!(!init::has_vox_hook(&settings));
}

#[test]
fn test_run_init_fresh_directory() {
    let dir = tempfile::tempdir().unwrap();
    let result = init::run_init(dir.path(), Some("en")).unwrap();

    assert!(result.claude_md_written);
    assert!(result.settings_written);

    // Verify files exist with correct content
    let claude_md = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("<!-- vox:start -->"));

    let settings = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert!(parsed["hooks"]["Stop"].is_array());
}

#[test]
fn test_run_init_idempotent() {
    let dir = tempfile::tempdir().unwrap();

    // First run
    let r1 = init::run_init(dir.path(), Some("en")).unwrap();
    assert!(r1.claude_md_written);
    assert!(r1.settings_written);

    let md_after_first = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();

    // Second run
    let r2 = init::run_init(dir.path(), Some("en")).unwrap();
    assert!(!r2.claude_md_written);
    assert!(!r2.settings_written);

    // Content unchanged
    let md_after_second = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    assert_eq!(md_after_first, md_after_second);
}

#[test]
fn test_run_init_appends_to_existing_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let claude_md_path = dir.path().join("CLAUDE.md");

    // Pre-existing content
    fs::write(&claude_md_path, "# My Project\n\nExisting instructions.\n").unwrap();

    let result = init::run_init(dir.path(), Some("en")).unwrap();
    assert!(result.claude_md_written);

    let content = fs::read_to_string(&claude_md_path).unwrap();
    assert!(content.starts_with("# My Project"));
    assert!(content.contains("Existing instructions."));
    assert!(content.contains("<!-- vox:start -->"));
    // Append uses the short form, not the full block
    assert!(!content.contains("## Voice feedback"));
}

#[test]
fn test_claude_md_append_block_is_short() {
    let block = init::claude_md_append_block(Some("en"));
    assert!(block.contains("<!-- vox:start -->"));
    assert!(block.contains("<!-- vox:end -->"));
    // Should be concise — no heading, no code fence
    assert!(!block.contains("## Voice feedback"));
    assert!(!block.contains("```bash"));
}

// ---------------------------------------------------------------------------
// Regression: `vox init` must not force the maintainer's language on users.
// See issue #63 ("not sure why I have french forced").
// ---------------------------------------------------------------------------

#[test]
fn templates_never_hardcode_french_for_other_languages() {
    for lang in ["en", "de", "ja", "es"] {
        let block = init::claude_md_block(Some(lang));
        let append = init::claude_md_append_block(Some(lang));
        let hook = init::stop_hook_command(Some(lang));
        for text in [&block, &append, &hook] {
            assert!(!text.contains("-l fr"), "{lang}: leaked `-l fr` in {text}");
            assert!(
                !text.contains("French"),
                "{lang}: leaked `French` in {text}"
            );
            assert!(
                !text.contains("Terminé"),
                "{lang}: leaked `Terminé` in {text}"
            );
        }
        assert!(
            block.contains(&format!("vox -l {lang}")),
            "{lang}: missing flag"
        );
        assert!(
            hook.contains(&format!("vox -l {lang}")),
            "{lang}: missing flag"
        );
    }
}

#[test]
fn templates_speak_the_requested_language() {
    assert!(init::claude_md_block(Some("de")).contains("in German"));
    assert!(init::claude_md_block(Some("de")).contains("Speak German."));
    assert!(init::claude_md_block(Some("ja")).contains("in Japanese"));
    // French still works when it is what the user actually wants.
    assert!(init::claude_md_block(Some("fr")).contains("in French"));
    assert_eq!(
        init::stop_hook_command(Some("fr")),
        r#"vox -l fr "Terminé.""#
    );
}

#[test]
fn no_language_means_follow_the_user() {
    let block = init::claude_md_block(None);
    assert!(!block.contains("-l "), "must not pass a language flag");
    assert!(block.contains("Match the language the user is writing in."));
    assert_eq!(init::stop_hook_command(None), r#"vox "Done.""#);
}

#[test]
fn stop_hook_in_settings_uses_the_requested_language() {
    let settings = init::build_settings(None, Some("de")).unwrap();
    assert!(settings.contains("vox -l de"));
    assert!(settings.contains("Fertig."));
    assert!(!settings.contains("Terminé"));
}

#[test]
fn run_init_writes_the_requested_language() {
    let dir = tempfile::tempdir().unwrap();
    init::run_init(dir.path(), Some("ja")).unwrap();
    let md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
    let settings = std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    assert!(md.contains("vox -l ja"));
    assert!(settings.contains("完了しました。"));
    assert!(!md.contains("French") && !settings.contains("Terminé"));
}
