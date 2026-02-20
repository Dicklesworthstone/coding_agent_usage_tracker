//! caut - Coding Agent Usage Tracker
//!
//! A CLI tool for monitoring LLM provider usage (Codex, Claude, Gemini, etc.).
//! This is a Rust port of `CodexBar`'s CLI functionality.

// Note: deny (not forbid) to allow #[allow(unsafe_code)] in test helpers for env var manipulation
#![deny(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
// Allow async functions without await - stub functions will use async when implemented
#![allow(clippy::unused_async)]

pub mod cli;
pub mod core;
pub mod error;
pub mod providers;
pub mod render;
pub mod rich;
pub mod storage;
pub mod tui;
pub mod util;

/// Test utilities module - included in test builds or when test-utils feature is enabled.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub use error::{CautError, ExitCode, Result};

// Re-export test utilities for external test crates
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::*;
