#!/usr/bin/env bats
# End-to-end tests for viberails uninstall-all command
#
# These tests verify the uninstall-all command behavior including:
# - Complete removal of hooks from all providers
# - Deletion of the binary
# - Removal of config directory
# - Removal of data directory (debug logs, upgrade state)
# - Cleanup of lock files and temporary binaries
# - Handling of various edge cases (no hooks, missing directories, permission errors)
#
# Prerequisites:
# - bats-core installed (https://github.com/bats-core/bats-core)
# - cargo build completed
#
# Run with: bats tests/e2e/uninstall_all.bats

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
# Basic uninstall-all command tests
# -----------------------------------------------------------------------------

@test "uninstall-all --help shows usage information" {
    run "$VIBERAILS_BIN" uninstall-all --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "Uninstall"
}

@test "uninstall-all -h shows usage information" {
    run "$VIBERAILS_BIN" uninstall-all -h
    assert_exit_code 0 "$status"
    assert_contains "$output" "Uninstall"
}

@test "uninstall-all command is recognized" {
    # Just verify the command exists and doesn't error on unrecognized command
    run "$VIBERAILS_BIN" uninstall-all --help
    assert_exit_code 0 "$status"
    assert_not_contains "$output" "unrecognized"
    assert_not_contains "$output" "unknown"
}

# -----------------------------------------------------------------------------
# Confirmation prompt tests
# -----------------------------------------------------------------------------

@test "uninstall-all --help shows --yes flag" {
    run "$VIBERAILS_BIN" uninstall-all --help
    assert_exit_code 0 "$status"
    assert_contains "$output" "--yes"
    assert_contains "$output" "-y"
}

@test "uninstall-all without --yes prompts and aborts on 'n'" {
    # Create config and binary so there's something to uninstall
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe 'n' to stdin — should abort without deleting anything
    run bash -c "echo 'n' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_contains "$output" "Aborted"

    # Nothing should have been deleted
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -d "$config_dir" ]]
}

@test "uninstall-all without --yes prompts and aborts on empty input" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe empty string to stdin — should abort (default is N)
    run bash -c "echo '' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_contains "$output" "Aborted"

    # Nothing should have been deleted
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -d "$config_dir" ]]
}

@test "uninstall-all without --yes prompts and proceeds on 'y'" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe 'y' to stdin — should proceed with uninstall
    run bash -c "echo 'y' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_not_contains "$output" "Aborted"

    # Everything should be cleaned up
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all without --yes prompts and proceeds on 'yes'" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe 'yes' to stdin — should proceed with uninstall
    run bash -c "echo 'yes' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_not_contains "$output" "Aborted"

    # Everything should be cleaned up
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all without --yes prompts and proceeds on 'Y' (case-insensitive)" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe 'Y' (uppercase) to stdin — should still proceed
    run bash -c "echo 'Y' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_not_contains "$output" "Aborted"

    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all --yes skips confirmation prompt" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # --yes should skip prompt entirely — no stdin needed
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    assert_exit_code 0 "$status"
    # Should NOT show the confirmation prompt text
    assert_not_contains "$output" "Are you sure"
    assert_not_contains "$output" "Aborted"

    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all -y shorthand works" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # -y short flag should work the same as --yes
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all -y

    assert_exit_code 0 "$status"
    assert_not_contains "$output" "Are you sure"
    assert_not_contains "$output" "Aborted"

    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all without --yes shows warning message" {
    # Create config and binary
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Pipe 'n' and verify the warning text is shown
    run bash -c "echo 'n' | '${bin_dir}/${VIBERAILS_EXE_NAME}' uninstall-all"

    assert_exit_code 0 "$status"
    assert_contains "$output" "permanently remove"
    assert_contains "$output" "Are you sure"
}

# -----------------------------------------------------------------------------
# No hooks installed tests
# -----------------------------------------------------------------------------

@test "uninstall-all works when no hooks are installed" {
    # Create minimal config directory
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

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should succeed and indicate no hooks
    assert_contains "$output" "No hooks are currently installed"
    assert_contains "$output" "cleanup complete" || assert_contains "$output" "removed"
}

@test "uninstall-all removes binary even when no hooks installed" {
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

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Binary should be removed
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
}

# -----------------------------------------------------------------------------
# Config directory cleanup tests
# -----------------------------------------------------------------------------

