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

# Setup function to be called at the start of each test
setup_test() {
    # Create isolated temp directory for this test
    TEST_TMPDIR="$(mktemp -d)"
    export HOME="${TEST_TMPDIR}/home"
    export XDG_CONFIG_HOME="${TEST_TMPDIR}/config"
    export XDG_DATA_HOME="${TEST_TMPDIR}/data"
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"

    # Set path to binary
    VIBERAILS_BIN="${BUILD_DIR}/viberails"

    # Ensure binary exists
    if [[ ! -x "$VIBERAILS_BIN" ]]; then
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
