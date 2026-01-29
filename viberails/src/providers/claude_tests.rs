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
    // Should have 2 entries: new wildcard matcher inserted at front + existing Bash matcher
    assert_eq!(hooks_arr.len(), 2);
    assert_eq!(hooks_arr[0]["matcher"], "*");
    assert_eq!(hooks_arr[0]["hooks"][0]["command"], "/usr/bin/test-program");
    assert_eq!(hooks_arr[1]["matcher"], "Bash");
}

#[test]
fn test_install_into_prepends_to_existing_wildcard_matcher() {
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
    // But the inner hooks array should have 2 entries, with our hook inserted at front
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 2);
    assert_eq!(inner_hooks[0]["command"], "/usr/bin/test-program");
    assert_eq!(inner_hooks[1]["command"], "/existing/program");
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

#[test]
fn test_uninstall_from_removes_our_hook() {
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

    claude.uninstall_from("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 0);
}

#[test]
fn test_uninstall_from_preserves_other_hooks() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": "/other/program"},
                        {"type": "command", "command": "/usr/bin/test-program"},
                        {"type": "command", "command": "/another/program"}
                    ]
                }
            ]
        }
    });

    claude.uninstall_from("PreToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 2);
    assert_eq!(inner_hooks[0]["command"], "/other/program");
    assert_eq!(inner_hooks[1]["command"], "/another/program");
}

#[test]
fn test_uninstall_from_no_hooks_object() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({});

    // Should succeed without error (just warns)
    claude.uninstall_from("PreToolUse", &mut json).unwrap();
}

#[test]
fn test_uninstall_from_no_hook_type() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {}
    });

    // Should succeed without error (just warns)
    claude.uninstall_from("PreToolUse", &mut json).unwrap();
}

#[test]
fn test_uninstall_from_no_wildcard_matcher() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "/usr/bin/test-program"}
                    ]
                }
            ]
        }
    });

    // Should succeed without error (just warns) - only looks at wildcard matcher
    claude.uninstall_from("PreToolUse", &mut json).unwrap();

    // Hook in non-wildcard matcher should remain untouched
    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 1);
}

#[test]
fn test_uninstall_from_hook_not_present() {
    let claude = Claude::with_custom_path("/usr/bin/test-program").unwrap();
    let mut json = json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": "/other/program"}
                    ]
                }
            ]
        }
    });

    // Should succeed without error (just warns)
    claude.uninstall_from("PreToolUse", &mut json).unwrap();

    // Other hooks should remain
    let hooks = &json["hooks"]["PreToolUse"];
    let hooks_arr = hooks.as_array().unwrap();
    let inner_hooks = hooks_arr[0]["hooks"].as_array().unwrap();
    assert_eq!(inner_hooks.len(), 1);
    assert_eq!(inner_hooks[0]["command"], "/other/program");
}

#[test]
fn test_uninstall_from_different_hook_types() {
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
            ],
            "PostToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": "/usr/bin/test-program"}
                    ]
                }
            ]
        }
    });

    claude.uninstall_from("PreToolUse", &mut json).unwrap();

    // PreToolUse hook should be removed
    let pre_hooks = &json["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
    assert_eq!(pre_hooks.len(), 0);

    // PostToolUse hook should remain
    let post_hooks = &json["hooks"]["PostToolUse"][0]["hooks"].as_array().unwrap();
    assert_eq!(post_hooks.len(), 1);
}
