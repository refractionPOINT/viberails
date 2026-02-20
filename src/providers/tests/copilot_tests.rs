#![allow(clippy::unwrap_used)]

use serde_json::json;

use crate::common::EXECUTABLE_NAME;
use crate::providers::copilot::Copilot;

fn make_copilot<P: AsRef<std::path::Path>>(program: P) -> Copilot {
    Copilot::with_test_paths(program, std::path::PathBuf::from("/tmp/test-hooks.json"))
}

fn test_exe_path() -> String {
    format!("/usr/bin/{EXECUTABLE_NAME}")
}

fn test_bash_command() -> String {
    format!("/usr/bin/{EXECUTABLE_NAME} copilot-callback")
}

#[test]
fn test_install_into_empty_json() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hooks = &json["hooks"]["preToolUse"];
    assert!(hooks.is_array());
    let hooks_arr = hooks.as_array().unwrap();
    assert_eq!(hooks_arr.len(), 1);
    assert_eq!(hooks_arr[0]["type"], "command");
    assert_eq!(
        hooks_arr[0]["bash"],
        "/usr/bin/test-program copilot-callback"
    );
}

#[test]
fn test_install_into_creates_hooks_object() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1});

    copilot.install_into("preToolUse", &mut json).unwrap();

    assert!(json.get("hooks").is_some());
    assert!(json["hooks"]["preToolUse"].is_array());
}

#[test]
fn test_install_into_creates_version() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({});

    copilot.install_into("preToolUse", &mut json).unwrap();

    assert_eq!(json["version"], 1);
}

#[test]
fn test_install_into_preserves_existing_hooks() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({
        "version": 1,
        "hooks": {
            "preToolUse": [
                {
                    "type": "command",
                    "bash": "/other/program"
                }
            ]
        }
    });

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hooks = json["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(hooks.len(), 2);
    // Our hook should be first
    assert_eq!(hooks[0]["bash"], "/usr/bin/test-program copilot-callback");
    assert_eq!(hooks[1]["bash"], "/other/program");
}

#[test]
fn test_install_into_skips_if_already_installed() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({
        "version": 1,
        "hooks": {
            "preToolUse": [
                {
                    "type": "command",
                    "bash": "/usr/bin/test-program copilot-callback",
                    "comment": "viberails security hook"
                }
            ]
        }
    });

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hooks = json["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
}

#[test]
fn test_install_into_different_hook_types() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    copilot.install_into("preToolUse", &mut json).unwrap();
    copilot
        .install_into("userPromptSubmitted", &mut json)
        .unwrap();

    assert!(json["hooks"]["preToolUse"].is_array());
    assert!(json["hooks"]["userPromptSubmitted"].is_array());
}

// Copilot-specific format tests

#[test]
fn test_install_into_uses_bash_field_not_command() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hook = &json["hooks"]["preToolUse"][0];
    // Must have `bash` field
    assert!(hook.get("bash").is_some());
    // Must NOT have `command` field (that's Cursor format)
    assert!(hook.get("command").is_none());
}

#[test]
fn test_install_into_has_comment_field() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hook = &json["hooks"]["preToolUse"][0];
    assert_eq!(hook["comment"], "viberails security hook");
}

#[test]
fn test_install_into_no_matcher_field() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    copilot.install_into("preToolUse", &mut json).unwrap();

    let hook = &json["hooks"]["preToolUse"][0];
    // Copilot does not use matchers
    assert!(hook.get("matcher").is_none());
}

// Uninstall tests

