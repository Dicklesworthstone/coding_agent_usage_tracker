//! Test utilities for caut.
//!
//! Provides shared helpers, test data factories, and assertion macros
//! for use across all test modules.
//!
//! # Usage
//!
//! ```rust,ignore
//! use caut::test_utils::*;
//!
//! let window = make_test_rate_window(30.0);
//! let snapshot = make_test_usage_snapshot();
//! let dir = TestDir::new();
//! dir.create_file("config.toml", "[general]\ntimeout_seconds = 30");
//! ```

use chrono::{TimeDelta, Utc};
use std::fs;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

use crate::core::models::{
    CostDailyEntry, CostPayload, CostTotals, CreditEvent, CreditsSnapshot, ProviderIdentity,
    ProviderPayload, RateWindow, StatusIndicator, StatusPayload, UsageSnapshot,
};

// =============================================================================
// Test Data Factories
// =============================================================================

/// Create a test `RateWindow` with the given usage percentage.
///
/// The window is configured with realistic values:
/// - 180-minute window duration
/// - Resets in ~2 hours
/// - Human-readable reset description
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_rate_window;
///
/// let window = make_test_rate_window(30.0);
/// assert_eq!(window.used_percent, 30.0);
/// assert_eq!(window.remaining_percent(), 70.0);
/// ```
#[must_use]
pub fn make_test_rate_window(used_percent: f64) -> RateWindow {
    RateWindow {
        used_percent,
        window_minutes: Some(180),
        resets_at: Some(Utc::now() + TimeDelta::hours(2)),
        reset_description: Some("resets in 2h".to_string()),
    }
}

/// Create a test `RateWindow` with minimal fields (only usage percentage).
///
/// Useful for testing rendering code that must handle missing optional fields.
#[must_use]
pub fn make_test_rate_window_minimal(used_percent: f64) -> RateWindow {
    RateWindow::new(used_percent)
}

/// Create a test `UsageSnapshot` with primary and secondary windows.
///
/// Includes realistic identity information and timestamps.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_usage_snapshot;
///
/// let snapshot = make_test_usage_snapshot();
/// assert!(snapshot.primary.is_some());
/// assert!(snapshot.secondary.is_some());
/// ```
#[must_use]
pub fn make_test_usage_snapshot() -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(make_test_rate_window(28.0)),
        secondary: Some(make_test_rate_window(45.0)),
        tertiary: None,
        updated_at: Utc::now(),
        identity: Some(ProviderIdentity {
            account_email: Some("test@example.com".to_string()),
            account_organization: Some("Test Org".to_string()),
            login_method: Some("oauth".to_string()),
        }),
    }
}

/// Create a test `UsageSnapshot` with all three tiers (for Claude-like providers).
#[must_use]
pub fn make_test_usage_snapshot_with_tertiary() -> UsageSnapshot {
    UsageSnapshot {
        primary: Some(make_test_rate_window(28.0)),
        secondary: Some(make_test_rate_window(45.0)),
        tertiary: Some(make_test_rate_window(55.0)),
        updated_at: Utc::now(),
        identity: Some(ProviderIdentity {
            account_email: Some("test@example.com".to_string()),
            account_organization: None,
            login_method: Some("oauth".to_string()),
        }),
    }
}

/// Create a minimal test `UsageSnapshot` with only primary window.
///
/// Useful for testing code that handles minimal data.
#[must_use]
pub fn make_test_usage_snapshot_minimal() -> UsageSnapshot {
    UsageSnapshot::new(make_test_rate_window_minimal(50.0))
}

/// Create a test `CreditsSnapshot` with the given remaining balance.
///
/// Includes sample credit events for history.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_credits_snapshot;
///
/// let credits = make_test_credits_snapshot(112.50);
/// assert_eq!(credits.remaining, 112.50);
/// assert!(!credits.events.is_empty());
/// ```
#[must_use]
pub fn make_test_credits_snapshot(remaining: f64) -> CreditsSnapshot {
    CreditsSnapshot {
        remaining,
        events: vec![
            CreditEvent {
                amount: 100.0,
                event_type: "purchase".to_string(),
                timestamp: Utc::now() - TimeDelta::days(30),
                description: Some("Monthly credit purchase".to_string()),
            },
            CreditEvent {
                amount: -12.50,
                event_type: "usage".to_string(),
                timestamp: Utc::now() - TimeDelta::days(1),
                description: Some("API usage".to_string()),
            },
        ],
        updated_at: Utc::now(),
    }
}

