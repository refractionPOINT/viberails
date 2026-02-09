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

# PID of test mock hook server process (set by start_mock_hook_server)
MOCK_SERVER_PID=""

# Check if running on Windows (Git Bash/MSYS)
is_windows() {
    [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]
}

# Resolve the python interpreter command for the current platform.
# Windows GHA runners ship "python" while Linux/macOS use "python3".
#
# Parameters: None
#
# Return:
#   Prints the python command name to stdout and exits 0 on success.
#   Exits 1 if no usable python >=3 is found.
get_python_cmd() {
    local cmd
    for cmd in python3 python; do
        if command -v "$cmd" >/dev/null 2>&1; then
            # Verify it's Python 3+
            if "$cmd" -c "import sys; sys.exit(0 if sys.version_info >= (3, 0) else 1)" 2>/dev/null; then
                echo "$cmd"
                return 0
            fi
        fi
    done
    return 1
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

# Acquire an available local TCP port for ephemeral test servers.
#
# Parameters: None
#
# Return:
#   Prints a port number to stdout and exits 0 on success.
#   Exits non-zero if python is unavailable or port selection fails.
get_available_tcp_port() {
    local py
    py="$(get_python_cmd)" || return 1

    "$py" -c "
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.bind(('127.0.0.1', 0))
print(s.getsockname()[1])
s.close()
"
}

# Terminate stale mock hook server processes from interrupted test runs.
# Best-effort: silently succeeds if process listing tools are unavailable.
#
# Parameters: None
#
# Return:
#   0 always
cleanup_stray_mock_hook_servers() {
    local script_name="mock_hook_server.py"
    local current_pid="$$"
    local pid

    # Use pgrep when available (Linux/macOS). On platforms without pgrep
    # (e.g. Windows/MSYS) this is a no-op — each test uses ephemeral ports
    # so stale servers from prior runs don't cause conflicts.
    command -v pgrep >/dev/null 2>&1 || return 0

    while IFS= read -r pid; do
        [[ -z "$pid" ]] && continue
        [[ "$pid" == "$current_pid" ]] && continue
        kill "$pid" 2>/dev/null || true
    done < <(pgrep -f "$script_name" 2>/dev/null || true)
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
    # Ensure interrupted/aborted test runs cannot leak stale mock servers.
    cleanup_stray_mock_hook_servers

    # Create isolated temp directory for this test
    TEST_TMPDIR="$(mktemp -d)"
    export HOME="${TEST_TMPDIR}/home"
    export XDG_CONFIG_HOME="${TEST_TMPDIR}/config"
    export XDG_DATA_HOME="${TEST_TMPDIR}/data"

    # Override directories for test isolation.
    # VIBERAILS_*_DIR env vars ensure isolation on all platforms —
    # macOS and Windows ignore XDG_CONFIG_HOME/XDG_DATA_HOME in their
    # native directory APIs (dirs::config_dir(), dirs::data_dir()).
    export VIBERAILS_BIN_DIR="${HOME}/.local/bin"
    export VIBERAILS_CONFIG_DIR="${XDG_CONFIG_HOME}/viberails"
    export VIBERAILS_DATA_DIR="${XDG_DATA_HOME}/viberails"

    # Only create parent directories — not the viberails-specific dirs themselves.
    # The binary creates config/data dirs on demand via create_secure_directory().
    # Tests that need them pre-created will set them up explicitly.
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
    # Stop mock server if running
    if [[ -n "${MOCK_SERVER_PID:-}" ]] && kill -0 "$MOCK_SERVER_PID" 2>/dev/null; then
        kill "$MOCK_SERVER_PID" 2>/dev/null || true
        wait "$MOCK_SERVER_PID" 2>/dev/null || true
    fi
    MOCK_SERVER_PID=""

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

# Wait until a TCP port accepts connections or timeout expires.
#
# Parameters:
#   $1: TCP port on 127.0.0.1 to probe
#   $2: Timeout in seconds (default: 10)
#
# Return:
#   0 when the port is accepting connections, 1 on timeout.
wait_for_port() {
    local port="$1"
    local timeout_secs="${2:-10}"
    local deadline=$((SECONDS + timeout_secs))

    local py
    py="$(get_python_cmd)" || return 1

    while [[ $SECONDS -lt $deadline ]]; do
        if "$py" -c "
import socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(0.5)
try:
    s.connect(('127.0.0.1', int(sys.argv[1])))
    s.close()
except Exception:
    sys.exit(1)
" "$port" 2>/dev/null; then
            return 0
        fi
        sleep 0.2
    done
    return 1
}

# Start a local mock hook server that captures one JSON request body.
#
# Parameters:
#   $1: TCP port to listen on
#   $2: Output file path where the raw JSON body is written
#
# Return:
#   0 on success, non-zero if startup fails
start_mock_hook_server() {
    local port="$1"
    local capture_file="$2"
    local log_file="${TEST_TMPDIR}/mock_hook_server.log"

    local py
    py="$(get_python_cmd)" || { echo "python3/python not found" >&2; return 1; }

    # Clean any stale mock server before attempting startup on this test run.
    cleanup_stray_mock_hook_servers

    "$py" "${PROJECT_ROOT}/tests/e2e/mock_hook_server.py" \
        --port "$port" \
        --capture-file "$capture_file" \
        >"$log_file" 2>&1 &
    MOCK_SERVER_PID=$!

    # Actively poll until the server accepts TCP connections (10s timeout).
    if ! wait_for_port "$port" 10; then
        echo "Mock hook server failed to start within 10s (log follows):" >&2
        cat "$log_file" >&2 2>/dev/null || true
        return 1
    fi
}

# Stop the local mock hook server, if running.
#
# Parameters: None
#
# Return:
#   0 always
stop_mock_hook_server() {
    if [[ -n "${MOCK_SERVER_PID:-}" ]] && kill -0 "$MOCK_SERVER_PID" 2>/dev/null; then
        kill "$MOCK_SERVER_PID" 2>/dev/null || true
        wait "$MOCK_SERVER_PID" 2>/dev/null || true
    fi
    MOCK_SERVER_PID=""
}
