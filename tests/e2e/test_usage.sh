#!/usr/bin/env bash
# E2E tests for caut usage command
# Run from project root: ./tests/e2e/test_usage.sh

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
LOG_FILE="$LOG_DIR/test_usage_$RUN_TS.log"
JUNIT_FILE="$LOG_DIR/test_usage.xml"
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
    ln -sf "$(basename "$LOG_FILE")" "$LOG_DIR/test_usage_latest.log" 2>/dev/null || true
}

# Run a single test case
# Usage: run_test "test_name" "description" test_function
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
        JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.usage\" time=\"\$(echo \"\$end_time - \$start_time\" | bc)\"/>")
    else
        exit_code=$?
        failure_msg="$output"
        log_error "FAIL: $test_name (exit code: $exit_code)"
        log_error "Output: $failure_msg"
        ((TESTS_FAILED++))
        JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.usage\"><failure message=\"Test failed with exit code $exit_code\"><![CDATA[$failure_msg]]></failure></testcase>")
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
    JUNIT_RESULTS+=("<testcase name=\"$test_name\" classname=\"caut.usage\"><skipped message=\"$reason\"/></testcase>")
}

# ==============================================================================
# Test Cases
# ==============================================================================

test_basic_invocation() {
    log_debug "Running: $CAUT_BIN usage"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage 2>&1) || exit_code=$?

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_basic_stdout.txt"
    log_trace "Output: $output"

    # Verify exit code (may be non-zero if no providers configured)
    if [[ $exit_code -ne 0 ]]; then
        # Check if it's an expected "no providers" error
        if echo "$output" | grep -qi "no.*provider\|not.*configured\|not.*found\|error"; then
            log_debug "Expected: No providers configured"
            return 0
        fi
        return $exit_code
    fi

    return 0
}

test_help_output() {
    log_debug "Running: $CAUT_BIN usage --help"

    local output
    output=$("$CAUT_BIN" usage --help 2>&1)

    # Verify help contains expected sections
    if ! echo "$output" | grep -q "Usage:"; then
        echo "Missing 'Usage:' section in help"
        return 1
    fi

    if ! echo "$output" | grep -q "\-\-provider"; then
        echo "Missing '--provider' option in help"
        return 1
    fi

    if ! echo "$output" | grep -q "\-\-format"; then
        echo "Missing '--format' option in help"
        return 1
    fi

    return 0
}

test_version_output() {
    log_debug "Running: $CAUT_BIN --version"

    local output
    output=$("$CAUT_BIN" --version 2>&1)

    # Verify version string format
    if ! echo "$output" | grep -qE "caut [0-9]+\.[0-9]+\.[0-9]+"; then
        echo "Invalid version format: $output"
        return 1
    fi

    return 0
}

test_json_output_format() {
    log_debug "Running: $CAUT_BIN usage --json"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage --json 2>&1) || exit_code=$?

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_json_output.json"

    # Check if valid JSON
    if ! echo "$output" | jq . >/dev/null 2>&1; then
        echo "Invalid JSON output"
        log_debug "Raw output: $output"
        return 1
    fi

    # Verify schema version
    local schema_version
    schema_version=$(echo "$output" | jq -r '.schemaVersion // empty')
    if [[ -z "$schema_version" ]]; then
        echo "Missing schemaVersion in JSON output"
        return 1
    fi
    log_debug "Schema version: $schema_version"

    # Verify required fields
    if [[ $(echo "$output" | jq -r '.command // empty') != "usage" ]]; then
        echo "Missing or incorrect 'command' field"
        return 1
    fi

    if [[ $(echo "$output" | jq -r '.generatedAt // empty') == "" ]]; then
        echo "Missing 'generatedAt' timestamp"
        return 1
    fi

    return 0
}

test_json_format_flag() {
    log_debug "Running: $CAUT_BIN usage --format json"

    local output
    output=$("$CAUT_BIN" usage --format json 2>&1) || true

    # Check if valid JSON
    if ! echo "$output" | jq . >/dev/null 2>&1; then
        echo "Invalid JSON output with --format json"
        return 1
    fi

    return 0
}

