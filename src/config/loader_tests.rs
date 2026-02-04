use std::io::Write;

use tempfile::NamedTempFile;

use crate::common::PROJECT_NAME;
use crate::config::loader::LcOrg;

use super::loader::{Config, UserConfig, parse_team_url};

#[test]
fn test_user_config_default() {
    let config = UserConfig::default();

    assert!(config.fail_open);
    assert!(config.audit_tool_use);
    assert!(config.audit_prompts);
    // Debug mode should be disabled by default (opt-in for security)
    assert!(!config.debug);
}

#[test]
fn test_user_config_builder() {
    let config = UserConfig::builder().fail_open(false).build();

    assert!(!config.fail_open);
}

#[test]
fn test_user_config_serialization() {
    let config = UserConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: UserConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(config.fail_open, deserialized.fail_open);
    assert_eq!(config.audit_tool_use, deserialized.audit_tool_use);
    assert_eq!(config.audit_prompts, deserialized.audit_prompts);
}

#[test]
fn test_user_config_audit_fields_serialization() {
    let config = UserConfig {
        fail_open: true,
        audit_tool_use: false,
        audit_prompts: false,
        debug: false,
    };
    let json = serde_json::to_string(&config).unwrap();

    assert!(json.contains("\"audit_tool_use\":false"));
    assert!(json.contains("\"audit_prompts\":false"));
    assert!(json.contains("\"debug\":false"));

    let deserialized: UserConfig = serde_json::from_str(&json).unwrap();
    assert!(!deserialized.audit_tool_use);
    assert!(!deserialized.audit_prompts);
}

#[test]
fn test_config_load_existing_valid() {
    let json = r#"{
        "user": {
            "fail_open": false,
            "audit_tool_use": true,
            "audit_prompts": false
        },
        "install_id": "test-install-id-123",
        "org": {
            "oid": "",
            "jwt": "",
            "name": "",
            "url": ""
        }
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let config = Config::load_existing(temp_file.path()).unwrap();

    assert!(!config.user.fail_open);
    assert!(config.user.audit_tool_use);
    assert!(!config.user.audit_prompts);
    assert_eq!(config.install_id, "test-install-id-123");
}

#[test]
fn test_config_load_existing_backwards_compatible() {
    // Old config format without audit_tool_use and audit_prompts fields
    // Should default to true for both
    let json = r#"{
        "user": {
            "fail_open": false
        },
        "install_id": "test-install-id-123",
        "org": {
            "oid": "",
            "name": "",
            "url": ""
        }
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let config = Config::load_existing(temp_file.path()).unwrap();

    assert!(!config.user.fail_open);
    // New fields should default to true for backwards compatibility
    assert!(config.user.audit_tool_use);
    assert!(config.user.audit_prompts);
}

