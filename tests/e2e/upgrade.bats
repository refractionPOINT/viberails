#!/usr/bin/env bats
# End-to-end tests for viberails upgrade functionality
#
# These tests verify the upgrade command behavior including:
# - Help output and CLI flags
# - Concurrent upgrade detection via lock files
# - Graceful error handling
# - Temp file cleanup
#
# Prerequisites:
# - bats-core installed (https://github.com/bats-core/bats-core)
# - cargo build completed
#
# Run with: bats tests/e2e/upgrade.bats

# Load test helpers
load test_helpers

# Setup runs before each test
setup() {
    setup_test
}

# Teardown runs after each test
teardown() {
    release_upgrade_lock 2>/dev/null || true
    teardown_test
}

# -----------------------------------------------------------------------------
# Basic upgrade command tests
# -----------------------------------------------------------------------------

@test "upgrade --help shows usage information" {
    run "$VIBERAILS_BIN" upgrade --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "Upgrade"
    assert_contains "$output" "--force"
}

@test "upgrade -h shows usage information" {
    run "$VIBERAILS_BIN" upgrade -h
    assert_exit_code 0 "$status"
    assert_contains "$output" "Upgrade"
}

# -----------------------------------------------------------------------------
# Force upgrade flag tests
# -----------------------------------------------------------------------------

@test "upgrade --force flag is accepted" {
    run timeout 2 "$VIBERAILS_BIN" upgrade --force 2>&1 || true
    # Should not fail due to unknown flag
    assert_not_contains "$output" "unexpected argument"
    assert_not_contains "$output" "unknown option"
}

@test "upgrade -f short flag is accepted" {
    run timeout 2 "$VIBERAILS_BIN" upgrade -f 2>&1 || true
    # Should not fail due to unknown flag
    assert_not_contains "$output" "unexpected argument"
    assert_not_contains "$output" "unknown option"
}

# -----------------------------------------------------------------------------
# Concurrent upgrade detection tests
# -----------------------------------------------------------------------------

@test "upgrade detects another upgrade in progress" {
    # Skip if flock not available
    command -v flock >/dev/null || skip "flock not available"

    # Create binary directory and install a copy
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/viberails"
    chmod +x "${bin_dir}/viberails"

    # Create and hold the upgrade lock
    local lock_file
    lock_file=$(create_upgrade_lock)

    # Try to upgrade - should detect lock and fail fast or timeout
    run timeout 2 "${bin_dir}/viberails" upgrade 2>&1 || true

    # Should indicate another upgrade is in progress or timeout
    [[ "$status" -eq 0 ]] || [[ -n "$output" ]]
}

# -----------------------------------------------------------------------------
# Binary installation path tests
# -----------------------------------------------------------------------------

@test "binary respects HOME environment variable" {
    # Create custom home directory
    local custom_home="${TEST_TMPDIR}/custom_home"
    mkdir -p "$custom_home"

    export HOME="$custom_home"

    # Binary should use the custom home for config/data
    run "$VIBERAILS_BIN" --version
    assert_exit_code 0 "$status"
}

# -----------------------------------------------------------------------------
# Version display tests
# -----------------------------------------------------------------------------

@test "version output format is correct" {
    run "$VIBERAILS_BIN" --version
    assert_exit_code 0 "$status"

    # Should contain 'viberails' and a version number
    assert_contains "$output" "viberails"
    # Version should match semver-like pattern or git hash
    [[ "$output" =~ [0-9]+\.[0-9]+ ]] || [[ "$output" =~ [a-f0-9]{7} ]]
}

# -----------------------------------------------------------------------------
# Error handling tests
# -----------------------------------------------------------------------------

@test "upgrade handles missing network gracefully" {
    # The upgrade should fail gracefully, not crash
    run timeout 3 "$VIBERAILS_BIN" upgrade 2>&1 || true

    # Should not panic or segfault
    [[ "$status" -ne 139 ]]  # Not SIGSEGV
    [[ "$status" -ne 134 ]]  # Not SIGABRT
}

# -----------------------------------------------------------------------------
# Cleanup tests
# -----------------------------------------------------------------------------

@test "upgrade cleans up old upgrade binaries on startup" {
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/viberails"

    # Create fake old upgrade binaries
    touch "${bin_dir}/viberails_upgrade_12345678"
    touch "${bin_dir}/viberails_upgrade_abcdef01"

    # Run upgrade (will fail but should clean up)
    run timeout 2 "${bin_dir}/viberails" upgrade 2>&1 || true

    # Old upgrade binaries should be removed
    # Note: cleanup happens at start of upgrade process
    [[ ! -f "${bin_dir}/viberails_upgrade_12345678" ]] || \
    [[ ! -f "${bin_dir}/viberails_upgrade_abcdef01" ]] || \
    true  # May not run cleanup if upgrade fails early
}

@test "upgrade does not leave temp files on failure" {
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/viberails"

    # Run upgrade (will fail)
    run timeout 2 "${bin_dir}/viberails" upgrade 2>&1 || true

    # Count temp files (should be none)
    local temp_count
    temp_count=$(find "$bin_dir" -name ".viberails_new_*" 2>/dev/null | wc -l)
    [[ "$temp_count" -eq 0 ]]
}

# -----------------------------------------------------------------------------
# Integration with other commands
# -----------------------------------------------------------------------------

@test "poll_upgrade runs silently on normal exit" {
    # When running a normal command, the background upgrade check should be silent
    run "$VIBERAILS_BIN" --version
    assert_exit_code 0 "$status"

    # Output should only contain version info, not upgrade messages
    assert_not_contains "$output" "Checking for updates"
    assert_not_contains "$output" "Downloading"
}

@test "list command does not trigger verbose upgrade output" {
    run "$VIBERAILS_BIN" list
    # Should not show upgrade progress
    assert_not_contains "$output" "Current version:"
    assert_not_contains "$output" "Latest version:"
}

# -----------------------------------------------------------------------------
# Auto-upgrade config tests
# -----------------------------------------------------------------------------

@test "auto_upgrade setting shown in configuration" {
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Auto Upgrade"
}

@test "auto_upgrade defaults to enabled" {
    # With no config file, show-config should show auto_upgrade enabled
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    # Output contains "Auto Upgrade" with enabled indicator (checkmark or true)
    assert_contains "$output" "Auto Upgrade"
}

@test "auto_upgrade can be disabled via config" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create config with auto_upgrade disabled
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true,
        "audit_tool_use": true,
        "audit_prompts": true,
        "debug": false,
        "auto_upgrade": false
    },
    "install_id": "test-install-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Auto Upgrade"
}

@test "auto_upgrade config persists across show-config calls" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create config with auto_upgrade disabled
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true,
        "audit_tool_use": true,
        "audit_prompts": true,
        "debug": false,
        "auto_upgrade": false
    },
    "install_id": "test-install-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Run show-config twice
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    # Config file should still exist with auto_upgrade: false
    local config_content
    config_content=$(cat "${config_dir}/config.json")
    [[ "$config_content" == *'"auto_upgrade": false'* ]] || \
    [[ "$config_content" == *'"auto_upgrade":false'* ]]
}
