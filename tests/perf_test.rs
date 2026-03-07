//! Performance tests — DB operations, bulk operations, response times.

use vox::db;

// ---------------------------------------------------------------------------
// DB open performance (should be <100ms even with migrations)
// ---------------------------------------------------------------------------

#[test]
fn db_open_is_fast() {
    let start = std::time::Instant::now();
    let _conn = db::open_in_memory().unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "DB open took {}ms, expected <100ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Bulk usage logging (should handle 1000 inserts in <1s)
// ---------------------------------------------------------------------------

#[test]
fn bulk_usage_log_performance() {
    let conn = db::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    for i in 0..1000 {
        db::log_usage(
            &conn,
            if i % 3 == 0 { "say" } else { "kokoro" },
            Some("default"),
            Some(if i % 2 == 0 { "fr" } else { "en" }),
            50 + (i % 200),
            Some(500 + (i as u64 % 3000)),
        )
        .unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "1000 inserts took {}ms, expected <1000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Stats query performance after bulk data
// ---------------------------------------------------------------------------

#[test]
fn stats_query_fast_after_bulk_data() {
    let conn = db::open_in_memory().unwrap();

    // Seed 500 entries across multiple backends and languages
    for i in 0..500 {
        let backend = match i % 4 {
            0 => "say",
            1 => "kokoro",
            2 => "qwen",
            _ => "qwen-native",
        };
        let lang = match i % 5 {
            0 => "fr",
            1 => "en",
            2 => "ja",
            3 => "de",
            _ => "es",
        };
        db::log_usage(&conn, backend, None, Some(lang), 100, Some(1000)).unwrap();
    }

    // Summary query
    let start = std::time::Instant::now();
    let (count, total_chars) = db::get_usage_summary(&conn).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(count, 500);
    assert_eq!(total_chars, 50000);
    assert!(
        elapsed.as_millis() < 50,
        "Summary query took {}ms, expected <50ms",
        elapsed.as_millis()
    );

    // Backend stats aggregation
    let start = std::time::Instant::now();
    let backend_stats = db::get_backend_stats(&conn).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(backend_stats.len(), 4);
    assert!(
        elapsed.as_millis() < 50,
        "Backend stats took {}ms, expected <50ms",
        elapsed.as_millis()
    );

    // Language stats aggregation
    let start = std::time::Instant::now();
    let lang_stats = db::get_lang_stats(&conn).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(lang_stats.len(), 5);
    assert!(
        elapsed.as_millis() < 50,
        "Lang stats took {}ms, expected <50ms",
        elapsed.as_millis()
    );

    // Total duration
    let start = std::time::Instant::now();
    let total_ms = db::get_total_duration_ms(&conn).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(total_ms, 500_000);
    assert!(
        elapsed.as_millis() < 50,
        "Total duration took {}ms, expected <50ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Preference read/write cycle performance
// ---------------------------------------------------------------------------

#[test]
fn preference_read_write_cycle_fast() {
    let conn = db::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        db::set_preference(&conn, "voice", "test_voice").unwrap();
        db::get_preferences(&conn).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "100 preference cycles took {}ms, expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Clone operations performance
// ---------------------------------------------------------------------------

#[test]
fn bulk_clone_operations() {
    let conn = db::open_in_memory().unwrap();

    // Add 50 clones
    let start = std::time::Instant::now();
    for i in 0..50 {
        db::add_clone(
            &conn,
            &format!("clone_{i}"),
            &format!("/tmp/audio_{i}.wav"),
            Some(&format!("Reference text {i}")),
        )
        .unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "50 clone inserts took {}ms, expected <500ms",
        elapsed.as_millis()
    );

    // List all clones
    let start = std::time::Instant::now();
    let clones = db::list_clones(&conn).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(clones.len(), 50);
    assert!(
        elapsed.as_millis() < 50,
        "List 50 clones took {}ms, expected <50ms",
        elapsed.as_millis()
    );

    // Lookup single clone
    let start = std::time::Instant::now();
    for i in 0..50 {
        db::get_clone(&conn, &format!("clone_{i}")).unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "50 clone lookups took {}ms, expected <100ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Usage stats query with no data (no crash, fast)
// ---------------------------------------------------------------------------

#[test]
fn stats_on_empty_db_is_fast() {
    let conn = db::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    let (count, chars) = db::get_usage_summary(&conn).unwrap();
    let backend = db::get_backend_stats(&conn).unwrap();
    let lang = db::get_lang_stats(&conn).unwrap();
    let dur = db::get_total_duration_ms(&conn).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(count, 0);
    assert_eq!(chars, 0);
    assert!(backend.is_empty());
    assert!(lang.is_empty());
    assert_eq!(dur, 0);
    assert!(
        elapsed.as_millis() < 50,
        "Empty stats took {}ms, expected <50ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Clone resolution speed with many clones
// ---------------------------------------------------------------------------

#[test]
fn clone_resolution_fast_with_200_clones() {
    let conn = db::open_in_memory().unwrap();
    for i in 0..200 {
        db::add_clone(
            &conn,
            &format!("voice_{i:04}"),
            &format!("/tmp/audio_{i}.wav"),
            Some(&format!("Reference text for clone {i}")),
        )
        .unwrap();
    }

    // Resolve the last clone (worst case for sequential scan)
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let result = db::get_clone(&conn, "voice_0199").unwrap();
        assert!(result.is_some());
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 200,
        "100 clone resolutions among 200 clones took {}ms, expected <200ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Config read/write cycle under load
// ---------------------------------------------------------------------------

#[test]
fn config_read_write_rapid_cycling_all_keys() {
    let conn = db::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    for i in 0..50 {
        db::set_preference(&conn, "voice", &format!("voice_{i}")).unwrap();
        db::set_preference(&conn, "lang", "fr").unwrap();
        db::set_preference(&conn, "gender", "feminine").unwrap();
        db::set_preference(&conn, "style", "calm").unwrap();
        let prefs = db::get_preferences(&conn).unwrap();
        assert_eq!(prefs.voice.as_deref(), Some(format!("voice_{i}").as_str()));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "50 full config cycles took {}ms, expected <1000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Sentence splitting performance on large text
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn sentence_splitting_performance_large_text() {
    use vox::chat::sentence::SentenceAccumulator;

    // Build a large text (~100KB) with many sentences
    let sentence = format!("{}. ", "A".repeat(150));
    let large_text = sentence.repeat(500); // ~75KB

    let start = std::time::Instant::now();
    let mut acc = SentenceAccumulator::new();
    let sentences = acc.push(&large_text);
    let _remaining = acc.flush();
    let elapsed = start.elapsed();

    assert!(
        sentences.len() >= 400,
        "Expected >=400 sentences, got {}",
        sentences.len()
    );
    assert!(
        elapsed.as_millis() < 100,
        "Sentence splitting of ~75KB took {}ms, expected <100ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// STT command building performance
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn stt_command_building_performance() {
    let start = std::time::Instant::now();
    for i in 0..1000 {
        let path = format!("/tmp/audio_{i}.wav");
        let _cmd = vox::stt::build_transcribe_command(&path, Some("fr"));
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 200,
        "1000 STT command builds took {}ms, expected <200ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Multiple sequential DB operations (mixed workload)
// ---------------------------------------------------------------------------

#[test]
fn mixed_db_operations_performance() {
    let conn = db::open_in_memory().unwrap();

    let start = std::time::Instant::now();

    // Interleave clones, preferences, usage logs, and queries
    for i in 0..100 {
        db::add_clone(
            &conn,
            &format!("perf_clone_{i}"),
            &format!("/tmp/audio_{i}.wav"),
            Some("ref text"),
        )
        .unwrap();
        db::log_usage(&conn, "kokoro", Some("default"), Some("en"), 100, Some(500)).unwrap();
        db::set_preference(&conn, "voice", &format!("v{i}")).unwrap();
        let _ = db::get_preferences(&conn).unwrap();
        let _ = db::get_clone(&conn, &format!("perf_clone_{i}")).unwrap();
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "500 mixed DB ops took {}ms, expected <2000ms",
        elapsed.as_millis()
    );

    // Verify data integrity after mixed workload
    let clones = db::list_clones(&conn).unwrap();
    assert_eq!(clones.len(), 100);
    let (count, _) = db::get_usage_summary(&conn).unwrap();
    assert_eq!(count, 100);
}

// ---------------------------------------------------------------------------
// Repeated DB open_in_memory (schema migration) performance
// ---------------------------------------------------------------------------

#[test]
fn repeated_db_open_performance() {
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let _conn = db::open_in_memory().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "100 DB opens took {}ms, expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Remove clone performance with many entries
// ---------------------------------------------------------------------------

#[test]
fn remove_clone_performance_with_many_entries() {
    let conn = db::open_in_memory().unwrap();
    for i in 0..200 {
        db::add_clone(
            &conn,
            &format!("del_clone_{i}"),
            &format!("/tmp/audio_{i}.wav"),
            None,
        )
        .unwrap();
    }

    let start = std::time::Instant::now();
    for i in 0..200 {
        let removed = db::remove_clone(&conn, &format!("del_clone_{i}")).unwrap();
        assert!(removed);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "200 clone removals took {}ms, expected <500ms",
        elapsed.as_millis()
    );

    let clones = db::list_clones(&conn).unwrap();
    assert!(clones.is_empty());
}
