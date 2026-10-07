//! caut - Coding Agent Usage Tracker
//!
//! A CLI tool for monitoring LLM provider usage (Codex, Claude, Gemini, etc.).
//! This is a Rust port of `CodexBar`'s CLI functionality.

// Note: deny (not forbid) to allow #[allow(unsafe_code)] in test helpers for env var manipulation
#![deny(unsafe_code)]
// The usage pipeline dispatches to one async fetcher per provider; proving
// those futures `Send` (for `tokio::spawn`) exceeds the default limit.
#![recursion_limit = "256"]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]
// Allow async functions without await - stub functions will use async when implemented.
// `unused_async_trait_impl` is the newer nightly-only sibling lint for `async fn`s in
// trait impls (e.g. the cost scanner); `unknown_lints` keeps older nightlies from
// tripping on the name.
#![allow(unknown_lints)]
#![allow(clippy::unused_async, clippy::unused_async_trait_impl)]

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
