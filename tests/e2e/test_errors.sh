#!/usr/bin/env bash
# E2E tests for caut error scenarios and edge cases
# Run from project root: ./tests/e2e/test_errors.sh

set -euo pipefail

# ==============================================================================
# Configuration
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CAUT_BIN="${CAUT_BIN:-$PROJECT_ROOT/target/debug/caut}"
LOG_DIR="${TEST_LOG_DIR:-$PROJECT_ROOT/test-logs}"
LOG_LEVEL="${TEST_LOG_LEVEL:-info}"
KEEP_ARTIFACTS="${TEST_KEEP_ARTIFACTS:-false}"

# Timestamp for this run
RUN_TS=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$LOG_DIR/test_errors_$RUN_TS.log"
JUNIT_FILE="$LOG_DIR/test_errors.xml"
ARTIFACTS_DIR="$LOG_DIR/artifacts"

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
declare -a JUNIT_RESULTS=()

# ==============================================================================
# Logging Functions
# ==============================================================================

log() {
    local level="${1:-INFO}"
    shift
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S.%3N')
    echo "[$timestamp] [$level] $*" | tee -a "$LOG_FILE"
}

log_info()  { log "INFO" "$@"; }
log_debug() { [[ "$LOG_LEVEL" =~ ^(debug|trace)$ ]] && log "DEBUG" "$@" || true; }
log_trace() { [[ "$LOG_LEVEL" == "trace" ]] && log "TRACE" "$@" || true; }
log_warn()  { log "WARN" "$@"; }
log_error() { log "ERROR" "$@"; }

log_section() {
    log_info "============================================================"
    log_info "$*"
    log_info "============================================================"
}

# ==============================================================================
# Test Infrastructure
# ==============================================================================

setup() {
    log_section "SETUP: Initializing test environment"

    mkdir -p "$LOG_DIR" "$ARTIFACTS_DIR"

    # Build caut if not present
    if [[ ! -x "$CAUT_BIN" ]]; then
        log_info "Building caut binary..."
        (cd "$PROJECT_ROOT" && cargo build --quiet)
    fi

    log_info "Log file: $LOG_FILE"
    log_info "Artifacts: $ARTIFACTS_DIR"
    log_info "Binary: $CAUT_BIN"
    log_info "Log level: $LOG_LEVEL"

    # Verify binary exists
    if [[ ! -x "$CAUT_BIN" ]]; then
        log_error "Binary not found: $CAUT_BIN"
        exit 1
    fi

    # Print version
    local version
    version=$("$CAUT_BIN" --version 2>&1 || echo "unknown")
    log_info "caut version: $version"
}

cleanup() {
    if [[ "$KEEP_ARTIFACTS" != "true" ]]; then
        log_debug "Cleanup: Keeping artifacts for inspection"
    fi

    # Create symlink to latest log
    ln -sf "$(basename "$LOG_FILE")" "$LOG_DIR/test_errors_latest.log" 2>/dev/null || true
}

# Run a single test case
run_test() {
    local test_name="$1"
    local description="$2"
    local test_func="$3"

    ((TESTS_RUN++))
    log_section "TEST [$TESTS_RUN]: $test_name"
    log_info "Description: $description"

    local start_time
    start_time=$(date +%s.%N)

    local exit_code=0
    local failure_msg=""

    # Run the test function, capturing output
    if output=$($test_func 2>&1); then
        log_info "PASS: $test_name"
        ((TESTS_PASSED++))
        JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.errors\"/>")
    else
        exit_code=$?
        failure_msg="$output"
        log_error "FAIL: $test_name (exit code: $exit_code)"
        log_error "Output: $failure_msg"
        ((TESTS_FAILED++))
        JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.errors\"><failure message=\"Test failed with exit code $exit_code\"><![CDATA[$failure_msg]]></failure></testcase>")
    fi

    local end_time
    end_time=$(date +%s.%N)
    local duration
    duration=$(echo "$end_time - $start_time" | bc)
    log_info "Duration: ${duration}s"

    return $exit_code
}

# Skip a test with reason
skip_test() {
    local test_name="$1"
    local reason="$2"

    ((TESTS_RUN++))
    ((TESTS_SKIPPED++))
    log_info "SKIP: $test_name - $reason"
    JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.errors\"><skipped message=\"$reason\"/></testcase>")
}

# ==============================================================================
# Test Cases
# ==============================================================================

test_invalid_provider() {
    log_debug "Running: $CAUT_BIN usage --provider=nonexistent_provider_xyz"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage --provider=nonexistent_provider_xyz 2>&1) || exit_code=$?

    echo "$output" > "$ARTIFACTS_DIR/errors_invalid_provider.txt"

    if [[ $exit_code -eq 0 ]]; then
        echo "Expected non-zero exit code for invalid provider"
        return 1
    fi

    if ! echo "$output" | grep -qiE "invalid|unknown|not found|available"; then
        echo "Error message not descriptive enough: $output"
        return 1
    fi

    return 0
}

