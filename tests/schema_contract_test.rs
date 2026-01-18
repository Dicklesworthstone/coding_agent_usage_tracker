//! JSON Schema Contract Tests
//!
//! These tests verify that JSON output matches the documented schema contract.
//! This prevents breaking changes to machine-readable output that downstream
//! tools depend on.

use jsonschema::Validator;
use serde_json::{Value, json};

/// Load and compile the CAUT v1 schema.
fn load_schema() -> Validator {
    let schema_str = include_str!("../schemas/caut-v1.schema.json");
    let schema: Value = serde_json::from_str(schema_str).expect("Schema should be valid JSON");
    jsonschema::validator_for(&schema).expect("Schema should compile")
}

// =============================================================================
// Schema Version Tests
// =============================================================================

#[test]
fn test_schema_version_is_caut_v1() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(schema.is_valid(&valid), "Valid schema version should pass");
}

#[test]
fn test_schema_version_wrong_value_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v2",  // Wrong version
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Wrong schema version should fail"
    );
}

#[test]
fn test_schema_version_missing_fails() {
    let schema = load_schema();

    let invalid = json!({
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Missing schema version should fail"
    );
}

// =============================================================================
// Required Fields Tests
// =============================================================================

#[test]
fn test_all_required_fields_present() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        schema.is_valid(&valid),
        "All required fields present should pass"
    );
}

#[test]
fn test_missing_generated_at_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Missing generatedAt should fail"
    );
}

#[test]
fn test_missing_command_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "Missing command should fail");
}

#[test]
fn test_missing_data_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "Missing data should fail");
}

#[test]
fn test_missing_errors_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "Missing errors should fail");
}

#[test]
fn test_missing_meta_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": []
    });

    assert!(!schema.is_valid(&invalid), "Missing meta should fail");
}

// =============================================================================
// Meta Required Fields Tests
// =============================================================================

#[test]
fn test_meta_missing_format_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Missing meta.format should fail"
    );
}

#[test]
fn test_meta_missing_flags_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "Missing meta.flags should fail");
}

#[test]
fn test_meta_missing_runtime_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": []
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Missing meta.runtime should fail"
    );
}

// =============================================================================
// Type Validation Tests
// =============================================================================

#[test]
fn test_errors_must_be_array() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": "not an array",  // Should be array
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "errors as string should fail");
}

#[test]
fn test_data_must_be_array() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": {},  // Should be array
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(!schema.is_valid(&invalid), "data as object should fail");
}

#[test]
fn test_flags_must_be_array_of_strings() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [1, 2, 3],  // Should be strings
            "runtime": "cli"
        }
    });

    assert!(
        !schema.is_valid(&invalid),
        "flags as array of numbers should fail"
    );
}

// =============================================================================
// Command Enum Tests
// =============================================================================

#[test]
fn test_command_usage_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_command_cost_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "cost",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_command_invalid_value_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "invalid_command",  // Not in enum
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(!schema.is_valid(&invalid), "Invalid command should fail");
}

// =============================================================================
// Format Enum Tests
// =============================================================================

#[test]
fn test_format_json_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_format_markdown_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "markdown", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_format_invalid_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "yaml", "flags": [], "runtime": "cli" }  // Invalid format
    });

    assert!(!schema.is_valid(&invalid), "Invalid format should fail");
}

// =============================================================================
// Runtime Enum Tests
// =============================================================================

#[test]
fn test_runtime_cli_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_runtime_daemon_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "daemon" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_runtime_watch_valid() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "watch" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_runtime_invalid_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "server" }  // Invalid
    });

    assert!(!schema.is_valid(&invalid), "Invalid runtime should fail");
}

// =============================================================================
// Date/Time Format Tests
// =============================================================================

#[test]
fn test_generated_at_valid_iso8601() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_generated_at_with_offset() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00+05:30",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_generated_at_with_milliseconds() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00.123Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

// =============================================================================
// ProviderPayload Tests (Usage Command)
// =============================================================================