/// Create a test `CreditsSnapshot` with no events.
#[must_use]
pub fn make_test_credits_snapshot_minimal(remaining: f64) -> CreditsSnapshot {
    CreditsSnapshot {
        remaining,
        events: Vec::new(),
        updated_at: Utc::now(),
    }
}

/// Create a test `StatusPayload` with the given indicator.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_status_payload;
/// use caut::core::models::StatusIndicator;
///
/// let status = make_test_status_payload(StatusIndicator::None);
/// assert_eq!(status.indicator, StatusIndicator::None);
/// ```
#[must_use]
pub fn make_test_status_payload(indicator: StatusIndicator) -> StatusPayload {
    StatusPayload {
        indicator,
        description: Some(indicator.label().to_string()),
        updated_at: Some(Utc::now()),
        url: "https://status.example.com".to_string(),
    }
}

/// Create a test `StatusPayload` for operational status.
#[must_use]
pub fn make_test_status_operational() -> StatusPayload {
    make_test_status_payload(StatusIndicator::None)
}

/// Create a test `StatusPayload` for a major outage.
#[must_use]
pub fn make_test_status_major_outage() -> StatusPayload {
    StatusPayload {
        indicator: StatusIndicator::Major,
        description: Some("Major service disruption affecting API availability".to_string()),
        updated_at: Some(Utc::now()),
        url: "https://status.example.com".to_string(),
    }
}

/// Create a test `ProviderPayload` for the given provider and source.
///
/// Includes realistic usage data, identity, and optional credits (for Codex).
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_provider_payload;
///
/// let payload = make_test_provider_payload("codex", "cli");
/// assert_eq!(payload.provider, "codex");
/// assert_eq!(payload.source, "cli");
/// assert!(payload.credits.is_some());
/// ```
#[must_use]
pub fn make_test_provider_payload(provider: &str, source: &str) -> ProviderPayload {
    let has_credits = provider.to_lowercase() == "codex";

    ProviderPayload {
        provider: provider.to_string(),
        account: Some("test@example.com".to_string()),
        version: Some("0.1.0".to_string()),
        source: source.to_string(),
        status: Some(make_test_status_operational()),
        usage: make_test_usage_snapshot(),
        credits: if has_credits {
            Some(make_test_credits_snapshot(112.50))
        } else {
            None
        },
        antigravity_plan_info: None,
        openai_dashboard: None,
        auth_warning: None,
    }
}

/// Create a test `ProviderPayload` with minimal fields.
///
/// No account, version, status, or credits - just provider, source, and usage.
#[must_use]
pub fn make_test_provider_payload_minimal(provider: &str, source: &str) -> ProviderPayload {
    ProviderPayload {
        provider: provider.to_string(),
        account: None,
        version: None,
        source: source.to_string(),
        status: None,
        usage: make_test_usage_snapshot_minimal(),
        credits: None,
        antigravity_plan_info: None,
        openai_dashboard: None,
        auth_warning: None,
    }
}

