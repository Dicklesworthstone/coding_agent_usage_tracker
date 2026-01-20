//! Tests for the log capture infrastructure.

use tracing::{error, info, warn};

mod common;
use common::log_capture::TestLogCapture;
use common::logger::TestLogger;

#[test]
fn test_log_capture_basic() {
    let capture = TestLogCapture::start();

    info!("This is an info message");
    warn!("This is a warning");

    capture.assert_logged("This is an info message");
    capture.assert_logged("This is a warning");
    capture.assert_logged_at_level(tracing::Level::INFO, "info message");
    capture.assert_logged_at_level(tracing::Level::WARN, "warning");
}

#[test]
fn test_log_capture_structured() {
    let capture = TestLogCapture::start();

    info!(user = "test_user", id = 123, "User action logged");

    capture.assert_logged("User action logged");
    capture.assert_field_logged("user", "test_user");
    capture.assert_field_logged("id", "123");
}

#[test]
#[should_panic(expected = "Unexpected errors")]
fn test_log_capture_errors() {
    let capture = TestLogCapture::start();

    info!("Everything is fine");
    capture.assert_no_errors();

    error!("Something went wrong");

    // This should panic
    capture.assert_no_errors();
}

#[test]
fn test_logger_with_capture_integration() {
    let (_logger, capture) = TestLogger::with_capture("test_logger_with_capture_integration");

    info!("Integration test log");

    capture.assert_logged("Integration test log");
}
