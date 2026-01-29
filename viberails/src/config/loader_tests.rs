use std::io::Write;

use tempfile::NamedTempFile;

use super::loader::{Config, UserConfig};

#[test]
fn test_user_config_default() {
    let config = UserConfig::default();

    assert_eq!(config.login_url, "http://localhost:8000/login");
    assert_eq!(config.authorize_url, "http://localhost:8000/dnr");
    assert_eq!(config.notification_url, "http://localhost:8000/notify");
    assert!(config.fail_open);
}

#[test]
fn test_user_config_builder() {
    let config = UserConfig::builder()
        .login_url("https://example.com/login".to_string())
        .authorize_url("https://example.com/auth".to_string())
        .notification_url("https://example.com/notify".to_string())
        .fail_open(false)
        .build();

    assert_eq!(config.login_url, "https://example.com/login");
    assert_eq!(config.authorize_url, "https://example.com/auth");
    assert_eq!(config.notification_url, "https://example.com/notify");
    assert!(!config.fail_open);
}

#[test]
fn test_user_config_serialization() {
    let config = UserConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: UserConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(config.login_url, deserialized.login_url);
    assert_eq!(config.authorize_url, deserialized.authorize_url);
    assert_eq!(config.notification_url, deserialized.notification_url);
    assert_eq!(config.fail_open, deserialized.fail_open);
}

#[test]
fn test_config_load_existing_valid() {
    let json = r#"{
        "user": {
            "login_url": "https://test.com/login",
            "authorize_url": "https://test.com/auth",
            "notification_url": "https://test.com/notify",
            "fail_open": false
        },
        "install_id": "test-install-id-123"
    }"#;

    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(json.as_bytes()).unwrap();

    let config = Config::load_existing(temp_file.path()).unwrap();

    assert_eq!(config.user.login_url, "https://test.com/login");
    assert_eq!(config.user.authorize_url, "https://test.com/auth");
    assert_eq!(config.user.notification_url, "https://test.com/notify");
    assert!(!config.user.fail_open);
    assert_eq!(config.install_id, "test-install-id-123");
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
        user: UserConfig::builder()
            .login_url("https://example.com/login".to_string())
            .authorize_url("https://example.com/auth".to_string())
            .notification_url("https://example.com/notify".to_string())
            .fail_open(true)
            .build(),
        install_id: "roundtrip-test-id".to_string(),
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    let deserialized: Config = serde_json::from_str(&json).unwrap();

    assert_eq!(config.user.login_url, deserialized.user.login_url);
    assert_eq!(config.user.authorize_url, deserialized.user.authorize_url);
    assert_eq!(
        config.user.notification_url,
        deserialized.user.notification_url
    );
    assert_eq!(config.user.fail_open, deserialized.user.fail_open);
    assert_eq!(config.install_id, deserialized.install_id);
}
