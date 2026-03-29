#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TEMP_DIR=""
PROVIDER="${1:-codex}"
TIMEOUT_SECONDS="${CAUT_SMOKE_TIMEOUT:-1}"
INTERVAL_SECONDS="${CAUT_SMOKE_INTERVAL:-60}"

if [[ -t 1 ]]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    NC='\033[0m'
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    NC=''
fi

log_step() {
    echo -e "${BLUE}▶${NC} $1"
}

log_pass() {
    echo -e "  ${GREEN}✓${NC} $1"
}

log_fail() {
    echo -e "  ${RED}✗${NC} $1"
    exit 1
}

wait_for_usage_snapshot() {
    local attempts="${1:-20}"
    local delay_seconds="${2:-1}"
    local output=""

    for ((i = 1; i <= attempts; i++)); do
        output="$($CAUT_BIN_PATH daemon status --json 2>&1 || true)"
        if echo "$output" | grep -Eq '"schemaVersion"[[:space:]]*:[[:space:]]*"caut\.v1"' \
            && echo "$output" | grep -Eq '"command"[[:space:]]*:[[:space:]]*"usage"'; then
            printf '%s' "$output"
            return 0
        fi
        sleep "$delay_seconds"
    done

    printf '%s' "$output"
    return 1
}

find_caut_bin() {
    if [[ -n "${CAUT_BIN:-}" ]]; then
        echo "$CAUT_BIN"
        return 0
    fi

    local candidates=(
        "./target/release/caut.exe"
        "./target/release/caut"
        "./target/debug/caut.exe"
        "./target/debug/caut"
        "/tmp/cargo-target/debug/caut.exe"
        "/tmp/cargo-target/debug/caut"
    )

    for candidate in "${candidates[@]}"; do
        if [[ -x "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done

    echo ""
    return 1
}

cleanup() {
    if [[ -n "${CAUT_BIN_PATH:-}" ]]; then
        "$CAUT_BIN_PATH" daemon stop >/dev/null 2>&1 || true
    fi
    if [[ -n "$TEMP_DIR" && -d "$TEMP_DIR" ]]; then
        rm -rf "$TEMP_DIR"
    fi
}

trap cleanup EXIT

cd "$PROJECT_ROOT"

CAUT_BIN_PATH="$(find_caut_bin || true)"
if [[ -z "$CAUT_BIN_PATH" ]]; then
    log_step "Building caut debug binary"
    cargo build >/dev/null
    CAUT_BIN_PATH="$(find_caut_bin || true)"
fi

if [[ -z "$CAUT_BIN_PATH" ]]; then
    log_fail "Unable to locate caut binary"
fi

TEMP_DIR="$(mktemp -d)"
export XDG_DATA_HOME="$TEMP_DIR"
export XDG_CONFIG_HOME="$TEMP_DIR"
export XDG_CACHE_HOME="$TEMP_DIR"

METADATA_PATH="$TEMP_DIR/caut/resident-daemon.json"

log_step "Starting resident daemon for provider '$PROVIDER'"
"$CAUT_BIN_PATH" daemon start --provider "$PROVIDER" --timeout "$TIMEOUT_SECONDS" --interval "$INTERVAL_SECONDS"
[[ -f "$METADATA_PATH" ]] || log_fail "Daemon metadata file was not created"
log_pass "Resident daemon started"

log_step "Waiting for the first cached resident snapshot"
status_output="$(wait_for_usage_snapshot 20 1)" \
    || log_fail "Resident daemon did not publish a cached usage snapshot in time"
log_pass "Resident status returned caut JSON"

log_step "Scheduling asynchronous resident refresh"
refresh_output="$($CAUT_BIN_PATH daemon refresh 2>&1)"
echo "$refresh_output"
[[ "$refresh_output" == *"Resident refresh"* ]] || log_fail "Refresh command did not acknowledge scheduling"
post_refresh_status="$(wait_for_usage_snapshot 20 1)" \
    || log_fail "Resident status did not recover after refresh scheduling"
echo "$post_refresh_status" | grep -Eq '"command"[[:space:]]*:[[:space:]]*"usage"' \
    || log_fail "Post-refresh status did not return a usage payload"
log_pass "Resident refresh acknowledged"

log_step "Stopping resident daemon"
stop_output="$($CAUT_BIN_PATH daemon stop 2>&1)"
echo "$stop_output"
[[ "$stop_output" == *"Resident daemon stopped."* ]] || log_fail "Stop command did not acknowledge shutdown"
[[ ! -f "$METADATA_PATH" ]] || log_fail "Daemon metadata file was not removed on stop"
log_pass "Resident daemon stopped cleanly"

echo -e "\n${GREEN}Resident daemon smoke test passed.${NC}"