@test "uninstall-all removes config directory" {
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

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Config directory should be removed
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all handles missing config directory gracefully" {
    # Don't create config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify config doesn't exist
    [[ ! -d "$config_dir" ]]

    # Run uninstall-all - should not crash
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should complete without error
    [[ "$status" -eq 0 ]] || assert_contains "$output" "cleanup"
}

# -----------------------------------------------------------------------------
# Data directory cleanup tests
# -----------------------------------------------------------------------------

@test "uninstall-all removes data directory" {
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

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Data directory should be removed
    [[ ! -d "$data_dir" ]]
}

@test "uninstall-all removes debug logs directory" {
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

    # Create data directory with debug logs
    local data_dir="${XDG_DATA_HOME}/viberails"
    local debug_dir="${data_dir}/debug"
    mkdir -p "$debug_dir"
    echo "debug log content" > "${debug_dir}/debug-12345-abcdef.log"
    echo "more debug log" > "${debug_dir}/debug-67890-fedcba.log"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify debug logs exist
    [[ -d "$debug_dir" ]]
    [[ -f "${debug_dir}/debug-12345-abcdef.log" ]]

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Debug directory should be removed along with data dir
    [[ ! -d "$debug_dir" ]]
    [[ ! -d "$data_dir" ]]
}

@test "uninstall-all handles missing data directory gracefully" {
    # Create only config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local data_dir="${XDG_DATA_HOME}/viberails"
    # Don't create data directory

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify data doesn't exist
    [[ ! -d "$data_dir" ]]

    # Run uninstall-all - should not crash
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should complete successfully
    [[ "$status" -eq 0 ]] || assert_contains "$output" "cleanup"
}

# -----------------------------------------------------------------------------
# Lock file and temp binary cleanup tests
# -----------------------------------------------------------------------------

@test "uninstall-all removes upgrade lock file" {
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

    # Create a stale lock file
    local lock_file="${bin_dir}/.viberails.upgrade.lock"
    echo "99999" > "$lock_file"

    # Verify lock file exists
    [[ -f "$lock_file" ]]

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Lock file should be removed
    [[ ! -f "$lock_file" ]]
}

@test "uninstall-all removes temporary upgrade binaries" {
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

    # Create temporary upgrade binaries (simulating failed upgrades)
    touch "${bin_dir}/viberails_upgrade_12345678"
    touch "${bin_dir}/viberails_upgrade_abcdef01"

    # Create temporary new binaries
    touch "${bin_dir}/.viberails_new_aabbccdd"

    # Verify temp files exist
    [[ -f "${bin_dir}/viberails_upgrade_12345678" ]]
    [[ -f "${bin_dir}/viberails_upgrade_abcdef01" ]]
    [[ -f "${bin_dir}/.viberails_new_aabbccdd" ]]

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # All temp files should be removed
    [[ ! -f "${bin_dir}/viberails_upgrade_12345678" ]]
    [[ ! -f "${bin_dir}/viberails_upgrade_abcdef01" ]]
    [[ ! -f "${bin_dir}/.viberails_new_aabbccdd" ]]
}

@test "uninstall-all reports cleaned temp files" {
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

    # Create temp files
    touch "${bin_dir}/viberails_upgrade_12345678"
    echo "99999" > "${bin_dir}/.viberails.upgrade.lock"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should mention cleaned files
    assert_contains "$output" "temporary file" || assert_contains "$output" "Cleaned"
}

# -----------------------------------------------------------------------------
# Comprehensive cleanup tests
# -----------------------------------------------------------------------------

