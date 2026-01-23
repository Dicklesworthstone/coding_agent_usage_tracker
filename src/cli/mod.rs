//! CLI argument parsing and command dispatch.

pub mod args;
pub mod cost;
pub mod doctor;
pub mod history;
pub mod prompt;
pub mod session;
pub mod usage;
pub mod watch;

pub use args::{Cli, Commands, OutputFormat};
