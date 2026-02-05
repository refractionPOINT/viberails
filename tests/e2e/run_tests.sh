#!/usr/bin/env bash
# Run e2e tests for viberails
#
# Usage: ./tests/e2e/run_tests.sh [bats options]
#
# Prerequisites:
# - bats-core installed
# - cargo build completed
#
# Examples:
#   ./tests/e2e/run_tests.sh                    # Run all tests
#   ./tests/e2e/run_tests.sh --tap              # TAP output
#   ./tests/e2e/run_tests.sh upgrade.bats       # Run specific file

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    # Check for bats
    if ! command -v bats >/dev/null 2>&1; then
        log_error "bats not found. Install with:"
        echo "  brew install bats-core  # macOS"
        echo "  apt install bats        # Debian/Ubuntu"
        echo "  npm install -g bats     # npm"
        exit 1
    fi
    log_info "Found bats: $(bats --version)"

    # Check for python3 (used for mock server)
    if ! command -v python3 >/dev/null 2>&1; then
        log_warn "python3 not found - some tests may be skipped"
    fi

    # Check for flock (used for lock tests)
    if ! command -v flock >/dev/null 2>&1; then
        log_warn "flock not found - lock tests may be skipped"
    fi
}

# Build the project
build_project() {
    log_info "Building project..."
    (cd "$PROJECT_ROOT" && cargo build --quiet)
    if [[ ! -x "${PROJECT_ROOT}/target/debug/viberails" ]]; then
        log_error "Build failed - binary not found"
        exit 1
    fi
    log_info "Build complete"
}

# Run the tests
run_tests() {
    log_info "Running e2e tests..."

    local test_files=()
    local bats_args=()

    # Parse arguments
    for arg in "$@"; do
        if [[ "$arg" == *.bats ]]; then
            test_files+=("$SCRIPT_DIR/$arg")
        else
            bats_args+=("$arg")
        fi
    done

    # If no test files specified, run all
    if [[ ${#test_files[@]} -eq 0 ]]; then
        test_files=("$SCRIPT_DIR"/*.bats)
    fi

    # Run bats with timing enabled by default
    bats --timing "${bats_args[@]}" "${test_files[@]}"
}

# Main
main() {
    check_prerequisites
    build_project
    run_tests "$@"
}

main "$@"