/// Create a test `CostPayload` for the given provider.
///
/// Includes session and 30-day totals with daily breakdown.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::make_test_cost_payload;
///
/// let cost = make_test_cost_payload("claude");
/// assert_eq!(cost.provider, "claude");
/// assert!(cost.session_cost_usd.is_some());
/// ```
#[must_use]
pub fn make_test_cost_payload(provider: &str) -> CostPayload {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let yesterday = (Utc::now() - TimeDelta::days(1))
        .format("%Y-%m-%d")
        .to_string();

    CostPayload {
        provider: provider.to_string(),
        source: "local".to_string(),
        updated_at: Utc::now(),
        session_tokens: Some(124_500),
        session_cost_usd: Some(2.45),
        last_30_days_tokens: Some(2_400_000),
        last_30_days_cost_usd: Some(47.82),
        daily: vec![
            CostDailyEntry {
                date: today,
                input_tokens: Some(100_000),
                output_tokens: Some(24_500),
                cache_read_tokens: Some(5_000),
                cache_creation_tokens: Some(1_000),
                total_tokens: Some(124_500),
                total_cost: Some(2.45),
                models_used: Some(vec![
                    "claude-3-opus".to_string(),
                    "claude-3-sonnet".to_string(),
                ]),
            },
            CostDailyEntry {
                date: yesterday,
                input_tokens: Some(80_000),
                output_tokens: Some(20_000),
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(100_000),
                total_cost: Some(1.95),
                models_used: Some(vec!["claude-3-sonnet".to_string()]),
            },
        ],
        totals: Some(CostTotals {
            input_tokens: Some(1_800_000),
            output_tokens: Some(600_000),
            cache_read_tokens: Some(50_000),
            cache_creation_tokens: Some(10_000),
            total_tokens: Some(2_400_000),
            total_cost: Some(47.82),
        }),
    }
}

/// Create a test `CostPayload` with minimal fields.
#[must_use]
pub fn make_test_cost_payload_minimal(provider: &str) -> CostPayload {
    CostPayload {
        provider: provider.to_string(),
        source: "local".to_string(),
        updated_at: Utc::now(),
        session_tokens: None,
        session_cost_usd: None,
        last_30_days_tokens: None,
        last_30_days_cost_usd: None,
        daily: Vec::new(),
        totals: None,
    }
}

/// Create a test `ProviderIdentity` with realistic data.
#[must_use]
pub fn make_test_provider_identity() -> ProviderIdentity {
    ProviderIdentity {
        account_email: Some("test@example.com".to_string()),
        account_organization: Some("Test Organization".to_string()),
        login_method: Some("google".to_string()),
    }
}

// =============================================================================
// Temp Directory Utilities
// =============================================================================

/// A temporary directory for tests with automatic cleanup.
///
/// Creates an isolated directory that is automatically deleted when
/// the `TestDir` is dropped. Uses the `tempfile` crate internally.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::test_utils::TestDir;
///
/// let dir = TestDir::new();
/// dir.create_file("config.toml", "[general]\ntimeout = 30");
///
/// let config_path = dir.path().join("config.toml");
/// assert!(config_path.exists());
///
/// // Directory is automatically cleaned up when `dir` goes out of scope
/// ```
pub struct TestDir {
    inner: tempfile::TempDir,
}

impl TestDir {
    /// Create a new isolated temporary directory.
    ///
    /// # Panics
    ///
    /// Panics if the temporary directory cannot be created.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: tempfile::tempdir().expect("Failed to create temp directory"),
        }
    }

    /// Get the path to the temporary directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.inner.path()
    }

    /// Create a file in the temporary directory with the given content.
    ///
    /// Creates parent directories as needed.
    ///
    /// # Panics
    ///
    /// Panics if the file cannot be created or written.
    pub fn create_file(&self, name: &str, content: &str) {
        let path = self.inner.path().join(name);

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent directories");
        }

        let mut file = fs::File::create(&path).expect("Failed to create test file");
        file.write_all(content.as_bytes())
            .expect("Failed to write test file");
    }

    /// Create an empty file in the temporary directory.
    ///
    /// # Panics
    ///
    /// Panics if the file cannot be created.
    pub fn create_empty_file(&self, name: &str) {
        self.create_file(name, "");
    }

    /// Create a subdirectory in the temporary directory.
    ///
    /// # Panics
    ///
    /// Panics if the directory cannot be created.
    pub fn create_dir(&self, name: &str) {
        let path = self.inner.path().join(name);
        fs::create_dir_all(&path).expect("Failed to create test directory");
    }

    /// Read a file from the temporary directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    pub fn read_file(&self, name: &str) -> io::Result<String> {
        let path = self.inner.path().join(name);
        fs::read_to_string(path)
    }

    /// Check if a file exists in the temporary directory.
    #[must_use]
    pub fn file_exists(&self, name: &str) -> bool {
        self.inner.path().join(name).exists()
    }

    /// Get the full path to a file in the temporary directory.
    #[must_use]
    pub fn file_path(&self, name: &str) -> PathBuf {
        self.inner.path().join(name)
    }

    /// Keep the temporary directory (don't delete on drop).
    ///
    /// Useful for debugging test failures.
    ///
    /// # Returns
    ///
    /// The path to the persisted directory.
    #[must_use]
    pub fn persist(self) -> PathBuf {
        let path = self.inner.path().to_path_buf();
        // Consume self without running Drop by keeping the directory
        let _ = self.inner.keep();
        path
    }
}

