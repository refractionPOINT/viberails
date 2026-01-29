use serde_json::json;

use super::claude::Claude;

#[test]
fn test_install_into_empty_json() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({});

    claude.install_into("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    assert!(hooks.is_array());
    let hooks_arr = hooks.as_array().unwrap();
    assert_eq!(hooks_arr.len(), 1);
    assert_eq!(hooks_arr[0]["matcher"], "*");
    assert_eq!(hooks_arr[0]["hooks"][0]["command"], "/usr/bin/test-program");
    assert_eq!(hooks_arr[0]["hooks"][0]["type"], "command");
}

#[test]
fn test_install_into_existing_hooks_object() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {}
    });

    claude.install_into("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    assert!(hooks.is_array());
    assert_eq!(hooks.as_array().unwrap().len(), 1);
    assert_eq!(hooks[0]["matcher"], "*");
}

#[test]
fn test_install_into_existing_hook_type_with_different_matcher() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "/other/program"}
                    ]
                }
            ]
        }
    });

    claude.install_into("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    // Should have 2 entries: existing Bash matcher + new wildcard matcher
    assert_eq!(hooks_arr.len(), 2);
    assert_eq!(hooks_arr[0]["matcher"], "Bash");
    assert_eq!(hooks_arr[1]["matcher"], "*");
    assert_eq!(hooks_arr[1]["hooks"][0]["command"], "/usr/bin/test-program");
}

#[test]
fn test_install_into_appends_to_existing_wildcard_matcher() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": "/existing/program"}
                    ]
                }
            ]
        }
    });

    claude.install_into("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    // Should still have 1 matcher entry
    assert_eq!(hooks_arr.len(), 1);
    // But the inner hooks array should have 2 entries
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 2);
    assert_eq!(inner_hooks[0]["command"], "/existing/program");
    assert_eq!(inner_hooks[1]["command"], "/usr/bin/test-program");
}

#[test]
fn test_install_into_skips_if_already_installed() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": "/usr/bin/test-program"}
                    ]
                }
            ]
        }
    });

    claude.install_into("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    assert_eq!(hooks_arr.len(), 1);
    // Should still have only 1 inner hook (not duplicated)
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 1);
}

#[test]
fn test_install_into_different_hook_types() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({});

    claude.install_into("PreToolUse", &mut json).unwrap();
    claude.install_into("PostToolUse", &mut json).unwrap();

    assert!(json["hooks"]["PreToolUse"].is_array());
    assert!(json["hooks"]["PostToolUse"].is_array());
}

#[test]
fn test_install_into_fails_on_non_object() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!([]);

    let result = claude.install_into("PreToolUse", &mut json);
    assert!(result.is_err());
}

#[test]
fn test_install_into_fails_if_hooks_not_object() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": "not an object"
    });

    let result = claude.install_into("PreToolUse", &mut json);
    assert!(result.is_err());
}
