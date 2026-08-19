use std::path::PathBuf;

use mimofan_memory::user_profile::{
    distill_from_transcript, render_for_injection, Bucket, ProfileEntry, SCHEMA_VERSION,
    UserProfile,
};

fn tmp_path() -> PathBuf {
    std::env::temp_dir().join(format!("mimofan_up_test_{}", uuid::Uuid::new_v4()))
}

#[test]
fn empty_profile_is_versioned_and_empty() {
    let p = UserProfile::empty();
    assert_eq!(p.version, SCHEMA_VERSION);
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
}

#[test]
fn save_then_load_roundtrips() {
    let path = tmp_path();
    let mut p = UserProfile::empty();
    p.apply_correction(
        Bucket::Languages,
        ProfileEntry::new("rust", "fluent in Rust"),
    );
    p.apply_correction(
        Bucket::Dislikes,
        ProfileEntry::new("no_db_mock", "don't mock the database"),
    );
    p.save(&path).expect("save ok");

    let loaded = UserProfile::load(&path);
    assert_eq!(loaded.languages.len(), 1);
    assert_eq!(loaded.languages[0].value, "fluent in Rust");
    assert_eq!(loaded.dislikes[0].tag, "no_db_mock");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn load_missing_file_returns_empty() {
    let path =
        std::env::temp_dir().join(format!("mimofan_up_missing_{}", uuid::Uuid::new_v4()));
    let p = UserProfile::load(&path);
    assert!(p.is_empty());
    assert_eq!(p.version, SCHEMA_VERSION);
}

#[test]
fn correction_replaces_not_appends() {
    let mut p = UserProfile::empty();
    p.apply_correction(
        Bucket::Preferences,
        ProfileEntry::new("response_length", "verbose"),
    );
    p.apply_correction(
        Bucket::Preferences,
        ProfileEntry::new("response_length", "concise"),
    );
    assert_eq!(
        p.preferences.len(),
        1,
        "correction must replace, not append"
    );
    assert_eq!(p.preferences[0].value, "concise");
}

#[test]
fn distinct_tags_append() {
    let mut p = UserProfile::empty();
    p.apply_correction(
        Bucket::Preferences,
        ProfileEntry::new("response_length", "concise"),
    );
    p.apply_correction(
        Bucket::Preferences,
        ProfileEntry::new("explain", "explain tradeoffs"),
    );
    assert_eq!(p.preferences.len(), 2);
}

#[test]
fn render_empty_profile_is_empty() {
    assert_eq!(render_for_injection(&UserProfile::empty(), 1000), "");
}

#[test]
fn render_includes_buckets_and_respects_budget() {
    let mut p = UserProfile::empty();
    p.apply_correction(
        Bucket::Languages,
        ProfileEntry::new("rust", "fluent in Rust"),
    );
    p.apply_correction(
        Bucket::Dislikes,
        ProfileEntry::new("no_db_mock", "don't mock the database"),
    );
    let rendered = render_for_injection(&p, 1000);
    assert!(rendered.contains("Languages"));
    assert!(rendered.contains("fluent in Rust"));
    assert!(rendered.contains("Hard Constraints"));

    let tiny = render_for_injection(&p, 20);
    let marker = "…(truncated)";
    assert!(tiny.len() <= 20 + marker.len(), "got len {}", tiny.len());
    assert!(tiny.contains(marker), "budget must truncate");
}

#[test]
fn distill_extracts_language_and_constraint() {
    let turns = vec![
        "I'm using Rust for this project".to_string(),
        "We don't mock the database in tests".to_string(),
        "Please prefer concise answers".to_string(),
    ];
    let distilled = distill_from_transcript(&turns);
    let has_rust = distilled
        .iter()
        .any(|(b, e)| *b == Bucket::Languages && e.value.contains("rust"));
    let has_constraint = distilled
        .iter()
        .any(|(b, e)| *b == Bucket::Dislikes && e.value.contains("third-party"));
    assert!(has_rust, "language distilled");
    assert!(has_constraint, "hard constraint distilled");
}

#[test]
fn distill_empty_on_no_signal() {
    let turns = vec!["hello".to_string(), "thanks".to_string()];
    assert!(distill_from_transcript(&turns).is_empty());
}

// ---- #831 用户画像免衰减 ----

#[test]
fn default_profile_is_decayable() {
    let p = UserProfile::empty();
    assert!(!p.decay_exempt, "profiles decay by default");
}

#[test]
fn exempt_constructor_and_setter() {
    let mut p = UserProfile::exempt();
    assert!(p.decay_exempt, "exempt() flags the profile");
    p.set_decay_exempt(false);
    assert!(!p.decay_exempt);
    p.set_decay_exempt(true);
    assert!(p.decay_exempt);
}

#[test]
fn decay_exempt_round_trips_through_json() {
    let path = std::env::temp_dir().join(format!("mimofan_up_exempt_{}", uuid::Uuid::new_v4()));
    let mut p = UserProfile::exempt();
    p.apply_correction(
        Bucket::Preferences,
        ProfileEntry::new("response_length", "concise"),
    );
    p.save(&path).expect("save ok");
    let loaded = UserProfile::load(&path);
    assert!(loaded.decay_exempt, "exemption survives persist/load");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn legacy_profile_without_flag_defaults_to_decayable() {
    // An older profile JSON that predates the `decay_exempt` field must
    // deserialize with `decay_exempt == false` (serde default), never error.
    let json = r#"{"version":1,"preferences":[{"tag":"x","value":"y"}]}"#;
    let p: UserProfile = serde_json::from_str(json).expect("legacy json parses");
    assert!(!p.decay_exempt);
    assert_eq!(p.preferences.len(), 1);
}
