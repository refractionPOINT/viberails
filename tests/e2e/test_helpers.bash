#!/usr/bin/env bash
# Test helpers for viberails e2e tests
#
# This file provides common setup, teardown, and utility functions
# for bats-based end-to-end tests.

# Get the project root directory
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Build directory for test binary
BUILD_DIR="${PROJECT_ROOT}/target/debug"

# Test temporary directory (set per-test)
TEST_TMPDIR=""

# Path to the viberails binary under test
VIBERAILS_BIN=""

# Binary name with platform-appropriate extension
VIBERAILS_EXE_NAME=""

# Check if running on Windows (Git Bash/MSYS)
is_windows() {
    [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]
}

# Get the binary name with correct extension for current platform
get_exe_name() {
    if is_windows; then
        echo "viberails.exe"
    else
        echo "viberails"
    fi
}

# Check if timeout command is available
has_timeout() {
    command -v timeout >/dev/null 2>&1
}

# Run command with timeout if available, otherwise run directly
# Usage: run_with_timeout <seconds> <command> [args...]
run_with_timeout() {
    local seconds="$1"
    shift
    if has_timeout; then
        timeout "$seconds" "$@"
    else
        "$@"
    fi
}

# Setup function to be called at the start of each test
setup_test() {
    # Create isolated temp directory for this test
    TEST_TMPDIR="$(mktemp -d)"
    export HOME="${TEST_TMPDIR}/home"
    export XDG_CONFIG_HOME="${TEST_TMPDIR}/config"
    export XDG_DATA_HOME="${TEST_TMPDIR}/data"

    # Override binary installation directory for test isolation
    # This ensures upgrade/install operations don't touch the real ~/.local/bin
    export VIBERAILS_BIN_DIR="${HOME}/.local/bin"

    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$VIBERAILS_BIN_DIR"

    # Set binary name and path (handle Windows .exe extension)
    VIBERAILS_EXE_NAME="$(get_exe_name)"
    VIBERAILS_BIN="${BUILD_DIR}/${VIBERAILS_EXE_NAME}"

    # Ensure binary exists
    if [[ ! -f "$VIBERAILS_BIN" ]]; then
        echo "Binary not found at $VIBERAILS_BIN - run 'cargo build' first" >&2
        return 1
    fi
}

# Teardown function to be called at the end of each test
teardown_test() {
    # Stop lock holder if running
    if [[ -n "${LOCK_HOLDER_PID:-}" ]] && kill -0 "$LOCK_HOLDER_PID" 2>/dev/null; then
        kill "$LOCK_HOLDER_PID" 2>/dev/null || true
        wait "$LOCK_HOLDER_PID" 2>/dev/null || true
    fi
    LOCK_HOLDER_PID=""

    # Clean up temp directory
    if [[ -n "$TEST_TMPDIR" && -d "$TEST_TMPDIR" ]]; then
        rm -rf "$TEST_TMPDIR"
    fi
    TEST_TMPDIR=""
}

# Build the project if needed
ensure_binary_built() {
    if [[ ! -x "${BUILD_DIR}/viberails" ]]; then
        echo "Building viberails..." >&2
        (cd "$PROJECT_ROOT" && cargo build --quiet)
    fi
}

# Get the version from the built binary
get_binary_version() {
    "$VIBERAILS_BIN" --version 2>/dev/null | head -1 | awk '{print $2}'
}

# Create a lock file to simulate another upgrade in progress
# Usage: create_upgrade_lock
# Returns: Path to lock file
create_upgrade_lock() {
    local lock_dir="${HOME}/.local/bin"
    mkdir -p "$lock_dir"
    local lock_file="${lock_dir}/.viberails.upgrade.lock"

    # Write current PID to lock file and hold it open
    echo "$$" > "$lock_file"

    # Use flock to hold the lock (runs in background)
    (
        exec 200>"$lock_file"
        flock -x 200
        # Hold lock briefly - test should complete before this expires
        sleep 3
    ) &
    LOCK_HOLDER_PID=$!

    # Wait for lock to be acquired
    sleep 0.2
    echo "$lock_file"
}

# Release the upgrade lock
release_upgrade_lock() {
    if [[ -n "${LOCK_HOLDER_PID:-}" ]]; then
        kill "$LOCK_HOLDER_PID" 2>/dev/null || true
        wait "$LOCK_HOLDER_PID" 2>/dev/null || true
    fi
}

# Assert that output contains a string
# Usage: assert_contains "$output" "expected string"
assert_contains() {
    local haystack="$1"
    local needle="$2"
    if [[ "$haystack" != *"$needle"* ]]; then
        echo "Expected output to contain: $needle" >&2
        echo "Actual output: $haystack" >&2
        return 1
    fi
}

# Assert that output does not contain a string
# Usage: assert_not_contains "$output" "unexpected string"
assert_not_contains() {
    local haystack="$1"
    local needle="$2"
    if [[ "$haystack" == *"$needle"* ]]; then
        echo "Expected output to NOT contain: $needle" >&2
        echo "Actual output: $haystack" >&2
        return 1
    fi
}

# Assert command exits with expected status
# Usage: assert_exit_code <expected> <actual>
assert_exit_code() {
    local expected="$1"
    local actual="$2"
    if [[ "$actual" -ne "$expected" ]]; then
        echo "Expected exit code $expected, got $actual" >&2
        return 1
    fi
}
