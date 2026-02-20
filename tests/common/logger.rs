//! Test logging infrastructure for structured test output and debugging.
#![allow(dead_code)]
//!
//! Provides a `TestLogger` for structured logging during tests with support for:
//! - Console and file output
//! - Log levels controlled by `TEST_LOG_LEVEL` env var
//! - JSON output mode for CI parsing
//! - Duration tracking per test
//! - Phase tracking (setup, test, teardown)
//!
//! # Usage
//!
//! ```rust,ignore
//! use common::logger::{TestLogger, init_test_logging};
//!
//! // Initialize once per test module (optional, auto-inits on first use)
//! init_test_logging();
//!
//! #[test]
//! fn test_example() {
//!     let log = TestLogger::new("test_example");
//!     log.info("Starting test");
//!
//!     log.phase("setup");
//!     // ... setup code ...
//!
//!     log.phase("test");
//!     log.debug("Intermediate result");
//!
//!     log.phase("teardown");
//!     // ... cleanup ...
//!
//!     log.finish_ok();  // or log.finish_err("reason")
//! }
//! ```
//!
//! # Environment Variables
//!
//! - `TEST_LOG_LEVEL` - Set log level: trace, debug, info, warn, error (default: info)
//! - `TEST_LOG_FILE` - Output file path (default: test-results.log)
//! - `TEST_LOG_JSON` - Set to "1" or "true" for JSON output format
//! - `NO_COLOR` - Disable colored output when set

use std::env;
use std::fmt::Display;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::log_capture::TestLogCapture;

// =============================================================================
// Log Levels
// =============================================================================

/// Log severity levels matching standard conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse from string, case-insensitive.
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" | "err" => Some(Self::Error),
            _ => None,
        }
    }

    /// Get ANSI color code for this level.
    #[must_use]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub const fn color_code(&self) -> &'static str {
        match self {
            Self::Trace => "\x1b[90m", // Gray
            Self::Debug => "\x1b[36m", // Cyan
            Self::Info => "\x1b[32m",  // Green
            Self::Warn => "\x1b[33m",  // Yellow
            Self::Error => "\x1b[31m", // Red
        }
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        };
        write!(f, "{s}")
    }
}

// =============================================================================
// JSON Log Entry
// =============================================================================

/// Structured log entry for JSON output mode.
#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub test: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

// =============================================================================
// Global State
// =============================================================================

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static MIN_LEVEL: OnceLock<LogLevel> = OnceLock::new();
static JSON_MODE: OnceLock<bool> = OnceLock::new();
static NO_COLOR: OnceLock<bool> = OnceLock::new();

/// Initialize test logging infrastructure.
///
/// Call once at the start of test execution. Safe to call multiple times;
/// subsequent calls are no-ops.
///
/// Configuration is read from environment variables:
/// - `TEST_LOG_LEVEL` - Minimum log level (default: info)
/// - `TEST_LOG_FILE` - Log file path (default: test-results.log)
/// - `TEST_LOG_JSON` - Enable JSON output (set to "1" or "true")
/// - `NO_COLOR` - Disable ANSI colors
pub fn init_test_logging() {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return; // Already initialized
    }

    // Parse log level
    let level = env::var("TEST_LOG_LEVEL")
        .ok()
        .and_then(|s| LogLevel::from_str(&s))
        .unwrap_or(LogLevel::Info);
    let _ = MIN_LEVEL.set(level);

    // Check JSON mode
    let json = env::var("TEST_LOG_JSON").is_ok_and(|v| v == "1" || v.to_lowercase() == "true");
    let _ = JSON_MODE.set(json);

    // Check color mode
    let no_color = env::var("NO_COLOR").is_ok();
    let _ = NO_COLOR.set(no_color);

    // Open log file
    let log_path =
        env::var("TEST_LOG_FILE").map_or_else(|_| PathBuf::from("test-results.log"), PathBuf::from);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();

    let _ = LOG_FILE.set(Mutex::new(file));
}

/// Ensure logging is initialized (auto-init on first use).
fn ensure_init() {
    if !INITIALIZED.load(Ordering::Relaxed) {
        init_test_logging();
    }
}

fn get_min_level() -> LogLevel {
    *MIN_LEVEL.get().unwrap_or(&LogLevel::Info)
}

fn is_json_mode() -> bool {
    *JSON_MODE.get().unwrap_or(&false)
}