#[test]
fn test_provider_payload_minimal() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "usage": {
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_provider_payload_full() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "account": "test@example.com",
            "version": "1.0.0",
            "source": "oauth",
            "status": {
                "indicator": "none",
                "description": "All systems operational",
                "url": "https://status.anthropic.com"
            },
            "usage": {
                "primary": {
                    "usedPercent": 30.0,
                    "windowMinutes": 180,
                    "resetsAt": "2026-01-18T12:30:00Z",
                    "resetDescription": "in 2 hours"
                },
                "secondary": {
                    "usedPercent": 15.0,
                    "windowMinutes": 10080,
                    "resetDescription": "in 5 days"
                },
                "updatedAt": "2026-01-18T10:30:00Z",
                "identity": {
                    "accountEmail": "test@example.com",
                    "loginMethod": "oauth"
                }
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_provider_payload_with_credits() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "codex",
            "source": "openai-web",
            "usage": {
                "primary": { "usedPercent": 25.0 },
                "updatedAt": "2026-01-18T10:30:00Z"
            },
            "credits": {
                "remaining": 112.50,
                "events": [{
                    "amount": 100.0,
                    "eventType": "purchase",
                    "timestamp": "2026-01-15T00:00:00Z"
                }],
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_provider_payload_missing_required_fails() {
    let schema = load_schema();

    // Missing 'source'
    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "usage": {
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(!schema.is_valid(&invalid), "Missing source should fail");
}

// =============================================================================
// CostPayload Tests (Cost Command)
// =============================================================================

#[test]
fn test_cost_payload_minimal() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "cost",
        "data": [{
            "provider": "claude",
            "source": "local",
            "updatedAt": "2026-01-18T10:30:00Z",
            "daily": []
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_cost_payload_full() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "cost",
        "data": [{
            "provider": "claude",
            "source": "local",
            "updatedAt": "2026-01-18T10:30:00Z",
            "sessionTokens": 12345,
            "sessionCostUsd": 0.15,
            "last30DaysTokens": 500000,
            "last30DaysCostUsd": 5.50,
            "daily": [{
                "date": "2026-01-18",
                "inputTokens": 5000,
                "outputTokens": 2000,
                "totalTokens": 7000,
                "totalCost": 0.15
            }],
            "totals": {
                "inputTokens": 400000,
                "outputTokens": 100000,
                "totalTokens": 500000,
                "totalCost": 5.50
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

// =============================================================================
// RateWindow Validation Tests
// =============================================================================

#[test]
fn test_rate_window_used_percent_bounds() {
    let schema = load_schema();

    // Valid: 0%
    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "usage": {
                "primary": { "usedPercent": 0.0 },
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });
    assert!(schema.is_valid(&valid));

    // Valid: 100%
    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "usage": {
                "primary": { "usedPercent": 100.0 },
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });
    assert!(schema.is_valid(&valid));
}

#[test]
fn test_rate_window_negative_percent_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "usage": {
                "primary": { "usedPercent": -5.0 },  // Invalid
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Negative usedPercent should fail"
    );
}

#[test]
fn test_rate_window_over_100_percent_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "usage": {
                "primary": { "usedPercent": 150.0 },  // Invalid
                "updatedAt": "2026-01-18T10:30:00Z"
            }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(!schema.is_valid(&invalid), "usedPercent > 100 should fail");
}

// =============================================================================
// StatusIndicator Enum Tests
// =============================================================================

#[test]
fn test_status_indicator_all_values() {
    let schema = load_schema();
    let indicators = [
        "none",
        "minor",
        "major",
        "critical",
        "maintenance",
        "unknown",
    ];

    for indicator in indicators {
        let valid = json!({
            "schemaVersion": "caut.v1",
            "generatedAt": "2026-01-18T10:30:00Z",
            "command": "usage",
            "data": [{
                "provider": "claude",
                "source": "oauth",
                "status": {
                    "indicator": indicator,
                    "url": "https://status.example.com"
                },
                "usage": { "updatedAt": "2026-01-18T10:30:00Z" }
            }],
            "errors": [],
            "meta": { "format": "json", "flags": [], "runtime": "cli" }
        });

        assert!(
            schema.is_valid(&valid),
            "Status indicator '{}' should be valid",
            indicator
        );
    }
}

#[test]
fn test_status_indicator_invalid_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [{
            "provider": "claude",
            "source": "oauth",
            "status": {
                "indicator": "broken",  // Invalid
                "url": "https://status.example.com"
            },
            "usage": { "updatedAt": "2026-01-18T10:30:00Z" }
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Invalid status indicator should fail"
    );
}

// =============================================================================
// CostDailyEntry Date Format Tests
// =============================================================================

#[test]
fn test_daily_entry_date_format() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "cost",
        "data": [{
            "provider": "claude",
            "source": "local",
            "updatedAt": "2026-01-18T10:30:00Z",
            "daily": [{
                "date": "2026-01-18"
            }]
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

#[test]
fn test_daily_entry_invalid_date_format_fails() {
    let schema = load_schema();

    let invalid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "cost",
        "data": [{
            "provider": "claude",
            "source": "local",
            "updatedAt": "2026-01-18T10:30:00Z",
            "daily": [{
                "date": "01-18-2026"  // Invalid format
            }]
        }],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(
        !schema.is_valid(&invalid),
        "Invalid date format should fail"
    );
}

// =============================================================================
// Multiple Providers Tests
// =============================================================================

#[test]
fn test_multiple_providers_in_data() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [
            {
                "provider": "claude",
                "source": "oauth",
                "usage": { "updatedAt": "2026-01-18T10:30:00Z" }
            },
            {
                "provider": "codex",
                "source": "openai-web",
                "usage": { "updatedAt": "2026-01-18T10:30:00Z" }
            }
        ],
        "errors": [],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

// =============================================================================
// Errors Array Tests
// =============================================================================

#[test]
fn test_errors_with_messages() {
    let schema = load_schema();

    let valid = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [
            "Failed to fetch claude: timeout",
            "Failed to fetch codex: unauthorized"
        ],
        "meta": { "format": "json", "flags": [], "runtime": "cli" }
    });

    assert!(schema.is_valid(&valid));
}

// =============================================================================
// Backward Compatibility Tests
// =============================================================================

#[test]
fn test_minimal_envelope_compatible() {
    let schema = load_schema();

    // Minimal valid output - should always be compatible
    let minimal = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli"
        }
    });

    assert!(
        schema.is_valid(&minimal),
        "Minimal envelope should always be valid"
    );
}

#[test]
fn test_extra_fields_allowed() {
    let schema = load_schema();

    // JSON Schema by default allows additional properties
    // This tests that we don't break if new fields are added
    let with_extra = json!({
        "schemaVersion": "caut.v1",
        "generatedAt": "2026-01-18T10:30:00Z",
        "command": "usage",
        "data": [],
        "errors": [],
        "meta": {
            "format": "json",
            "flags": [],
            "runtime": "cli",
            "futureField": "some value"  // Extra field
        },
        "futureTopLevel": true  // Extra top-level field
    });

    assert!(
        schema.is_valid(&with_extra),
        "Extra fields should be allowed for forward compatibility"
    );
}

// =============================================================================
// camelCase Naming Convention Tests
// =============================================================================

#[test]
fn test_camel_case_naming() {
    // This test verifies that the schema uses camelCase
    let schema_str = include_str!("../schemas/caut-v1.schema.json");

    // Verify key fields are camelCase
    assert!(schema_str.contains("\"schemaVersion\""));
    assert!(schema_str.contains("\"generatedAt\""));
    assert!(schema_str.contains("\"usedPercent\""));
    assert!(schema_str.contains("\"windowMinutes\""));
    assert!(schema_str.contains("\"resetsAt\""));
    assert!(schema_str.contains("\"resetDescription\""));
    assert!(schema_str.contains("\"updatedAt\""));
    assert!(schema_str.contains("\"sessionTokens\""));
    assert!(schema_str.contains("\"sessionCostUsd\""));
    assert!(schema_str.contains("\"last30DaysTokens\""));
    assert!(schema_str.contains("\"last30DaysCostUsd\""));
    assert!(schema_str.contains("\"totalTokens\""));
    assert!(schema_str.contains("\"totalCost\""));
    assert!(schema_str.contains("\"inputTokens\""));
    assert!(schema_str.contains("\"outputTokens\""));

    // Verify no snake_case in field names (except for meta keywords like $schema, $id, $defs)
    let fields_to_check = [
        "schema_version",
        "generated_at",
        "used_percent",
        "window_minutes",
        "resets_at",
        "reset_description",
        "updated_at",
        "session_tokens",
        "session_cost_usd",
        "last_30_days_tokens",
        "last_30_days_cost_usd",
        "total_tokens",
        "total_cost",
        "input_tokens",
        "output_tokens",
    ];

    for field in fields_to_check {
        assert!(
            !schema_str.contains(&format!("\"{}\"", field)),
            "Found snake_case field: {}",
            field
        );
    }
}