#[test]
fn test_config_load_existing_invalid_json() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(b"not valid json").unwrap();

    let result = Config::load_existing(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn test_config_load_existing_missing_fields() {
    let json = r#"{"user": {}}"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let result = Config::load_existing(temp_file.path());

    assert!(result.is_err());
}

#[test]
fn test_config_load_existing_nonexistent_file() {
    let result = Config::load_existing(std::path::Path::new("/nonexistent/path/config.json"));
    assert!(result.is_err());
}

#[test]
fn test_config_serialization_roundtrip() {
    let config = Config {
        user: UserConfig::builder().fail_open(true).build(),
        install_id: "roundtrip-test-id".to_string(),
        org: LcOrg::default(),
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    let deserialized: Config = serde_json::from_str(&json).unwrap();

    assert_eq!(config.user.fail_open, deserialized.user.fail_open);
    assert_eq!(config.install_id, deserialized.install_id);
}

// Tests for debug field

#[test]
fn test_debug_defaults_to_false() {
    // debug should be opt-in (false by default) for security
    let config = UserConfig::default();
    assert!(!config.debug, "debug must be false by default for security");
}

#[test]
fn test_debug_backwards_compatible() {
    // Old config without debug should default to false
    let json = r#"{
        "user": {
            "fail_open": true,
            "audit_tool_use": true,
            "audit_prompts": true
        },
        "install_id": "test-id",
        "org": {
            "oid": "",
            "name": "",
            "url": ""
        }
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let config = Config::load_existing(temp_file.path()).unwrap();

    // debug should default to false when not present in old configs
    assert!(!config.user.debug);
}

#[test]
fn test_debug_can_be_enabled() {
    let config = UserConfig {
        fail_open: true,
        audit_tool_use: true,
        audit_prompts: true,
        debug: true,
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("\"debug\":true"));

    let deserialized: UserConfig = serde_json::from_str(&json).unwrap();
    assert!(deserialized.debug);
}

#[test]
fn test_debug_serialization_roundtrip() {
    // Test that debug=true survives serialization roundtrip
    let json = r#"{
        "user": {
            "fail_open": true,
            "audit_tool_use": true,
            "audit_prompts": true,
            "debug": true
        },
        "install_id": "test-id",
        "org": {
            "oid": "test-oid",
            "name": "test-org",
            "url": "https://example.com/oid/adapter/secret"
        }
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let config = Config::load_existing(temp_file.path()).unwrap();
    assert!(config.user.debug);
}

// Tests for get_debug_log_path

#[test]
fn test_get_debug_log_path_returns_debug_directory() {
    use super::loader::get_debug_log_path;

    let path = get_debug_log_path().unwrap();

    // Should end with 'debug' directory
    assert!(
        path.ends_with("debug"),
        "Debug log path should be the 'debug' directory, got: {}",
        path.display()
    );
}

#[test]
fn test_get_debug_log_path_creates_directory() {
    use super::loader::get_debug_log_path;

    let path = get_debug_log_path().unwrap();

    // Directory should exist after calling get_debug_log_path
    assert!(
        path.exists(),
        "Debug directory should be created: {}",
        path.display()
    );
    assert!(
        path.is_dir(),
        "Debug path should be a directory: {}",
        path.display()
    );
}

#[cfg(unix)]
#[test]
fn test_get_debug_log_path_secure_permissions() {
    use std::os::unix::fs::PermissionsExt;

    use super::loader::get_debug_log_path;

    let debug_dir = get_debug_log_path().unwrap();

    let perms = std::fs::metadata(&debug_dir).unwrap().permissions();
    let mode = perms.mode() & 0o777;

    // Should be owner-only (0o700)
    assert_eq!(
        mode, 0o700,
        "Debug directory should have 0o700 permissions, got: {:o}",
        mode
    );
}

#[cfg(unix)]
#[test]
fn test_get_debug_log_path_fixes_insecure_permissions() {
    use std::os::unix::fs::PermissionsExt;

    use super::loader::get_debug_log_path;

    // Ensure directory exists first
    let debug_dir = get_debug_log_path().unwrap();

    // Set insecure permissions (world-readable)
    std::fs::set_permissions(&debug_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Verify permissions changed - some CI environments (macOS sandbox) may prevent
    // setting more permissive modes, so skip the rest of the test if we can't
    let mode_before = std::fs::metadata(&debug_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    if mode_before != 0o755 {
        // Platform restrictions prevent setting insecure permissions (e.g., macOS sandbox)
        // Skip the rest of this test - the secure permissions test covers the main case
        eprintln!(
            "Skipping test: platform prevented setting insecure permissions (got {:o}, expected 0o755)",
            mode_before
        );
        return;
    }

    // Call get_debug_log_path again - should fix permissions
    let _ = get_debug_log_path().unwrap();

    // Verify permissions are now secure
    let mode_after = std::fs::metadata(&debug_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode_after, 0o700,
        "Debug directory permissions should be fixed to 0o700, got: {:o}",
        mode_after
    );
}

// Tests for clean_logs_in_dir (uses temp directories for isolation)

#[test]
fn test_clean_logs_in_dir_removes_log_files() {
    use super::loader::clean_logs_in_dir;

    // Use temp directory for test isolation
    let temp_dir = tempfile::tempdir().unwrap();
    let dir = temp_dir.path();

    // Create test .log files
    std::fs::write(dir.join("test1.log"), "content 1").unwrap();
    std::fs::write(dir.join("test2.log"), "content 2").unwrap();

    let (removed, bytes) = clean_logs_in_dir(dir).unwrap();

    assert_eq!(removed, 2, "Should remove 2 .log files");
    assert!(bytes > 0, "Should report bytes freed");
    assert!(!dir.join("test1.log").exists());
    assert!(!dir.join("test2.log").exists());
    // temp_dir automatically cleaned up on drop
}

#[test]
fn test_clean_logs_in_dir_only_removes_log_extension() {
    use super::loader::clean_logs_in_dir;

    let temp_dir = tempfile::tempdir().unwrap();
    let dir = temp_dir.path();

    // Create files with different extensions
    std::fs::write(dir.join("keep.txt"), "should not be deleted").unwrap();
    std::fs::write(dir.join("keep.json"), "should not be deleted").unwrap();
    std::fs::write(dir.join("delete1.log"), "should be deleted").unwrap();
    std::fs::write(dir.join("delete2.log"), "should be deleted").unwrap();

    let (removed, _) = clean_logs_in_dir(dir).unwrap();

    assert_eq!(removed, 2, "Should only remove .log files");
    assert!(dir.join("keep.txt").exists(), ".txt should remain");
    assert!(dir.join("keep.json").exists(), ".json should remain");
    assert!(!dir.join("delete1.log").exists(), ".log should be removed");
    assert!(!dir.join("delete2.log").exists(), ".log should be removed");
}

#[test]
fn test_clean_logs_in_dir_empty_directory() {
    use super::loader::clean_logs_in_dir;

    let temp_dir = tempfile::tempdir().unwrap();

    let (removed, bytes) = clean_logs_in_dir(temp_dir.path()).unwrap();

    assert_eq!(removed, 0);
    assert_eq!(bytes, 0);
}

#[test]
fn test_clean_logs_in_dir_nonexistent_directory() {
    use super::loader::clean_logs_in_dir;

    let result = clean_logs_in_dir(std::path::Path::new("/nonexistent/path/that/does/not/exist"));

    // Should succeed with 0 files (not error)
    assert!(result.is_ok());
    let (removed, bytes) = result.unwrap();
    assert_eq!(removed, 0);
    assert_eq!(bytes, 0);
}

#[test]
fn test_clean_logs_in_dir_reports_correct_bytes() {
    use super::loader::clean_logs_in_dir;

    let temp_dir = tempfile::tempdir().unwrap();
    let dir = temp_dir.path();

    // Create files with known sizes
    let content = "x".repeat(100);
    std::fs::write(dir.join("file1.log"), &content).unwrap();
    std::fs::write(dir.join("file2.log"), &content).unwrap();

    let (removed, bytes) = clean_logs_in_dir(dir).unwrap();

    assert_eq!(removed, 2);
    assert_eq!(bytes, 200, "Should report correct total bytes");
}

// Tests for parse_team_url

#[test]
fn test_parse_team_url_valid() {
    let url = format!("https://abc123.hook.limacharlie.io/org-id/{PROJECT_NAME}/secret-token");
    let oid = parse_team_url(&url).unwrap();
    assert_eq!(oid, "org-id");
}

#[test]
fn test_parse_team_url_valid_with_trailing_slash() {
    let url = "https://9157798c50af.hook.limacharlie.io/org-id-456/adapter/secret/";
    let oid = parse_team_url(url).unwrap();
    assert_eq!(oid, "org-id-456");
}

#[test]
fn test_parse_team_url_rejects_http() {
    let url = format!("http://abc123.hook.limacharlie.io/org-id/{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("HTTPS"));
}

#[test]
fn test_parse_team_url_rejects_missing_segments() {
    let url = format!("https://abc123.hook.limacharlie.io/org-id/{PROJECT_NAME}");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid team URL format")
    );
}

#[test]
fn test_parse_team_url_rejects_empty_oid() {
    let url = format!("https://abc123.hook.limacharlie.io//{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[test]
fn test_parse_team_url_rejects_no_path() {
    let url = "https://abc123.hook.limacharlie.io";
    let result = parse_team_url(url);
    assert!(result.is_err());
}

#[test]
fn test_parse_team_url_rejects_invalid_url() {
    let url = "not-a-valid-url";
    let result = parse_team_url(url);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid URL"));
}

#[test]
fn test_parse_team_url_rejects_no_host() {
    let url = format!("https:///org-id/{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
}

#[test]
fn test_parse_team_url_rejects_invalid_domain() {
    let url = format!("https://evil.example.com/org-id/{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("LimaCharlie hook URL"));
}

#[test]
fn test_parse_team_url_rejects_similar_domain() {
    // Ensure we don't accept domains that just contain the string
    let url = format!("https://hook.limacharlie.io.evil.com/org-id/{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("LimaCharlie hook URL"));
}

#[test]
fn test_parse_team_url_rejects_bare_domain() {
    // hook.limacharlie.io without a subdomain is not valid
    let url = format!("https://hook.limacharlie.io/org-id/{PROJECT_NAME}/secret");
    let result = parse_team_url(&url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("LimaCharlie hook URL"));
}