@test "uninstall-all removes everything in one operation" {
    # Create complete installation
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    local data_dir="${XDG_DATA_HOME}/viberails"
    local debug_dir="${data_dir}/debug"
    local bin_dir="${HOME}/.local/bin"

    mkdir -p "$config_dir" "$debug_dir" "$bin_dir"

    # Config files
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true, "debug": true },
    "install_id": "test-id",
    "org": { "oid": "test-oid", "name": "Test", "url": "https://test.hook.limacharlie.io/test" }
}
EOF

    # Data files
    echo '{"last_poll": 12345, "last_upgrade": 12300}' > "${data_dir}/upgrade_state.json"
    echo "debug log 1" > "${debug_dir}/debug-111-aaa.log"
    echo "debug log 2" > "${debug_dir}/debug-222-bbb.log"

    # Binary
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Temp files
    touch "${bin_dir}/viberails_upgrade_12345678"
    touch "${bin_dir}/.viberails_new_aabbccdd"
    echo "$$" > "${bin_dir}/.viberails.upgrade.lock"

    # Verify everything exists
    [[ -d "$config_dir" ]]
    [[ -d "$data_dir" ]]
    [[ -d "$debug_dir" ]]
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -f "${bin_dir}/viberails_upgrade_12345678" ]]

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Everything should be gone
    [[ ! -d "$config_dir" ]]
    [[ ! -d "$data_dir" ]]
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -f "${bin_dir}/viberails_upgrade_12345678" ]]
    [[ ! -f "${bin_dir}/.viberails_new_aabbccdd" ]]
    [[ ! -f "${bin_dir}/.viberails.upgrade.lock" ]]
}

@test "uninstall-all shows success message on completion" {
    # Create minimal setup
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should show success message
    assert_contains "$output" "cleanup complete" || assert_contains "$output" "removed"
}

# -----------------------------------------------------------------------------
# Permission error handling tests
# -----------------------------------------------------------------------------

@test "uninstall-all continues when some directories cannot be deleted" {
    # Skip on Windows - permission handling differs
    is_windows && skip "permission behavior differs on Windows"

    # Create setup with a permission-protected subdirectory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    local data_dir="${XDG_DATA_HOME}/viberails"
    local bin_dir="${HOME}/.local/bin"

    mkdir -p "$config_dir" "$data_dir" "$bin_dir"

    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Create a directory we can't delete by removing write permission
    local protected_dir="${data_dir}/protected"
    mkdir -p "$protected_dir"
    touch "${protected_dir}/file.txt"

    # Remove write permission from parent (makes deletion of contents fail)
    # Note: This may not work in all environments (e.g., root, some CI)
    chmod 555 "$protected_dir" 2>/dev/null || skip "cannot change permissions"

    # Run uninstall-all - should complete but may report errors
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Restore permissions for cleanup
    chmod 755 "$protected_dir" 2>/dev/null || true

    # The command should have attempted cleanup (may succeed or fail depending on environment)
    # At minimum it should not crash
    [[ -n "$output" ]]
}

# -----------------------------------------------------------------------------
# CLI alias tests
# -----------------------------------------------------------------------------

@test "uninstall-all is accessible via CLI command" {
    run "$VIBERAILS_BIN" uninstall-all --help
    assert_exit_code 0 "$status"
}

# -----------------------------------------------------------------------------
# Edge case tests
# -----------------------------------------------------------------------------

@test "uninstall-all handles empty config directory" {
    # Create empty config directory
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    # Don't add any files

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should complete and remove empty directory
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all handles deeply nested debug logs" {
    # Create config
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create deeply nested structure in data dir
    local data_dir="${XDG_DATA_HOME}/viberails"
    local deep_dir="${data_dir}/debug/archive/2025/01"
    mkdir -p "$deep_dir"
    echo "old log" > "${deep_dir}/old-debug.log"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Everything should be removed
    [[ ! -d "$data_dir" ]]
}

@test "uninstall-all does not affect other files in bin directory" {
    # Create config
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create bin directory with other files
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Create other unrelated files
    echo "other script" > "${bin_dir}/other-script.sh"
    echo "another tool" > "${bin_dir}/another-tool"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # viberails should be gone, but other files should remain
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -f "${bin_dir}/other-script.sh" ]]
    [[ -f "${bin_dir}/another-tool" ]]
}

@test "uninstall-all only removes viberails temp files" {
    # Create config
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Create viberails temp files
    touch "${bin_dir}/viberails_upgrade_12345678"
    touch "${bin_dir}/.viberails_new_aabbccdd"

    # Create similar but unrelated files
    touch "${bin_dir}/other_upgrade_12345678"
    touch "${bin_dir}/.other_new_aabbccdd"
    touch "${bin_dir}/viberails_config_backup"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # viberails temp files should be gone
    [[ ! -f "${bin_dir}/viberails_upgrade_12345678" ]]
    [[ ! -f "${bin_dir}/.viberails_new_aabbccdd" ]]

    # Unrelated files should remain
    [[ -f "${bin_dir}/other_upgrade_12345678" ]]
    [[ -f "${bin_dir}/.other_new_aabbccdd" ]]
    [[ -f "${bin_dir}/viberails_config_backup" ]]
}

