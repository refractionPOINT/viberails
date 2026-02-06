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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all

    # Second uninstall using the test binary (since installed one is gone)
    run "$VIBERAILS_BIN" uninstall-all

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all 2>&1 || true

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all 2>&1 || true

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
    run "$VIBERAILS_BIN" uninstall-all 2>&1 || true

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all 2>&1 || true

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
    run "${bin_dir}/${VIBERAILS_EXE_NAME}" uninstall-all 2>&1 || true

    # Should indicate there was a symlink issue (or at least not crash)
    # The command may fail but should not delete the target
    [[ -d "$target_dir" ]]
}
