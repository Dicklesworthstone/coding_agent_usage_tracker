# Test Fixtures

This directory contains test fixtures for integration and unit tests. Each subdirectory corresponds to a domain or provider.

## Directory Structure

```
tests/fixtures/
├── claude/               # Claude (Anthropic) provider fixtures
│   ├── credentials_full.json      # Full credentials with email and UUID
│   ├── credentials_minimal.json   # Minimal credentials (email only)
│   ├── credentials_empty.json     # Empty/null credentials
│   ├── cli_output_v0.2.105.txt    # CLI output for v0.2.105
│   ├── cli_output_legacy.txt      # Legacy CLI output format
│   ├── rate_limit_full.json       # Full rate limit response (all tiers)
│   ├── rate_limit_partial.json    # Partial rate limit (primary only)
│   └── rate_limit_empty.json      # Empty rate limit response
│
├── codex/                # Codex (OpenAI) provider fixtures
│   ├── auth_oauth.json           # OAuth tokens with JWT
│   ├── auth_api_key.json         # API key only
│   ├── auth_both.json            # Both OAuth tokens and API key
│   ├── jwt_claims_sample.json    # Decoded JWT claims structure
│   ├── rate_limit_full.json      # Full rate limit with credits
│   ├── rate_limit_credits_only.json  # Credits without rate limits
│   └── rate_limit_user_only.json     # Identity only
│
├── status/               # Status page response fixtures
│   ├── statuspage_operational.json   # All systems operational
│   ├── statuspage_minor.json         # Minor disruption
│   ├── statuspage_major.json         # Major disruption
│   ├── statuspage_critical.json      # Critical failure
│   └── statuspage_maintenance.json   # Scheduled maintenance
│
├── cost/                 # Cost scanning fixtures
│   ├── claude_stats_cache.json   # ClaudeStatsCache format
│   ├── codex_event_log.json      # Codex event log format
│   ├── daily_breakdown.json      # Daily cost breakdown
│   └── monthly_totals.json       # Monthly totals by model
│
├── token_accounts/       # Token account storage fixtures
│   ├── single_account.json       # Single account configuration
│   ├── multi_account.json        # Multiple providers and accounts
│   ├── empty.json                # Empty configuration
│   └── edge_cases.json           # Edge cases (empty labels, invalid indices)
│
└── errors/               # Error response fixtures
    ├── network_timeout.json      # Network timeout error
    ├── http_401.json             # Unauthorized error
    ├── http_403.json             # Forbidden error
    ├── http_500.json             # Server error
    ├── malformed_json.txt        # Invalid JSON for parse error testing
    └── empty_response.json       # Empty JSON response
```

## Usage

### Loading Fixtures in Rust Tests

```rust
use common::fixtures::{load_fixture, load_fixture_text, load_fixture_json};

// Load and deserialize to a specific type
let creds: serde_json::Value = load_fixture("claude/credentials_full.json");

// Load as raw text
let cli_output = load_fixture_text("claude/cli_output_v0.2.105.txt");

// Load as serde_json::Value
let json = load_fixture_json("codex/auth_oauth.json");
```

### Using Factory Functions

Factory functions are available for creating test objects programmatically:

```rust
use common::fixtures::*;

// Create usage snapshots
let usage = usage_snapshot(30.0, Some(45.0));
let full_usage = usage_snapshot_full(30.0, 45.0, 60.0);

// Create provider payloads
let payload = provider_payload("codex", "cli", usage);
let default_payload = provider_payload_default("claude", "oauth");

// Create status payloads
let status = status_operational();
let outage = status_major();

// Create credits
let credits = credits_snapshot(75.0);

// Create cost payloads
let cost = cost_payload("claude", 2.50, 50.00, 100_000);
```

## Fixture Design Principles

1. **Realistic Data**: Fixtures use realistic values that match production API responses
2. **Edge Cases**: Include minimal, empty, and edge case variants for robustness testing
3. **Self-Documenting**: JSON files are formatted and include representative values
4. **Versioned**: CLI output fixtures include version numbers when format varies
5. **Sanitized**: No real credentials or PII - all tokens are clearly marked as test data

## Adding New Fixtures

1. Create the JSON file in the appropriate subdirectory
2. Use realistic field names and values
3. Add both "full" and "minimal/empty" variants
4. Update this README with the new fixture
5. Add a corresponding factory function in `tests/common/fixtures.rs` if needed