impl Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Assertion Macros
// =============================================================================

/// Assert that a string contains a substring.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_contains;
///
/// let text = "Hello, world!";
/// assert_contains!(text, "world");
/// ```
#[macro_export]
macro_rules! assert_contains {
    ($haystack:expr, $needle:expr) => {
        let haystack = $haystack;
        let needle = $needle;
        assert!(
            haystack.contains(needle),
            "Expected string to contain {:?}\n\nActual string:\n{:?}",
            needle,
            haystack
        );
    };
    ($haystack:expr, $needle:expr, $($arg:tt)*) => {
        let haystack = $haystack;
        let needle = $needle;
        assert!(
            haystack.contains(needle),
            $($arg)*
        );
    };
}

/// Assert that a string does NOT contain a substring.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_not_contains;
///
/// let text = "Hello, world!";
/// assert_not_contains!(text, "goodbye");
/// ```
#[macro_export]
macro_rules! assert_not_contains {
    ($haystack:expr, $needle:expr) => {
        let haystack = $haystack;
        let needle = $needle;
        assert!(
            !haystack.contains(needle),
            "Expected string NOT to contain {:?}\n\nActual string:\n{:?}",
            needle,
            haystack
        );
    };
    ($haystack:expr, $needle:expr, $($arg:tt)*) => {
        let haystack = $haystack;
        let needle = $needle;
        assert!(
            !haystack.contains(needle),
            $($arg)*
        );
    };
}

/// Assert that a string is valid JSON.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_json_valid;
///
/// let json = r#"{"key": "value"}"#;
/// assert_json_valid!(json);
/// ```
#[macro_export]
macro_rules! assert_json_valid {
    ($json:expr) => {
        let json = $json;
        match serde_json::from_str::<serde_json::Value>(json) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Expected valid JSON, but parsing failed: {}\n\nJSON string:\n{}",
                    e, json
                );
            }
        }
    };
}

/// Assert that a string is valid JSON and matches the expected structure.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_json_eq;
///
/// let json = r#"{"key": "value"}"#;
/// assert_json_eq!(json, serde_json::json!({"key": "value"}));
/// ```
#[macro_export]
macro_rules! assert_json_eq {
    ($json:expr, $expected:expr) => {
        let json = $json;
        let parsed: serde_json::Value = serde_json::from_str(json).expect("Invalid JSON");
        let expected: serde_json::Value = $expected;
        assert_eq!(
            parsed,
            expected,
            "JSON mismatch\n\nExpected:\n{}\n\nActual:\n{}",
            serde_json::to_string_pretty(&expected).unwrap(),
            serde_json::to_string_pretty(&parsed).unwrap()
        );
    };
}

/// Assert that a string contains ANSI escape codes (has colors/formatting).
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_ansi_codes;
///
/// let colored = "\x1b[31mred text\x1b[0m";
/// assert_ansi_codes!(colored);
/// ```
#[macro_export]
macro_rules! assert_ansi_codes {
    ($text:expr) => {
        let text = $text;
        assert!(
            text.contains('\x1b') || text.contains('\u{001b}'),
            "Expected string to contain ANSI escape codes, but none found.\n\nActual string:\n{:?}",
            text
        );
    };
}