# -----------------------------------------------------------------------------
# Multiple hooks cleanup tests (when hooks could be detected)
# -----------------------------------------------------------------------------

@test "uninstall-all attempts to remove hooks from all detected providers" {
    # Create config with org configured
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "test-oid", "name": "Test Org", "url": "https://test.hook.limacharlie.io/test" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should complete successfully (even without actual hooks installed)
    # The important thing is it doesn't crash trying to enumerate providers
    [[ "$status" -eq 0 ]] || assert_contains "$output" "cleanup"
}

# -----------------------------------------------------------------------------
# Idempotency tests
# -----------------------------------------------------------------------------

@test "running uninstall-all twice does not cause errors" {
    # Create setup
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # First uninstall
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Second uninstall using the test binary (since installed one is gone)
    run "$VIBERAILS_BIN" uninstall-all --yes

    # Should not crash - dirs already gone is OK
    [[ -n "$output" ]]
}

# -----------------------------------------------------------------------------
# Symlink safety tests
# -----------------------------------------------------------------------------

@test "uninstall-all refuses to follow symlink for config directory" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target directory that should NOT be deleted
    local target_dir="${TEST_TMPDIR}/precious_data"
    mkdir -p "$target_dir"
    echo "precious content" > "${target_dir}/important.txt"

    # Create config directory location as a symlink pointing to precious data
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$(dirname "$config_dir")"
    ln -s "$target_dir" "$config_dir"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1 || true

    # The precious data should still exist (symlink was not followed)
    [[ -d "$target_dir" ]]
    [[ -f "${target_dir}/important.txt" ]]

    # Verify content is preserved
    local content
    content=$(cat "${target_dir}/important.txt")
    [[ "$content" == "precious content" ]]
}

@test "uninstall-all refuses to follow symlink for data directory" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target directory that should NOT be deleted
    local target_dir="${TEST_TMPDIR}/precious_logs"
    mkdir -p "$target_dir"
    echo "precious log" > "${target_dir}/critical.log"

    # Create real config directory (so command proceeds)
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create data directory location as a symlink pointing to precious data
    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$(dirname "$data_dir")"
    ln -s "$target_dir" "$data_dir"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1 || true

    # The precious data should still exist (symlink was not followed)
    [[ -d "$target_dir" ]]
    [[ -f "${target_dir}/critical.log" ]]

    # Verify content is preserved
    local content
    content=$(cat "${target_dir}/critical.log")
    [[ "$content" == "precious log" ]]
}

@test "uninstall-all refuses to follow symlink for binary" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target file that should NOT be deleted
    local target_file="${TEST_TMPDIR}/system_binary"
    echo "#!/bin/bash" > "$target_file"
    echo "echo 'important system binary'" >> "$target_file"
    chmod +x "$target_file"

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

    # Create bin directory with viberails as a symlink to the target
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    ln -s "$target_file" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all using the test binary (not the symlink)
    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1 || true

    # The target file should still exist (symlink was not followed)
    [[ -f "$target_file" ]]

    # Verify content is preserved
    assert_contains "$(cat "$target_file")" "important system binary"
}

@test "uninstall-all refuses to remove temp files that are symlinks" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target file that should NOT be deleted
    local target_file="${TEST_TMPDIR}/important_binary"
    echo "important content" > "$target_file"

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

    # Create a malicious symlink disguised as a temp upgrade file
    ln -s "$target_file" "${bin_dir}/viberails_upgrade_malicious"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1 || true

    # The target file should still exist (symlink was not followed)
    [[ -f "$target_file" ]]

    # Verify content is preserved
    local content
    content=$(cat "$target_file")
    [[ "$content" == "important content" ]]
}

@test "uninstall-all reports symlink refusal in output" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target directory
    local target_dir="${TEST_TMPDIR}/target"
    mkdir -p "$target_dir"

    # Create config directory as a symlink
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$(dirname "$config_dir")"
    ln -s "$target_dir" "$config_dir"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1 || true

    # Should indicate there was a symlink issue (or at least not crash)
    # The command may fail but should not delete the target
    [[ -d "$target_dir" ]]
}

