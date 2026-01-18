//! JSONL logging to stderr.
//!
//! Implements CodexBar's `--json-output` logging behavior.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;

/// Log level from CLI argument.
#[derive(Debug, Clone, Copy, Default)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    #[default]
    Error,
    Critical,
}

impl LogLevel {
    /// Parse from CLI argument.
    #[must_use]
    pub fn from_arg(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "verbose" | "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            "critical" | "crit" => Some(Self::Critical),
            _ => None,
        }
    }

    /// Convert to tracing filter string.
    #[must_use]
    pub const fn as_filter(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error | Self::Critical => "error",
        }
    }
}

/// Initialize logging with the given settings.
pub fn init(level: LogLevel, json_output: bool, verbose: bool) {
    let level = if verbose && matches!(level, LogLevel::Error) {
        LogLevel::Debug
    } else {
        level
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("caut={}", level.as_filter())));

    if json_output {
        // JSONL format to stderr
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr)
                .with_span_events(FmtSpan::CLOSE),
        );
        tracing::subscriber::set_global_default(subscriber).ok();
    } else {
        // Human-readable format to stderr
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .without_time(),
        );
        tracing::subscriber::set_global_default(subscriber).ok();
    }
}