/// Assert that a string does NOT contain ANSI escape codes.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_no_ansi_codes;
///
/// let plain = "plain text";
/// assert_no_ansi_codes!(plain);
/// ```
#[macro_export]
macro_rules! assert_no_ansi_codes {
    ($text:expr) => {
        let text = $text;
        assert!(
            !text.contains('\x1b') && !text.contains('\u{001b}'),
            "Expected string to NOT contain ANSI escape codes.\n\nActual string:\n{:?}",
            text
        );
    };
}

/// Assert approximate floating point equality.
///
/// # Examples
///
/// ```rust,ignore
/// use caut::assert_float_eq;
///
/// assert_float_eq!(70.0, 70.0000001);
/// assert_float_eq!(70.0, 70.05, 0.1); // Custom epsilon
/// ```
#[macro_export]
macro_rules! assert_float_eq {
    ($left:expr, $right:expr) => {
        let left: f64 = $left;
        let right: f64 = $right;
        let epsilon: f64 = f64::EPSILON * 100.0;
        assert!(
            (left - right).abs() < epsilon,
            "Float equality assertion failed: {} != {} (epsilon: {})",
            left,
            right,
            epsilon
        );
    };
    ($left:expr, $right:expr, $epsilon:expr) => {
        let left: f64 = $left;
        let right: f64 = $right;
        let epsilon: f64 = $epsilon;
        assert!(
            (left - right).abs() < epsilon,
            "Float equality assertion failed: {} != {} (epsilon: {})",
            left,
            right,
            epsilon
        );
    };
}

// =============================================================================
// Test Helpers
// =============================================================================

/// Check if a string contains valid ANSI escape sequences.
#[must_use]
pub fn has_ansi_codes(text: &str) -> bool {
    text.contains('\x1b') || text.contains('\u{001b}')
}