fn use_color() -> bool {
    !NO_COLOR.get().unwrap_or(&false)
}

fn write_to_file(content: &str) {
    if let Some(file_mutex) = LOG_FILE.get()
        && let Ok(mut guard) = file_mutex.lock()
        && let Some(ref mut file) = *guard
    {
        let _ = writeln!(file, "{content}");
    }
}

// =============================================================================
// TestLogger
// =============================================================================

/// Per-test logger with structured output and duration tracking.
///
/// Create one `TestLogger` per test function to track timing and phases.
///
/// # Example
///
/// ```rust,ignore
/// #[test]
/// fn test_something() {
///     let log = TestLogger::new("test_something");
///     log.info("Test starting");
///
///     // ... test code ...
///
///     log.finish_ok();
/// }
/// ```
pub struct TestLogger {
    test_name: String,
    start_time: Instant,
    current_phase: Mutex<String>,
}

impl TestLogger {
    /// Create a new test logger.
    ///
    /// # Arguments
    ///
    /// * `test_name` - Name of the test (usually the function name)
    #[must_use]
    pub fn new(test_name: &str) -> Self {
        ensure_init();

        let logger = Self {
            test_name: test_name.to_string(),
            start_time: Instant::now(),
            current_phase: Mutex::new("init".to_string()),
        };

        logger.log(LogLevel::Info, "Test starting", None);
        logger
    }

    /// Create logger with capture for assertions.
    pub fn with_capture(test_name: &str) -> (Self, TestLogCapture) {
        let capture = TestLogCapture::start();
        let logger = Self::new(test_name);
        (logger, capture)
    }

    /// Set the current test phase.
    ///
    /// Common phases: "setup", "test", "teardown"
    pub fn phase(&self, phase: &str) {
        if let Ok(mut current) = self.current_phase.lock() {
            *current = phase.to_string();
        }
        self.log(LogLevel::Debug, &format!("Phase: {phase}"), None);
    }

    /// Log a trace message.
    pub fn trace(&self, message: &str) {
        self.log(LogLevel::Trace, message, None);
    }

    /// Log a debug message.
    pub fn debug(&self, message: &str) {
        self.log(LogLevel::Debug, message, None);
    }

    /// Log an info message.
    pub fn info(&self, message: &str) {
        self.log(LogLevel::Info, message, None);
    }

    /// Log a warning message.
    pub fn warn(&self, message: &str) {
        self.log(LogLevel::Warn, message, None);
    }

    /// Log an error message.
    pub fn error(&self, message: &str) {
        self.log(LogLevel::Error, message, None);
    }

    /// Log with additional structured context.
    pub fn with_context(&self, level: LogLevel, message: &str, context: serde_json::Value) {
        self.log(level, message, Some(context));
    }

    /// Log an HTTP request (for HTTP tests).
    pub fn http_request(&self, method: &str, url: &str) {
        self.debug(&format!("HTTP {method} {url}"));
    }

    /// Log an HTTP response (for HTTP tests).
    pub fn http_response(&self, status: u16, duration_ms: u64) {
        self.debug(&format!("HTTP Response: {status} ({duration_ms}ms)"));
    }

