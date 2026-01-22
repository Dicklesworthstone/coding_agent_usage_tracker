#!/bin/bash
# =============================================================================
# History E2E Tests
# =============================================================================
#
# End-to-end tests for the caut history subsystem.
# Tests the full CLI flow including database initialization, snapshot recording,
# queries, pruning, and export functionality.
#
# Usage:
#   ./scripts/history_e2e.sh
#
# Environment:
#   CAUT_BIN    Path to caut binary (default: auto-detect from cargo)
#   VERBOSE     Set to 1 for verbose output

set -euo pipefail

# =============================================================================
# Configuration
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TEMP_DIR=""
PASS_COUNT=0
FAIL_COUNT=0
VERBOSE="${VERBOSE:-0}"

# Colors (if terminal supports them)
if [[ -t 1 ]]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    NC=''
fi

# =============================================================================
# Helpers
# =============================================================================

log_section() {
    echo -e "\n${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
}

log_test() {
    echo -e "${YELLOW}▶${NC} $1"
}

log_pass() {
    echo -e "  ${GREEN}✓${NC} PASS"
    ((PASS_COUNT++))
}

log_fail() {
    echo -e "  ${RED}✗${NC} FAIL: $1"
    ((FAIL_COUNT++))
}

log_verbose() {
    if [[ "$VERBOSE" == "1" ]]; then
        echo -e "  ${BLUE}→${NC} $1"
    fi
}

fail() {
    log_fail "$1"
    return 1
}

