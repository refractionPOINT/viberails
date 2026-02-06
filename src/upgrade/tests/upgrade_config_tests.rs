use std::{
    fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tempfile::TempDir;

use super::super::UpgradeConfig;

#[test]
fn test_default_values() {
    let config = UpgradeConfig::default();
    assert_eq!(config.last_poll, 0);
    assert_eq!(config.last_upgrade, 0);
}

#[test]
fn test_save_and_load_roundtrip() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("upgrade_state.json");

    let config = UpgradeConfig {
        last_poll: 1_700_000_000,
        last_upgrade: 1_699_999_000,
    };

    config.save_to_path(&path).expect("Failed to save");

    let loaded = UpgradeConfig::load_from_path(&path);
    assert_eq!(loaded.last_poll, 1_700_000_000);
    assert_eq!(loaded.last_upgrade, 1_699_999_000);
}

#[test]
fn test_load_missing_file_returns_default() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("nonexistent.json");

    let config = UpgradeConfig::load_from_path(&path);
    assert_eq!(config.last_poll, 0);
    assert_eq!(config.last_upgrade, 0);
}

#[test]
fn test_load_corrupt_file_returns_default() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("corrupt.json");

    fs::write(&path, "not valid json!!!").expect("Failed to write");

    let config = UpgradeConfig::load_from_path(&path);
    assert_eq!(config.last_poll, 0);
    assert_eq!(config.last_upgrade, 0);
}

#[test]
fn test_load_partial_json_uses_defaults_for_missing() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("partial.json");

    fs::write(&path, r#"{"last_poll": 12345}"#).expect("Failed to write");

    let config = UpgradeConfig::load_from_path(&path);
    assert_eq!(config.last_poll, 12345);
    assert_eq!(config.last_upgrade, 0);
}

#[test]
fn test_should_poll_with_zero_timestamp() {
    let config = UpgradeConfig::default();
    let interval = Duration::from_secs(900); // 15 minutes
    assert!(config.should_poll(&interval));
}

#[test]
fn test_should_poll_recent_poll() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let config = UpgradeConfig {
        last_poll: now,
        last_upgrade: 0,
    };

    let interval = Duration::from_secs(900);
    assert!(!config.should_poll(&interval));
}

#[test]
fn test_should_poll_old_poll() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let config = UpgradeConfig {
        last_poll: now.saturating_sub(1000), // 1000 seconds ago
        last_upgrade: 0,
    };

    let interval = Duration::from_secs(900); // 15 minutes = 900 seconds
    assert!(config.should_poll(&interval));
}

#[test]
fn test_record_poll_updates_timestamp() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("upgrade_state.json");

    let mut config = UpgradeConfig::default();
    assert_eq!(config.last_poll, 0);

    config.last_poll = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    config.save_to_path(&path).expect("Failed to save");

    let loaded = UpgradeConfig::load_from_path(&path);
    assert!(loaded.last_poll > 0);
    assert_eq!(loaded.last_upgrade, 0);
}

#[test]
fn test_record_upgrade_updates_both_timestamps() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("upgrade_state.json");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let config = UpgradeConfig {
        last_poll: now,
        last_upgrade: now,
    };
    config.save_to_path(&path).expect("Failed to save");

    let loaded = UpgradeConfig::load_from_path(&path);
    assert!(loaded.last_poll > 0);
    assert!(loaded.last_upgrade > 0);
}

#[cfg(unix)]
#[test]
fn test_save_creates_file_with_secure_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("upgrade_state.json");

    let config = UpgradeConfig::default();
    config.save_to_path(&path).expect("Failed to save");

    let perms = fs::metadata(&path).unwrap().permissions();
    let mode = perms.mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "File should have 0600 permissions, got: {mode:o}"
    );
}
