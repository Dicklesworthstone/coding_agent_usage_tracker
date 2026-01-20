//! Integration tests for logging format initialization.

use caut::core::logging::{self, LogFormat, LogLevel};

#[test]
fn test_log_format_human() {
    logging::init(LogLevel::Debug, LogFormat::Human, None, false);
}

#[test]
fn test_log_format_json() {
    logging::init(LogLevel::Debug, LogFormat::Json, None, false);
}
