#!/usr/bin/env bats
# End-to-end tests for viberails show-config command
#
# These tests verify the show-config command behavior including:
# - Display of configuration values
# - "Other Settings" section with Debug Mode and Auto Upgrade
# - Backwards compatibility with old config formats
#
# Prerequisites:
# - bats-core installed (https://github.com/bats-core/bats-core)
# - cargo build completed
#
# Run with: bats tests/e2e/show_config.bats

# Load test helpers
load test_helpers

# Setup runs before each test
setup() {
    setup_test
}

# Teardown runs after each test
teardown() {
    teardown_test
}

# -----------------------------------------------------------------------------
# Basic show-config tests
# -----------------------------------------------------------------------------

@test "show-config --help shows usage information" {
    run "$VIBERAILS_BIN" show-config --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "Show Config"
}

@test "show-config displays configuration" {
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Fail Open"
}

# -----------------------------------------------------------------------------
# Other Settings section tests
# -----------------------------------------------------------------------------

@test "show-config displays Other Settings section" {
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Other Settings"
}

@test "show-config displays Debug Mode in Other Settings" {
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Debug Mode"
}

@test "show-config displays Auto Upgrade in Other Settings" {
    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"
    assert_contains "$output" "Auto Upgrade"
}

# -----------------------------------------------------------------------------
# Auto Upgrade config tests
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

# -----------------------------------------------------------------------------
# Backwards compatibility tests
# -----------------------------------------------------------------------------

@test "show-config works with old config format (no debug field)" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create old-style config without debug field
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true,
        "audit_tool_use": true,
        "audit_prompts": true
    },
    "install_id": "old-style-id",
    "org": { "oid": "test-oid", "name": "Test Org", "url": "https://test.hook.limacharlie.io/oid/adapter/secret" }
}
EOF

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    # Should show debug mode (defaulted to disabled)
    assert_contains "$output" "Debug Mode"
    # Should show auto upgrade (defaulted to enabled)
    assert_contains "$output" "Auto Upgrade"
    # Should show Other Settings section
    assert_contains "$output" "Other Settings"
}

@test "show-config works with very old config format (minimal fields)" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create very old-style config with only fail_open
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true
    },
    "install_id": "legacy-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    # Should still work and show all sections
    assert_contains "$output" "Fail Open"
    assert_contains "$output" "Other Settings"
    assert_contains "$output" "Debug Mode"
    assert_contains "$output" "Auto Upgrade"
}

# -----------------------------------------------------------------------------
# Debug mode display tests
# -----------------------------------------------------------------------------

@test "show-config shows debug mode as disabled by default" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create config without explicit debug setting
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true
    },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    # Debug mode should be shown as disabled
    assert_contains "$output" "Debug Mode"
    assert_contains "$output" "disabled"
}

@test "show-config shows enabled debug mode when configured" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"

    # Create config with debug enabled
    cat > "${config_dir}/config.json" <<EOF
{
    "user": {
        "fail_open": true,
        "debug": true
    },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    run "$VIBERAILS_BIN" show-config
    assert_exit_code 0 "$status"

    # Should show debug mode as enabled
    assert_contains "$output" "Debug Mode"
    assert_contains "$output" "enabled"
}
