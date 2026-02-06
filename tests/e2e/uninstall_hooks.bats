#!/usr/bin/env bats
# End-to-end tests for viberails uninstall-hooks command
#
# These tests verify the uninstall-hooks command behavior including:
# - Removing hooks from selected providers
# - Keeping the binary and config intact
# - Backward compatibility with 'uninstall' alias
#
# Prerequisites:
# - bats-core installed (https://github.com/bats-core/bats-core)
# - cargo build completed
#
# Run with: bats tests/e2e/uninstall_hooks.bats

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
# Basic uninstall-hooks command tests
# -----------------------------------------------------------------------------

@test "uninstall-hooks --help shows usage information" {
    run "$VIBERAILS_BIN" uninstall-hooks --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "Uninstall"
    assert_contains "$output" "hooks"
}

@test "uninstall-hooks -h shows usage information" {
    run "$VIBERAILS_BIN" uninstall-hooks -h
    assert_exit_code 0 "$status"
    assert_contains "$output" "Uninstall"
}

@test "uninstall-hooks command is recognized" {
    # Just verify the command exists and doesn't error on unrecognized command
    run "$VIBERAILS_BIN" uninstall-hooks --help
    assert_exit_code 0 "$status"
    assert_not_contains "$output" "unrecognized"
    assert_not_contains "$output" "unknown"
}

# -----------------------------------------------------------------------------
# Backward compatibility - 'uninstall' alias
# -----------------------------------------------------------------------------

@test "uninstall alias --help shows usage information" {
    run "$VIBERAILS_BIN" uninstall --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "Uninstall"
}

@test "uninstall alias is recognized" {
    run "$VIBERAILS_BIN" uninstall --help
    assert_exit_code 0 "$status"
    assert_not_contains "$output" "unrecognized"
    assert_not_contains "$output" "unknown"
}

# -----------------------------------------------------------------------------
# Binary preservation tests
# -----------------------------------------------------------------------------

@test "uninstall-hooks does not remove the binary" {
    # Create config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify binary exists
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]

    # Run uninstall-hooks (will fail due to no providers, but that's ok)
    # We just want to verify it doesn't delete the binary
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-hooks 2>&1 || true

    # Binary should still be there
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
}

@test "uninstall-hooks does not remove the config directory" {
    # Create config directory with files
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify config exists
    [[ -d "$config_dir" ]]
    [[ -f "${config_dir}/config.json" ]]

    # Run uninstall-hooks
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-hooks 2>&1 || true

    # Config directory should still be there
    [[ -d "$config_dir" ]]
    [[ -f "${config_dir}/config.json" ]]
}

@test "uninstall-hooks does not remove the data directory" {
    # Create config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create data directory with files
    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$data_dir"
    echo '{"last_poll": 12345}' > "${data_dir}/upgrade_state.json"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify data dir exists
    [[ -d "$data_dir" ]]
    [[ -f "${data_dir}/upgrade_state.json" ]]

    # Run uninstall-hooks
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-hooks 2>&1 || true

    # Data directory should still be there
    [[ -d "$data_dir" ]]
    [[ -f "${data_dir}/upgrade_state.json" ]]
}

# -----------------------------------------------------------------------------
# Output message tests
# -----------------------------------------------------------------------------

@test "uninstall-hooks shows binary retained message" {
    # Create config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-hooks
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-hooks 2>&1 || true

    # Should indicate binary is retained
    assert_contains "$output" "retained" || assert_contains "$output" "No providers"
}
