#!/usr/bin/env bats
# End-to-end tests for Codex callback request metadata.
#
# These tests verify the Codex cloud notification payload includes
# process metadata fields required for tool lineage identification.

# Load test helpers
load test_helpers

# Setup runs before each test
setup() {
    setup_test
}

# Teardown runs after each test
teardown() {
    stop_mock_hook_server 2>/dev/null || true
    teardown_test
}

@test "codex callback sends meta_data.ppid as a positive integer" {
    get_python_cmd >/dev/null 2>&1 || skip "python3/python not available"
    command -v jq >/dev/null 2>&1 || skip "jq not available"

    local mock_port
    mock_port="$(get_available_tcp_port)" || skip "unable to allocate local tcp port"
    local capture_file="${TEST_TMPDIR}/captured_request.json"
    # VIBERAILS_CONFIG_DIR is set by setup_test and respected by the binary,
    # so config goes where the binary actually looks on all platforms.
    local config_dir="${VIBERAILS_CONFIG_DIR}"

    # Start local mock cloud endpoint to capture request payload.
    start_mock_hook_server "$mock_port" "$capture_file"

    # Create an authorized config that points to the local mock endpoint.
    # URL includes the secret as the final segment, matching CloudQuery parsing.
    local config_file="${config_dir}/config.json"
    cat > "$config_file" <<EOF
{
    "user": {
        "fail_open": false,
        "audit_tool_use": true,
        "audit_prompts": true,
        "debug": false,
        "auto_upgrade": false
    },
    "install_id": "test-install-id",
    "org": {
        "oid": "test-oid",
        "name": "Test Org",
        "url": "http://127.0.0.1:${mock_port}/test-oid/adapter/test-secret"
    }
}
EOF

    # Sanity check: verify the config file exists where we expect it.
    if [[ ! -f "$config_file" ]]; then
        echo "FATAL: config file not created at: $config_file" >&2
        echo "  HOME=$HOME" >&2
        echo "  XDG_CONFIG_HOME=$XDG_CONFIG_HOME" >&2
        echo "  OSTYPE=$OSTYPE" >&2
        echo "  get_config_dir=$(get_config_dir)" >&2
        return 1
    fi

    local payload='{"session_id":"session-123","event":"agent-turn-complete"}'
    run "$VIBERAILS_BIN" codex-callback "$payload"
    if [[ "$status" -ne 0 ]]; then
        echo "codex-callback failed (exit=$status):" >&2
        echo "  output: $output" >&2
        echo "  config_dir: $config_dir" >&2
        echo "  config_file exists: $(test -f "$config_file" && echo yes || echo NO)" >&2
        echo "  capture_file exists: $(test -f "$capture_file" && echo yes || echo NO)" >&2
        echo "  HOME=$HOME  OSTYPE=$OSTYPE  APPDATA=${APPDATA:-unset}" >&2
        echo "--- config.json ---" >&2
        cat "$config_file" >&2 2>/dev/null || echo "  (file missing)" >&2
        echo "--- captured request ---" >&2
        cat "$capture_file" >&2 2>/dev/null || echo "  (no request captured)" >&2
        echo "--- mock server log ---" >&2
        cat "${TEST_TMPDIR}/mock_hook_server.log" >&2 2>/dev/null || echo "  (no log)" >&2
        return 1
    fi

    # Validate captured request payload structure and ppid type/value.
    # meta_data.ppid must be an integer > 0
    local ppid
    ppid="$(jq -e '.meta_data.ppid' "$capture_file")"
    [[ "$ppid" =~ ^[0-9]+$ ]] || { echo "meta_data.ppid is not a positive integer: $ppid" >&2; return 1; }
    [[ "$ppid" -gt 0 ]] || { echo "meta_data.ppid must be > 0, got: $ppid" >&2; return 1; }

    # meta_data.source must be "codex"
    run jq -e -r '.meta_data.source' "$capture_file"
    assert_exit_code 0 "$status"
    [[ "$output" == "codex" ]] || { echo "meta_data.source expected 'codex', got: $output" >&2; return 1; }

    # notify.event must match the event we sent
    run jq -e -r '.notify.event' "$capture_file"
    assert_exit_code 0 "$status"
    [[ "$output" == "agent-turn-complete" ]] || { echo "notify.event expected 'agent-turn-complete', got: $output" >&2; return 1; }
}