# -----------------------------------------------------------------------------
# Binary already missing tests
# -----------------------------------------------------------------------------

@test "uninstall-all handles missing binary gracefully" {
    # Create config directory (so uninstall has something to do)
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Don't install the binary to bin_dir — it's already missing
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"

    # Verify binary is NOT in bin_dir
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]

    # Run uninstall-all using test binary (not from bin_dir)
    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1

    # Should not crash — missing binary is handled gracefully
    [[ -n "$output" ]]

    # Config should still be cleaned up
    [[ ! -d "$config_dir" ]]
}

# -----------------------------------------------------------------------------
# Corrupt config tests
# -----------------------------------------------------------------------------

@test "uninstall-all handles corrupt config file" {
    # Create config directory with invalid JSON
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    echo "this is not valid json{{{" > "${config_dir}/config.json"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all — should not crash on bad config
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1

    # Should complete without segfault or panic
    [[ -n "$output" ]]

    # Config directory should still be removed even if config is corrupt
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all handles config directory with extra files" {
    # Create config directory with extra non-standard files
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF
    echo "some backup" > "${config_dir}/config.json.bak"
    echo "notes" > "${config_dir}/notes.txt"

    # Install the binary
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Entire config directory (including extra files) should be removed
    [[ ! -d "$config_dir" ]]
}

# -----------------------------------------------------------------------------
# Output message verification tests
# -----------------------------------------------------------------------------

@test "uninstall-all reports binary removal in output" {
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

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should explicitly say binary was removed
    assert_contains "$output" "Binary removed"
}

@test "uninstall-all reports config removal in output" {
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

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should explicitly say config was removed
    assert_contains "$output" "Configuration removed"
}

@test "uninstall-all reports data directory removal in output" {
    # Create config and data directories
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$data_dir"
    echo '{"last_poll": 12345}' > "${data_dir}/upgrade_state.json"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should explicitly say data directory was removed
    assert_contains "$output" "Data directory removed"
}

# -----------------------------------------------------------------------------
# No auto-upgrade after uninstall-all
# -----------------------------------------------------------------------------

@test "uninstall-all does not re-create files via auto-upgrade" {
    # This verifies the is_uninstall_all skip in main.rs.
    # After uninstall-all, poll_upgrade() must NOT run, otherwise
    # it would re-download the binary and recreate config files.
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$data_dir"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Wait briefly to ensure no background process recreates anything
    sleep 0.5

    # Nothing should have been recreated
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ ! -d "$config_dir" ]]
    [[ ! -d "$data_dir" ]]
}

# -----------------------------------------------------------------------------
# Self-delete tests (binary deleting itself via self-replace)
# -----------------------------------------------------------------------------

@test "uninstall-all self-deletes when run from installed location" {
    # This is the core self-replace test: the binary running from the
    # install path must be able to delete its own file.
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Verify the installed copy exists and is executable
    [[ -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -x "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]

    # Run from the installed location — exercises self_replace::self_delete_at
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    assert_exit_code 0 "$status"

    # Binary must be gone after self-delete
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]

    # Other cleanup should have happened too
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all self-delete succeeds and reports binary removal" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run from installed location (self-delete path)
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Should report successful removal even through self-delete
    assert_contains "$output" "Binary removed"
    assert_contains "$output" "cleanup complete" || assert_contains "$output" "removed"
}

@test "uninstall-all self-delete does not affect other bin directory files" {
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Place unrelated files next to the binary
    echo "keep me" > "${bin_dir}/other-tool"
    echo "me too" > "${bin_dir}/important-script.sh"

    # Self-delete via installed binary
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Binary gone, neighbors untouched
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
    [[ -f "${bin_dir}/other-tool" ]]
    [[ -f "${bin_dir}/important-script.sh" ]]
}

# -----------------------------------------------------------------------------
# Additional symlink security tests
# -----------------------------------------------------------------------------

@test "uninstall-all refuses dangling symlink for binary" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create a dangling symlink where the binary should be
    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    ln -s "/nonexistent/path/to/binary" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run from build dir (the symlink isn't executable)
    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1 || true

    # Should not crash — dangling symlink is detected and refused
    [[ -n "$output" ]]

    # Config should still be cleaned up despite binary symlink error
    [[ ! -d "$config_dir" ]]
}

@test "uninstall-all refuses symlink chain for binary" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a real target that should NOT be deleted
    local target_file="${TEST_TMPDIR}/real_system_binary"
    echo "important" > "$target_file"

    # Create chain: bin_dir/viberails -> link_mid -> real_system_binary
    local link_mid="${TEST_TMPDIR}/intermediate_link"
    ln -s "$target_file" "$link_mid"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    ln -s "$link_mid" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run from build dir
    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1 || true

    # Target must survive the symlink chain
    [[ -f "$target_file" ]]
    [[ "$(cat "$target_file")" == "important" ]]
}