test_invalid_command() {
    log_debug "Running: $CAUT_BIN notacommand"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" notacommand 2>&1) || exit_code=$?

    echo "$output" > "$ARTIFACTS_DIR/errors_invalid_command.txt"

    if [[ $exit_code -eq 0 ]]; then
        echo "Expected non-zero exit code for invalid command"
        return 1
    fi

    if ! echo "$output" | grep -qiE "unknown|unrecognized|invalid|error"; then
        echo "Error message not descriptive enough: $output"
        return 1
    fi

    return 0
}

test_help_flag() {
    log_debug "Running: $CAUT_BIN --help"

    local output
    output=$("$CAUT_BIN" --help 2>&1)

    echo "$output" > "$ARTIFACTS_DIR/errors_help_output.txt"

    if ! echo "$output" | grep -q "Usage:"; then
        echo "Help output missing Usage section"
        return 1
    fi

    return 0
}

test_version_flag() {
    log_debug "Running: $CAUT_BIN --version"

    local output
    output=$("$CAUT_BIN" --version 2>&1)

    echo "$output" > "$ARTIFACTS_DIR/errors_version_output.txt"

    if ! echo "$output" | grep -qE "caut [0-9]+\.[0-9]+\.[0-9]+"; then
        echo "Invalid version output: $output"
        return 1
    fi

    return 0
}

test_conflicting_flags() {
    log_debug "Running: $CAUT_BIN usage --all-accounts --account test"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage --all-accounts --account test 2>&1) || exit_code=$?

    echo "$output" > "$ARTIFACTS_DIR/errors_conflicting_flags.txt"

    if [[ $exit_code -eq 0 ]]; then
        echo "Expected non-zero exit code for conflicting flags"
        return 1
    fi

    if ! echo "$output" | grep -qi "all-accounts"; then
        echo "Conflict message missing --all-accounts detail"
        return 1
    fi

    return 0
}

test_corrupted_config_no_panic() {
    log_debug "Running: CAUT_CONFIG=<corrupted> $CAUT_BIN usage"

    local temp_config
    temp_config=$(mktemp)
    echo "this is not valid toml {{{{ " > "$temp_config"

    local output
    local exit_code=0
    output=$(CAUT_CONFIG="$temp_config" "$CAUT_BIN" usage 2>&1) || exit_code=$?

    rm -f "$temp_config"

    echo "$output" > "$ARTIFACTS_DIR/errors_corrupted_config.txt"

    # Expect failure but no panic
    if echo "$output" | grep -qi "panic"; then
        echo "Panic detected with corrupted config"
        return 1
    fi

    # Exit code can be non-zero (expected)
    log_debug "Exit code: $exit_code"

    return 0
}

# ==============================================================================
# JUnit XML Generation
# ==============================================================================

generate_junit_xml() {
    log_info "Generating JUnit XML report"

    cat > "$JUNIT_FILE" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="caut_errors_e2e" tests="$TESTS_RUN" failures="$TESTS_FAILED" skipped="$TESTS_SKIPPED" timestamp="$(date -Iseconds)">
$(printf '%s\n' "${JUNIT_RESULTS[@]}")
</testsuite>
EOF

    log_info "JUnit XML saved to: $JUNIT_FILE"
}

# ==============================================================================
# Main Execution
# ==============================================================================

main() {
    trap cleanup EXIT

    setup

    log_section "RUNNING TEST SUITE: caut error scenarios"

    # Run all tests (continue on failure to get full report)
    set +e

    run_test "invalid_provider" "Invalid provider produces helpful error" test_invalid_provider
    run_test "invalid_command" "Unknown command is rejected" test_invalid_command
    run_test "help_flag" "Help flag exits cleanly" test_help_flag
    run_test "version_flag" "Version output format" test_version_flag
    run_test "conflicting_flags" "Conflicting flags are rejected" test_conflicting_flags
    run_test "corrupted_config" "Corrupted config does not panic" test_corrupted_config_no_panic

    set -e

    # Generate reports
    generate_junit_xml

    # Summary
    log_section "TEST SUMMARY"
    log_info "Tests run:    $TESTS_RUN"
    log_info "Tests passed: $TESTS_PASSED"
    log_info "Tests failed: $TESTS_FAILED"
    log_info "Tests skipped: $TESTS_SKIPPED"
    log_info ""
    log_info "Log file: $LOG_FILE"
    log_info "JUnit XML: $JUNIT_FILE"
    log_info "Artifacts: $ARTIFACTS_DIR"

    if [[ $TESTS_FAILED -gt 0 ]]; then
        log_error "SUITE FAILED: $TESTS_FAILED test(s) failed"
        exit 1
    fi

    log_info "SUITE PASSED: All tests passed"
    exit 0
}

# Run if executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