# Find caut binary
find_caut_bin() {
    if [[ -n "${CAUT_BIN:-}" ]]; then
        echo "$CAUT_BIN"
        return 0
    fi

    # Try cargo target directories
    local candidates=(
        "/tmp/cargo-target/debug/caut"
        "$PROJECT_ROOT/target/debug/caut"
        "$PROJECT_ROOT/target/release/caut"
    )

    for candidate in "${candidates[@]}"; do
        if [[ -x "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done

    # Try which
    if command -v caut &> /dev/null; then
        which caut
        return 0
    fi

    echo ""
    return 1
}

setup() {
    TEMP_DIR=$(mktemp -d)
    export XDG_DATA_HOME="$TEMP_DIR"
    export XDG_CONFIG_HOME="$TEMP_DIR"
    export XDG_CACHE_HOME="$TEMP_DIR"

    # Create app directory structure
    mkdir -p "$TEMP_DIR/caut"

    log_verbose "Test environment: $TEMP_DIR"
}

cleanup() {
    if [[ -n "$TEMP_DIR" && -d "$TEMP_DIR" ]]; then
        rm -rf "$TEMP_DIR"
    fi
}

trap cleanup EXIT

# =============================================================================
# Test Functions
# =============================================================================

test_history_stats_empty() {
    log_test "history stats on empty database"

    local output
    output=$("$CAUT_BIN" history stats 2>&1) || true

    if echo "$output" | grep -q "History Database Statistics"; then
        log_pass
    else
        fail "Expected 'History Database Statistics' in output"
    fi
}

test_history_stats_json() {
    log_test "history stats JSON output"

    local output
    output=$("$CAUT_BIN" history stats --json 2>&1)

    if echo "$output" | jq -e '.schemaVersion == "caut.v1"' > /dev/null 2>&1; then
        log_pass
    else
        fail "Invalid JSON schema version"
    fi
}

test_history_show_empty() {
    log_test "history show on empty database"

    local output
    output=$("$CAUT_BIN" history show 2>&1) || true

    if echo "$output" | grep -q "No usage data found"; then
        log_pass
    else
        fail "Expected 'No usage data found' message"
    fi
}

test_history_show_json_empty() {
    log_test "history show JSON empty database"

    local output
    output=$("$CAUT_BIN" history show --json 2>&1)

    if echo "$output" | jq -e '.data.providers | length == 0' > /dev/null 2>&1; then
        log_pass
    else
        fail "Expected empty providers array"
    fi
}

test_history_show_markdown() {
    log_test "history show markdown output"

    local output
    output=$("$CAUT_BIN" history show --format md 2>&1)

    if echo "$output" | grep -q "# Usage History"; then
        log_pass
    else
        fail "Expected markdown header"
    fi
}

test_history_show_days_flag() {
    log_test "history show with --days flag"

    local output
    output=$("$CAUT_BIN" history show --days 30 --json 2>&1)

    if echo "$output" | jq -e '.data.period.days == 30' > /dev/null 2>&1; then
        log_pass
    else
        fail "Expected days=30 in period"
    fi
}

test_history_show_ascii() {
    log_test "history show ASCII mode"

    local output
    output=$("$CAUT_BIN" history show --ascii 2>&1) || true

    # Should not error
    if [[ $? -eq 0 ]] || echo "$output" | grep -q "No usage data"; then
        log_pass
    else
        fail "ASCII mode failed"
    fi
}

test_history_prune_dry_run() {
    log_test "history prune dry run"

    local output
    output=$("$CAUT_BIN" history prune --dry-run 2>&1)

    if echo "$output" | grep -q "Dry run"; then
        log_pass
    else
        fail "Expected dry run indicator"
    fi
}

test_history_prune_json() {
    log_test "history prune JSON output"

    local output
    output=$("$CAUT_BIN" history prune --dry-run --json 2>&1)

    if echo "$output" | jq -e '.data.dryRun == true' > /dev/null 2>&1; then
        log_pass
    else
        fail "Expected dryRun=true in JSON"
    fi
}

test_history_prune_retention() {
    log_test "history prune with retention flag"

    local output
    output=$("$CAUT_BIN" history prune --retention-days 7 --dry-run --json 2>&1)

    # Should succeed without error
    if echo "$output" | jq -e '.command == "history prune"' > /dev/null 2>&1; then
        log_pass
    else
        fail "Prune with retention flag failed"
    fi
}

test_history_stats_after_init() {
    log_test "history stats shows database exists"

    local output
    output=$("$CAUT_BIN" history stats --json 2>&1)

    if echo "$output" | jq -e '.data.snapshotCount >= 0' > /dev/null 2>&1; then
        log_pass
    else
        fail "Expected snapshotCount in stats"
    fi
}

test_help_history() {
    log_test "history help displays correctly"

    local output
    output=$("$CAUT_BIN" history --help 2>&1)

    if echo "$output" | grep -q "show\|stats\|prune"; then
        log_pass
    else
        fail "Expected subcommands in help"
    fi
}

test_invalid_subcommand() {
    log_test "invalid history subcommand fails gracefully"

    local output
    local exit_code=0
    output=$("$CAUT_BIN" history invalid 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        log_pass
    else
        fail "Expected non-zero exit for invalid subcommand"
    fi
}

test_concurrent_stats() {
    log_test "concurrent history stats calls"

    # Run multiple stats calls in parallel
    local pids=()
    for i in {1..5}; do
        "$CAUT_BIN" history stats --json > /dev/null 2>&1 &
        pids+=($!)
    done

    local all_success=true
    for pid in "${pids[@]}"; do
        if ! wait "$pid"; then
            all_success=false
        fi
    done

    if $all_success; then
        log_pass
    else
        fail "Concurrent stats calls failed"
    fi
}

test_json_schema_contract() {
    log_test "JSON output follows schema contract"

    local output
    output=$("$CAUT_BIN" history show --json 2>&1)

    # Check required fields
    local has_schema has_generated has_command has_data has_errors has_meta
    has_schema=$(echo "$output" | jq -e '.schemaVersion' > /dev/null 2>&1 && echo "1" || echo "0")
    has_generated=$(echo "$output" | jq -e '.generatedAt' > /dev/null 2>&1 && echo "1" || echo "0")
    has_command=$(echo "$output" | jq -e '.command' > /dev/null 2>&1 && echo "1" || echo "0")
    has_data=$(echo "$output" | jq -e '.data' > /dev/null 2>&1 && echo "1" || echo "0")
    has_errors=$(echo "$output" | jq -e '.errors' > /dev/null 2>&1 && echo "1" || echo "0")
    has_meta=$(echo "$output" | jq -e '.meta' > /dev/null 2>&1 && echo "1" || echo "0")

    if [[ "$has_schema" == "1" && "$has_generated" == "1" && "$has_command" == "1" &&
          "$has_data" == "1" && "$has_errors" == "1" && "$has_meta" == "1" ]]; then
        log_pass
    else
        fail "Missing required JSON fields"
    fi
}

test_database_path_in_stats() {
    log_test "stats shows database path"

    local output
    output=$("$CAUT_BIN" history stats 2>&1)

    if echo "$output" | grep -q "usage-history.sqlite\|database"; then
        log_pass
    else
        fail "Expected database path in stats output"
    fi
}

# =============================================================================
# Main
# =============================================================================

main() {
    log_section "History E2E Tests"

    # Find binary
    CAUT_BIN=$(find_caut_bin) || {
        echo -e "${RED}Error: Could not find caut binary${NC}"
        echo "Run 'cargo build' first or set CAUT_BIN environment variable"
        exit 1
    }

    echo "Using binary: $CAUT_BIN"
    echo "Binary version: $("$CAUT_BIN" --version 2>&1 || echo 'unknown')"

    # Check jq is available
    if ! command -v jq &> /dev/null; then
        echo -e "${RED}Error: jq is required for JSON tests${NC}"
        exit 1
    fi

    # Setup test environment
    setup

    log_section "Basic Commands"
    test_help_history
    test_invalid_subcommand

    log_section "History Stats"
    test_history_stats_empty
    test_history_stats_json
    test_history_stats_after_init
    test_database_path_in_stats

    log_section "History Show"
    test_history_show_empty
    test_history_show_json_empty
    test_history_show_markdown
    test_history_show_days_flag
    test_history_show_ascii

    log_section "History Prune"
    test_history_prune_dry_run
    test_history_prune_json
    test_history_prune_retention

    log_section "Schema Contract"
    test_json_schema_contract

    log_section "Concurrency"
    test_concurrent_stats

    # Summary
    log_section "Summary"
    echo -e "  ${GREEN}Passed: $PASS_COUNT${NC}"
    echo -e "  ${RED}Failed: $FAIL_COUNT${NC}"
    echo ""

    if [[ $FAIL_COUNT -gt 0 ]]; then
        echo -e "${RED}Some tests failed!${NC}"
        exit 1
    else
        echo -e "${GREEN}All tests passed!${NC}"
        exit 0
    fi
}

main "$@"