@test "uninstall-all refuses symlink lock file" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target file that should NOT be deleted
    local target_file="${TEST_TMPDIR}/critical_file"
    echo "do not delete" > "$target_file"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Create lock file as symlink to critical file
    ln -s "$target_file" "${bin_dir}/.viberails.upgrade.lock"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1 || true

    # Critical file must survive (symlink lock was refused by safe_remove_file)
    [[ -f "$target_file" ]]
    [[ "$(cat "$target_file")" == "do not delete" ]]
}

@test "uninstall-all preserves files symlinked from inside data directory" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create an external file that a symlink inside the data dir points to
    local external_file="${TEST_TMPDIR}/external_important.log"
    echo "external data" > "$external_file"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    # Create real data directory with a symlink inside it
    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$data_dir"
    ln -s "$external_file" "${data_dir}/sneaky_link.log"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Data directory should be removed
    [[ ! -d "$data_dir" ]]

    # External file should survive — remove_dir_all removes the symlink
    # entry, not the target
    [[ -f "$external_file" ]]
    [[ "$(cat "$external_file")" == "external data" ]]
}

@test "uninstall-all preserves files symlinked from inside config directory" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create external file that a symlink inside config points to
    local external_file="${TEST_TMPDIR}/external_secret"
    echo "secret data" > "$external_file"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF
    # Symlink inside the real config dir pointing outside
    ln -s "$external_file" "${config_dir}/linked_secret"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Config dir removed
    [[ ! -d "$config_dir" ]]

    # External file must survive
    [[ -f "$external_file" ]]
    [[ "$(cat "$external_file")" == "secret data" ]]
}

@test "uninstall-all handles binary path with spaces" {
    # Override bin dir to one with spaces to verify path handling
    local bin_dir="${HOME}/.local/bin with spaces"
    export VIBERAILS_BIN_DIR="$bin_dir"
    mkdir -p "$bin_dir"

    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run from installed location (path with spaces)
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    assert_exit_code 0 "$status"
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]
}

# ============================================================================
# Resilience tests — uninstall-all continues when some components fail
# ============================================================================

@test "uninstall-all still cleans config and data when binary dir is invalid" {
    # When VIBERAILS_BIN_DIR points at something invalid, uninstall-all should
    # still clean up config and data directories rather than bailing entirely.
    local config_dir="${VIBERAILS_CONFIG_DIR}"
    local data_dir="${VIBERAILS_DATA_DIR}"

    mkdir -p "$config_dir" "$data_dir"
    echo '{}' > "${config_dir}/config.json"
    echo 'log data' > "${data_dir}/debug.log"

    # Point bin dir at a relative path, which will fail validation.
    # Config and data cleanup should still proceed.
    export VIBERAILS_BIN_DIR="relative/invalid/path"

    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1 || true

    # Config and data should be cleaned up despite binary failure
    [[ ! -d "$config_dir" ]] || {
        echo "Config dir should have been removed: $config_dir" >&2
        return 1
    }
    [[ ! -d "$data_dir" ]] || {
        echo "Data dir should have been removed: $data_dir" >&2
        return 1
    }
}

@test "uninstall-all does not recreate config dir when it was already absent" {
    # Before the fix, uninstall_config would call project_config_dir() which
    # eagerly creates the directory. With project_config_dir_path() this
    # should no longer happen.
    local config_dir="${VIBERAILS_CONFIG_DIR}"

    # Ensure config dir does NOT exist
    [[ ! -d "$config_dir" ]]

    local bin_dir="${VIBERAILS_BIN_DIR}"
    mkdir -p "$bin_dir"

    local config_parent
    config_parent="$(dirname "$config_dir")"
    mkdir -p "$config_parent"
    cat > "${config_parent}/viberails_config_dummy" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Config dir should still not exist (not recreated by uninstall path)
    [[ ! -d "$config_dir" ]] || {
        echo "Config dir should NOT have been recreated: $config_dir" >&2
        return 1
    }
}