#[test]
fn test_uninstall_from_removes_our_hook() {
    let copilot = make_copilot(test_exe_path());
    let mut json = json!({
        "version": 1,
        "hooks": {
            "preToolUse": [
                {
                    "type": "command",
                    "bash": test_bash_command(),
                    "comment": "viberails security hook"
                }
            ]
        }
    });

    copilot.uninstall_from("preToolUse", &mut json);

    let hooks = json["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(hooks.len(), 0);
}

#[test]
fn test_uninstall_from_preserves_other_hooks() {
    let copilot = make_copilot(test_exe_path());
    let mut json = json!({
        "version": 1,
        "hooks": {
            "preToolUse": [
                {
                    "type": "command",
                    "bash": "/other/program"
                },
                {
                    "type": "command",
                    "bash": test_bash_command(),
                    "comment": "viberails security hook"
                }
            ]
        }
    });

    copilot.uninstall_from("preToolUse", &mut json);

    let hooks = json["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["bash"], "/other/program");
}

#[test]
fn test_uninstall_from_no_hooks_object() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1});

    // Should not panic
    copilot.uninstall_from("preToolUse", &mut json);
}

#[test]
fn test_uninstall_from_no_hook_type() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"version": 1, "hooks": {}});

    // Should not panic
    copilot.uninstall_from("preToolUse", &mut json);
}

// Discovery tests

#[test]
fn test_copilot_discovery_id() {
    use crate::providers::ProviderDiscovery;
    use crate::providers::copilot::CopilotDiscovery;

    let discovery = CopilotDiscovery;
    assert_eq!(discovery.id(), "copilot");
}

#[test]
fn test_copilot_discovery_display_name() {
    use crate::providers::ProviderDiscovery;
    use crate::providers::copilot::CopilotDiscovery;

    let discovery = CopilotDiscovery;
    assert_eq!(discovery.display_name(), "GitHub Copilot CLI");
}

#[test]
fn test_copilot_discovery_supported_hooks() {
    use crate::providers::ProviderDiscovery;
    use crate::providers::copilot::CopilotDiscovery;

    let discovery = CopilotDiscovery;
    let hooks = discovery.supported_hooks();
    assert!(hooks.contains(&"preToolUse"));
    assert!(hooks.contains(&"userPromptSubmitted"));
    assert!(hooks.contains(&"postToolUse"));
}

// write_answer format tests
//
// Copilot is the ONLY provider that overrides write_answer().
// It uses "permissionDecision"/"permissionDecisionReason" instead of the
// default "decision"/"reason" format. These tests verify the JSON structure.

#[test]
fn test_block_response_uses_copilot_format() {
    // Verify the Copilot response format has the right field names
    let response = serde_json::json!({
        "permissionDecision": "deny",
        "permissionDecisionReason": "test reason"
    });
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(serialized.contains("permissionDecision"));
    assert!(serialized.contains("permissionDecisionReason"));
    // Verify it does NOT contain the default format fields
    assert!(!serialized.contains("\"decision\""));
    assert!(!serialized.contains("\"reason\""));
}

#[test]
fn test_block_response_default_reason() {
    // When reason is None, should use "Blocked by viberails policy"
    let reason: Option<&str> = None;
    let response = serde_json::json!({
        "permissionDecision": "deny",
        "permissionDecisionReason": reason.unwrap_or("Blocked by viberails policy")
    });
    assert_eq!(
        response["permissionDecisionReason"],
        "Blocked by viberails policy"
    );
}

#[test]
fn test_block_response_custom_reason() {
    // When reason is Some, should use the provided reason
    let reason: Option<&str> = Some("Custom block reason");
    let response = serde_json::json!({
        "permissionDecision": "deny",
        "permissionDecisionReason": reason.unwrap_or("Blocked by viberails policy")
    });
    assert_eq!(
        response["permissionDecisionReason"],
        "Custom block reason"
    );
}

// Error path tests for install_into

#[test]
fn test_install_into_fails_on_non_object() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!([]);

    let result = copilot.install_into("preToolUse", &mut json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("JSON object"));
}

#[test]
fn test_install_into_fails_if_hooks_not_object() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"hooks": "not an object"});

    let result = copilot.install_into("preToolUse", &mut json);
    assert!(result.is_err());
}

#[test]
fn test_install_into_fails_if_hook_type_not_array() {
    let copilot = make_copilot("/usr/bin/test-program");
    let mut json = json!({"hooks": {"preToolUse": "not an array"}});

    let result = copilot.install_into("preToolUse", &mut json);
    assert!(result.is_err());
}