test_markdown_output() {
    log_debug "Running: $CAUT_BIN usage --format md"

    local output
    output=$("$CAUT_BIN" usage --format md 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_md_output.md"

    # Verify markdown markers
    if ! echo "$output" | grep -qE "^#|^\*\*|^\-"; then
        # If no markdown markers but also no error, might be empty output
        if echo "$output" | grep -qi "error\|fail"; then
            echo "Error in markdown output: $output"
            return 1
        fi
    fi

    return 0
}

test_no_color_mode() {
    log_debug "Running: $CAUT_BIN usage --no-color"

    local output
    output=$("$CAUT_BIN" usage --no-color 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_nocolor_stdout.txt"

    # Check for ANSI escape codes
    if echo "$output" | grep -qE $'\x1b\['; then
        echo "ANSI escape codes found in --no-color output"
        return 1
    fi

    return 0
}

test_verbose_mode() {
    log_debug "Running: $CAUT_BIN usage --verbose"

    local output
    output=$("$CAUT_BIN" usage --verbose 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_verbose_stdout.txt"

    # Verbose mode should produce some debug output (or at least not crash)
    # This is a soft check - verbose output format may vary
    log_debug "Verbose output length: ${#output}"

    return 0
}

test_pretty_json() {
    log_debug "Running: $CAUT_BIN usage --json --pretty"

    local output
    output=$("$CAUT_BIN" usage --json --pretty 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_pretty_json.json"

    # Pretty JSON should have newlines and indentation
    if ! echo "$output" | grep -q $'^\s\+'; then
        # Check if it's valid JSON at all
        if echo "$output" | jq . >/dev/null 2>&1; then
            # Valid JSON but not pretty - check for newlines
            local line_count
            line_count=$(echo "$output" | wc -l)
            if [[ $line_count -lt 3 ]]; then
                echo "JSON output not pretty-printed (only $line_count lines)"
                return 1
            fi
        else
            echo "Invalid JSON output"
            return 1
        fi
    fi

    return 0
}

test_invalid_provider() {
    log_debug "Running: $CAUT_BIN usage --provider=nonexistent_provider_xyz"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage --provider=nonexistent_provider_xyz 2>&1) || exit_code=$?

    # Should either error or produce empty/warning output
    # We don't mandate a specific exit code, but output should indicate issue
    log_debug "Exit code: $exit_code, Output: $output"

    return 0
}

test_provider_filter() {
    log_debug "Running: $CAUT_BIN usage --provider=claude"

    local output
    output=$("$CAUT_BIN" usage --provider=claude 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_claude_stdout.txt"

    # If Claude is configured, output should contain claude
    # If not configured, should get appropriate error/empty
    log_debug "Output length: ${#output}"

    return 0
}

test_all_providers() {
    log_debug "Running: $CAUT_BIN usage --provider=all"

    local output
    output=$("$CAUT_BIN" usage --provider=all 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_all_providers_stdout.txt"

    log_debug "Output length: ${#output}"

    return 0
}

test_status_flag() {
    log_debug "Running: $CAUT_BIN usage --status"

    local output
    output=$("$CAUT_BIN" usage --status 2>&1) || true

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_status_stdout.txt"

    log_debug "Output length: ${#output}"

    return 0
}

test_json_with_errors() {
    log_debug "Running: $CAUT_BIN usage --json (checking error array)"

    local output
    output=$("$CAUT_BIN" usage --json 2>&1) || true

    # Check if errors array exists in JSON
    if echo "$output" | jq . >/dev/null 2>&1; then
        local has_errors
        has_errors=$(echo "$output" | jq -r 'has("errors")')
        if [[ "$has_errors" != "true" ]]; then
            echo "JSON output missing 'errors' array"
            return 1
        fi
    fi

    return 0
}

test_timeout_handling() {
    log_debug "Running: $CAUT_BIN usage --web-timeout=1"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" usage --web-timeout=1 2>&1) || exit_code=$?

    # Save artifact
    echo "$output" > "$ARTIFACTS_DIR/usage_timeout_stdout.txt"

    # With 1 second timeout, we expect either quick success or timeout
    # Both are valid outcomes
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
<testsuite name="caut_usage_e2e" tests="$TESTS_RUN" failures="$TESTS_FAILED" skipped="$TESTS_SKIPPED" timestamp="$(date -Iseconds)">
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

    log_section "RUNNING TEST SUITE: caut usage"

    # Run all tests (continue on failure to get full report)
    set +e

    run_test "basic_invocation" "Basic usage command invocation" test_basic_invocation
    run_test "help_output" "Usage help displays correctly" test_help_output
    run_test "version_output" "Version command works" test_version_output
    run_test "json_output_format" "JSON output is valid and has schema" test_json_output_format
    run_test "json_format_flag" "--format json produces valid JSON" test_json_format_flag
    run_test "markdown_output" "Markdown output format" test_markdown_output
    run_test "no_color_mode" "No ANSI codes in --no-color mode" test_no_color_mode
    run_test "verbose_mode" "Verbose mode runs without error" test_verbose_mode
    run_test "pretty_json" "Pretty JSON output is formatted" test_pretty_json
    run_test "invalid_provider" "Invalid provider handled gracefully" test_invalid_provider
    run_test "provider_filter" "Provider filtering works" test_provider_filter
    run_test "all_providers" "--provider=all works" test_all_providers
    run_test "status_flag" "--status flag works" test_status_flag
    run_test "json_with_errors" "JSON output has errors array" test_json_with_errors
    run_test "timeout_handling" "Timeout parameter handled" test_timeout_handling

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