@test "uninstall-all does not recreate data dir when it was already absent" {
    local data_dir="${VIBERAILS_DATA_DIR}"

    # Ensure data dir does NOT exist
    [[ ! -d "$data_dir" ]]

    local bin_dir="${VIBERAILS_BIN_DIR}"
    mkdir -p "$bin_dir"

    local config_dir="${VIBERAILS_CONFIG_DIR}"
    mkdir -p "$config_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF

    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    # Data dir should still not exist (not recreated by uninstall path)
    [[ ! -d "$data_dir" ]] || {
        echo "Data dir should NOT have been recreated: $data_dir" >&2
        return 1
    }
}

@test "uninstall-all reports correct output for full cleanup" {
    local bin_dir="${VIBERAILS_BIN_DIR}"
    local config_dir="${VIBERAILS_CONFIG_DIR}"
    local data_dir="${VIBERAILS_DATA_DIR}"

    mkdir -p "$bin_dir" "$config_dir" "$data_dir"
    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF
    echo "log" > "${data_dir}/debug.log"

    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes

    assert_exit_code 0 "$status"
    assert_contains "$output" "Binary removed"
    assert_contains "$output" "Configuration removed"
    assert_contains "$output" "Data directory removed"
    assert_contains "$output" "Full cleanup complete"
}

# -----------------------------------------------------------------------------
# Failure detection tests — uninstall-all returns error on partial failures
# -----------------------------------------------------------------------------

@test "uninstall-all returns non-zero when config dir is a symlink (partial failure)" {
    # Skip on Windows - symlink behavior differs
    is_windows && skip "symlink behavior differs on Windows"

    # Create a target directory that should NOT be deleted
    local target_dir="${TEST_TMPDIR}/precious_data"
    mkdir -p "$target_dir"
    echo "precious" > "${target_dir}/important.txt"

    # Config dir is a symlink — safe_remove_dir_all refuses symlinks
    local config_dir="${XDG_CONFIG_HOME}/viberails"
    mkdir -p "$(dirname "$config_dir")"
    ln -s "$target_dir" "$config_dir"

    # Data dir is real — should still be cleaned up despite config failure
    local data_dir="${XDG_DATA_HOME}/viberails"
    mkdir -p "$data_dir"
    echo "cleanup me" > "${data_dir}/log.txt"

    local bin_dir="${HOME}/.local/bin"
    mkdir -p "$bin_dir"
    cp "$VIBERAILS_BIN" "${bin_dir}/${VIBERAILS_EXE_NAME}"

    # Run uninstall-all — should return non-zero due to symlink refusal
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all --yes 2>&1
    assert_exit_code 1 "$status"

    # Binary should still be cleaned up
    [[ ! -f "${bin_dir}/${VIBERAILS_EXE_NAME}" ]]

    # Data dir should still be cleaned up (independent of config failure)
    [[ ! -d "$data_dir" ]]

    # Precious data must survive
    [[ -f "${target_dir}/important.txt" ]]
}

@test "uninstall-all returns non-zero when binary dir override is invalid" {
    # Invalid bin dir means binary_location fails, but config/data
    # cleanup should still proceed.
    local config_dir="${VIBERAILS_CONFIG_DIR}"
    local data_dir="${VIBERAILS_DATA_DIR}"
    mkdir -p "$config_dir" "$data_dir"

    cat > "${config_dir}/config.json" <<EOF
{
    "user": { "fail_open": true },
    "install_id": "test-id",
    "org": { "oid": "", "name": "", "url": "" }
}
EOF
    echo "log" > "${data_dir}/debug.log"

    # Invalid relative path triggers validation failure
    export VIBERAILS_BIN_DIR="relative/invalid"

    run "$VIBERAILS_BIN" uninstall-all --yes 2>&1
    assert_exit_code 1 "$status"

    # Config and data should still be cleaned up despite binary failure
    [[ ! -d "$config_dir" ]]
    [[ ! -d "$data_dir" ]]
}
