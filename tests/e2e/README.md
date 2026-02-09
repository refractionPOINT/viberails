# End-to-End Tests for Viberails

This directory contains bash-based end-to-end tests that exercise various viberails functions
by directly invoking the CLI and testing various scenarios.

## What These Tests Cover

### Show Config Tests (`show_config.bats`)

Tests for the `show-config` command:
- **Basic functionality**: Help output, configuration display
- **Other Settings section**: Verifies Debug Mode and Auto Upgrade are shown in separate section
- **Auto Upgrade config**: Setting display, enabling/disabling, persistence
- **Backwards compatibility**: Old config formats without `debug` or `auto_upgrade` fields work correctly
- **Debug mode display**: Shows enabled/disabled status correctly

All tests use mock config data written to isolated temp directories.

### Upgrade Command Tests (`upgrade.bats`)

Tests for the auto-upgrade functionality:
- **CLI flags**: Help output, `--force`/`-f` flags, version display
- **Concurrent execution**: Lock file prevents multiple simultaneous upgrades
- **Error handling**: Graceful failure on network errors, invalid responses
- **Cleanup**: Temp files and old upgrade binaries are properly removed
- **Background behavior**: Silent upgrade checks don't pollute normal command output

Note: Some upgrade tests require network access to the embedded upgrade URL and will
timeout gracefully when unreachable. Full upgrade flow testing is limited since the
upgrade URL is compiled into the binary.

### Codex Metadata Tests (`codex_ppid.bats`)

Tests for Codex callback cloud metadata:
- **PPID field presence**: Verifies `meta_data.ppid` is included in outbound request payload
- **PPID validity**: Verifies `meta_data.ppid` is a positive integer when emitted
- **Metadata integrity**: Verifies the request still carries expected provider and notify fields

These tests use a local mock HTTP server and isolated config data per test run.

## Prerequisites

### Required

- **bats-core** - Bash Automated Testing System
- **cargo** - Rust build tool (to build the binary)

### Optional (some tests skipped without these)

- **flock** - For lock file tests (may be missing on macOS)

## Installation

### Ubuntu/Debian

```bash
sudo apt-get update
sudo apt-get install -y bats
```

### macOS

```bash
brew install bats-core
```

### From Source

```bash
git clone https://github.com/bats-core/bats-core.git
cd bats-core
sudo ./install.sh /usr/local
```

## Running Tests

### Quick Start

```bash
# Build the binary first
cargo build

# Run all e2e tests
./tests/e2e/run_tests.sh
```

### Manual Execution

```bash
# Run all test files
bats tests/e2e/*.bats

# Run specific test file
bats tests/e2e/upgrade.bats

# Run with verbose output (shows each command)
bats --verbose-run tests/e2e/*.bats

# Run with TAP output format
bats --tap tests/e2e/*.bats

# Run specific test by name pattern
bats tests/e2e/*.bats --filter "force"
```

### CI Integration

The tests run automatically in GitHub Actions on:
- Pull requests to `master`
- Pushes to `master`
- Tag releases

The e2e job executes `bats tests/e2e/*.bats`, so newly added `.bats` files
(including `codex_ppid.bats`) are picked up automatically.

See `.github/workflows/build.yml` for the workflow configuration.

## Test Environment

Each test runs in an isolated environment with:

| Variable | Value |
|----------|-------|
| `HOME` | Temp directory (`$TEST_TMPDIR/home`) |
| `XDG_CONFIG_HOME` | Temp directory (`$TEST_TMPDIR/config`) |
| `XDG_DATA_HOME` | Temp directory (`$TEST_TMPDIR/data`) |

This ensures tests don't interfere with your actual configuration.

## Test Files

| File | Purpose |
|------|---------|
| `test_helpers.bash` | Common setup, teardown, and assertion functions |
| `show_config.bats` | Show-config command and backwards compatibility tests |
| `codex_ppid.bats` | Codex callback cloud payload metadata checks (`meta_data.ppid`) |
| `upgrade.bats` | Upgrade command behavior tests |
| `run_tests.sh` | Test runner with prerequisites check |
| `README.md` | This documentation |

## Helper Functions

The `test_helpers.bash` file provides:

```bash
# Setup/teardown
setup_test          # Create isolated temp environment
teardown_test       # Clean up temp files and processes

# Assertions
assert_contains "$output" "expected"      # Check substring exists
assert_not_contains "$output" "bad"       # Check substring absent
assert_exit_code 0 "$status"              # Check exit code

# Lock testing
create_upgrade_lock             # Simulate another upgrade in progress
release_upgrade_lock            # Release the lock
```

## Adding New Tests

1. Create test functions in existing `.bats` files or create new ones:

```bash
@test "descriptive test name" {
    # Setup
    setup_test

    # Run command
    run "$VIBERAILS_BIN" upgrade --some-flag

    # Assertions
    assert_exit_code 0 "$status"
    assert_contains "$output" "expected output"
}
```

2. Use `skip "reason"` for tests that can't run in all environments:

```bash
@test "requires flock" {
    command -v flock >/dev/null || skip "flock not available"
    # ... rest of test
}
```

3. Always clean up in teardown (handled automatically if using `setup_test`/`teardown_test`).

## Limitations

Some tests are skipped because:

- **Embedded upgrade URL**: The upgrade URL is compiled into the binary, so full upgrade flow tests require a real release server or custom build.
- **Platform-specific tools**: `flock` may not be available on all platforms.
- **Network access**: Some tests that would require real network access are skipped.

## Troubleshooting

### Tests fail with "Binary not found"

```bash
cargo build  # Build the binary first
```

### Tests fail with "bats not found"

Install bats using the instructions above.

### Lock tests skipped on macOS

macOS doesn't have `flock` by default. Install via:

```bash
brew install flock
```
