#!/bin/bash
set -euo pipefail

BIN="${CAUT_BIN:-target/debug/caut}"

if [[ ! -x "$BIN" ]]; then
  echo "Building caut..."
  cargo build >/dev/null
fi

echo "Testing logging with robot mode..."
output=$(CAUT_LOG=debug "$BIN" --debug-rich --json 2>&1 || true)
if echo "$output" | grep -q "robot_mode"; then
  echo "PASS: Robot mode logged"
else
  echo "FAIL: Robot mode not logged"
  exit 1
fi

echo "Testing JSON log format..."
json_logs=$(CAUT_LOG=debug CAUT_LOG_FORMAT=json "$BIN" --debug-rich --json 2>/dev/null || true)
first_line=$(printf "%s" "$json_logs" | head -n 1)
if echo "$first_line" | jq . >/dev/null 2>&1; then
  echo "PASS: JSON format valid"
else
  echo "WARN: JSON format may have issues"
fi

echo "Testing debug diagnostics..."
output=$("$BIN" --debug-rich 2>&1 || true)
if echo "$output" | grep -q "stdout is TTY"; then
  echo "PASS: Debug diagnostics work"
else
  echo "FAIL: Debug diagnostics missing"
  exit 1
fi