/// Strip ANSI escape codes from a string.
///
/// Useful for comparing output content without formatting.
#[must_use]
pub fn strip_ansi_codes(text: &str) -> String {
    // Simple regex-free ANSI stripping
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (the terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Create sample JSONL content for testing cost scanning.
///
/// Returns JSONL lines representing completion events.
#[must_use]
pub fn make_test_jsonl_events() -> String {
    let now = Utc::now();
    let today = now.format("%Y-%m-%dT%H:%M:%S%.3fZ");

    format!(
        r#"{{"type":"completion","timestamp":"{today}","model":"claude-3-opus","input_tokens":1000,"output_tokens":500}}
{{"type":"completion","timestamp":"{today}","model":"claude-3-sonnet","input_tokens":2000,"output_tokens":1000}}
{{"type":"completion","timestamp":"{today}","model":"claude-3-opus","input_tokens":500,"output_tokens":250}}"#
    )
}

/// Create sample config TOML content for testing.
#[must_use]
pub fn make_test_config_toml() -> String {
    r#"[general]
timeout_seconds = 30
include_status = false
log_level = "info"

[providers]
default_providers = ["codex", "claude"]

[providers.claude]
enabled = true

[providers.codex]
enabled = true

[output]
format = "human"
color = true
pretty = false
"#
    .to_string()
}

/// Create sample token accounts JSON content for testing.
#[must_use]
pub fn make_test_token_accounts_json() -> String {
    r#"{
  "version": 1,
  "providers": {
    "claude": {
      "version": 1,
      "accounts": [
        {
          "id": "550e8400-e29b-41d4-a716-446655440000",
          "label": "personal",
          "token": "sk-ant-test-token",
          "addedAt": "2026-01-01T00:00:00Z",
          "lastUsed": null
        }
      ],
      "activeIndex": 0
    }
  }
}"#
    .to_string()
}

// =============================================================================
// Tests for Test Utilities
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_window_factory_creates_valid_window() {
        let window = make_test_rate_window(30.0);
        assert_float_eq!(window.used_percent, 30.0);
        assert_float_eq!(window.remaining_percent(), 70.0);
        assert!(window.window_minutes.is_some());
        assert!(window.resets_at.is_some());
        assert!(window.reset_description.is_some());
    }

    #[test]
    fn usage_snapshot_factory_creates_complete_snapshot() {
        let snapshot = make_test_usage_snapshot();
        assert!(snapshot.primary.is_some());
        assert!(snapshot.secondary.is_some());
        assert!(snapshot.identity.is_some());

        let identity = snapshot.identity.unwrap();
        assert!(identity.account_email.is_some());
    }

    #[test]
    fn credits_snapshot_factory_creates_with_events() {
        let credits = make_test_credits_snapshot(112.50);
        assert_float_eq!(credits.remaining, 112.50);
        assert!(!credits.events.is_empty());
    }

    #[test]
    fn provider_payload_factory_adds_credits_for_codex() {
        let codex = make_test_provider_payload("codex", "cli");
        assert!(codex.credits.is_some());

        let claude = make_test_provider_payload("claude", "oauth");
        assert!(claude.credits.is_none());
    }

    #[test]
    fn test_dir_creates_and_cleans_up() {
        let path: PathBuf;
        {
            let dir = TestDir::new();
            path = dir.path().to_path_buf();
            assert!(path.exists());
            dir.create_file("test.txt", "hello");
            assert!(path.join("test.txt").exists());
        }
        // Directory should be cleaned up after drop
        assert!(!path.exists());
    }

    #[test]
    fn test_dir_creates_nested_files() {
        let dir = TestDir::new();
        dir.create_file("subdir/nested/file.txt", "nested content");
        assert!(dir.file_exists("subdir/nested/file.txt"));
        assert_eq!(
            dir.read_file("subdir/nested/file.txt").unwrap(),
            "nested content"
        );
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        let colored = "\x1b[31mred\x1b[0m text";
        let stripped = strip_ansi_codes(colored);
        assert_eq!(stripped, "red text");
    }

    #[test]
    fn has_ansi_detects_escape_sequences() {
        assert!(has_ansi_codes("\x1b[31mred\x1b[0m"));
        assert!(!has_ansi_codes("plain text"));
    }

    #[test]
    fn assert_contains_macro_works() {
        let text = "Hello, world!";
        assert_contains!(text, "world");
        assert_contains!(text, "Hello");
    }

    #[test]
    fn assert_not_contains_macro_works() {
        let text = "Hello, world!";
        assert_not_contains!(text, "goodbye");
        assert_not_contains!(text, "xyz");
    }

    #[test]
    fn assert_json_valid_macro_works() {
        assert_json_valid!(r#"{"key": "value"}"#);
        assert_json_valid!(r"[1, 2, 3]");
        assert_json_valid!(r"null");
    }

    #[test]
    fn assert_ansi_codes_macro_works() {
        assert_ansi_codes!("\x1b[31mred\x1b[0m");
    }

    #[test]
    fn assert_no_ansi_codes_macro_works() {
        assert_no_ansi_codes!("plain text");
    }

    #[test]
    fn assert_float_eq_macro_works() {
        assert_float_eq!(70.0, 70.0);
        assert_float_eq!(0.1 + 0.2, 0.3, 0.001);
    }

    #[test]
    fn status_payload_factories_work() {
        let operational = make_test_status_operational();
        assert_eq!(operational.indicator, StatusIndicator::None);

        let outage = make_test_status_major_outage();
        assert_eq!(outage.indicator, StatusIndicator::Major);
    }

    #[test]
    fn cost_payload_factory_creates_complete_payload() {
        let cost = make_test_cost_payload("claude");
        assert_eq!(cost.provider, "claude");
        assert_eq!(cost.source, "local");
        assert!(cost.session_cost_usd.is_some());
        assert!(cost.last_30_days_cost_usd.is_some());
        assert!(!cost.daily.is_empty());
        assert!(cost.totals.is_some());
    }

    #[test]
    fn test_helpers_create_valid_content() {
        let jsonl = make_test_jsonl_events();
        assert!(jsonl.contains("completion"));
        assert!(jsonl.contains("claude-3-opus"));

        let config = make_test_config_toml();
        assert!(config.contains("[general]"));
        assert!(config.contains("timeout_seconds"));

        let token_accounts = make_test_token_accounts_json();
        assert_json_valid!(&token_accounts);
        assert!(token_accounts.contains("claude"));
    }
}