    /// Mark test as passed with duration.
    #[allow(clippy::cast_possible_truncation)]
    pub fn finish_ok(&self) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        let msg = format!("Test passed (duration: {duration_ms}ms)");
        self.log_with_duration(LogLevel::Info, &msg, duration_ms);
    }

    /// Mark test as failed with reason.
    #[allow(clippy::cast_possible_truncation)]
    pub fn finish_err(&self, reason: &str) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        let msg = format!("Test FAILED: {reason} (duration: {duration_ms}ms)");
        self.log_with_duration(LogLevel::Error, &msg, duration_ms);
    }

    /// Get elapsed time since test start.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    // Internal logging implementation
    fn log(&self, level: LogLevel, message: &str, context: Option<serde_json::Value>) {
        if level < get_min_level() {
            return;
        }

        let timestamp = Utc::now();
        let phase = self.current_phase.lock().ok().map(|p| p.clone());

        if is_json_mode() {
            self.log_json(level, message, phase.as_deref(), None, context);
        } else {
            self.log_text(level, message, timestamp);
        }
    }

    fn log_with_duration(&self, level: LogLevel, message: &str, duration_ms: u64) {
        if level < get_min_level() {
            return;
        }

        let timestamp = Utc::now();
        let phase = self.current_phase.lock().ok().map(|p| p.clone());

        if is_json_mode() {
            self.log_json(level, message, phase.as_deref(), Some(duration_ms), None);
        } else {
            self.log_text(level, message, timestamp);
        }
    }

    fn log_json(
        &self,
        level: LogLevel,
        message: &str,
        phase: Option<&str>,
        duration_ms: Option<u64>,
        context: Option<serde_json::Value>,
    ) {
        let entry = LogEntry {
            timestamp: Utc::now(),
            level,
            test: self.test_name.clone(),
            message: message.to_string(),
            phase: phase.map(String::from),
            duration_ms,
            context,
        };

        if let Ok(json) = serde_json::to_string(&entry) {
            eprintln!("{json}");
            write_to_file(&json);
        }
    }

    fn log_text(&self, level: LogLevel, message: &str, timestamp: DateTime<Utc>) {
        let ts = timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ");

        let line = if use_color() {
            let reset = "\x1b[0m";
            let color = level.color_code();
            format!(
                "[{ts}] [{color}{level}{reset}] [{}] {message}",
                self.test_name
            )
        } else {
            format!("[{ts}] [{level}] [{}] {message}", self.test_name)
        };

        eprintln!("{line}");
        // Strip ANSI codes for file output
        let plain = strip_ansi(&line);
        write_to_file(&plain);
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Strip ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }

    result
}

/// Quick log a message without a `TestLogger` instance.
///
/// Useful for one-off logging in test setup/teardown.
pub fn log_test_message(test_name: &str, level: LogLevel, message: &str) {
    ensure_init();

    if level < get_min_level() {
        return;
    }

    let timestamp = Utc::now();

    if is_json_mode() {
        let entry = LogEntry {
            timestamp,
            level,
            test: test_name.to_string(),
            message: message.to_string(),
            phase: None,
            duration_ms: None,
            context: None,
        };

        if let Ok(json) = serde_json::to_string(&entry) {
            eprintln!("{json}");
            write_to_file(&json);
        }
    } else {
        let ts = timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ");
        let line = if use_color() {
            let reset = "\x1b[0m";
            let color = level.color_code();
            format!("[{ts}] [{color}{level}{reset}] [{test_name}] {message}")
        } else {
            format!("[{ts}] [{level}] [{test_name}] {message}")
        };

        eprintln!("{line}");
        write_to_file(&strip_ansi(&line));
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_parsing() {
        assert_eq!(LogLevel::from_str("trace"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::from_str("DEBUG"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("Info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("WARN"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("warning"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("invalid"), None);
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn test_logger_basic() {
        let log = TestLogger::new("test_logger_basic");
        log.info("This is an info message");
        log.debug("This is a debug message");
        log.finish_ok();
    }

    #[test]
    fn test_logger_phases() {
        let log = TestLogger::new("test_logger_phases");

        log.phase("setup");
        log.debug("Setting up test data");

        log.phase("test");
        log.info("Running assertions");

        log.phase("teardown");
        log.debug("Cleaning up");

        log.finish_ok();
    }

    #[test]
    fn test_logger_http() {
        let log = TestLogger::new("test_logger_http");

        log.http_request("GET", "https://api.example.com/status");
        log.http_response(200, 45);

        log.finish_ok();
    }

    #[test]
    fn test_logger_with_context() {
        let log = TestLogger::new("test_logger_with_context");

        let context = serde_json::json!({
            "request_id": "abc123",
            "retry_count": 3
        });
        log.with_context(LogLevel::Debug, "Request details", context);

        log.finish_ok();
    }

    #[test]
    fn strip_ansi_codes() {
        let colored = "\x1b[32mgreen\x1b[0m \x1b[31mred\x1b[0m";
        let stripped = strip_ansi(colored);
        assert_eq!(stripped, "green red");
    }

    #[test]
    fn quick_log_message() {
        log_test_message("quick_test", LogLevel::Info, "Quick message without logger");
    }

    #[test]
    fn log_entry_serialization() {
        let entry = LogEntry {
            timestamp: Utc::now(),
            level: LogLevel::Info,
            test: "test_entry".to_string(),
            message: "Test message".to_string(),
            phase: Some("test".to_string()),
            duration_ms: Some(42),
            context: None,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"level\":\"INFO\""));
        assert!(json.contains("\"test\":\"test_entry\""));
        assert!(json.contains("\"duration_ms\":42"));
    }
}
