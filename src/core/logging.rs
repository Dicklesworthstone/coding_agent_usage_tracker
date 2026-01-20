//! JSONL logging to stderr.
//!
//! Implements CodexBar's `--json-output` logging behavior.

use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

const LOG_LEVEL_ENV: &str = "CAUT_LOG";
const LOG_FORMAT_ENV: &str = "CAUT_LOG_FORMAT";
const LOG_FILE_ENV: &str = "CAUT_LOG_FILE";

/// Log output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable logs.
    #[default]
    Human,
    /// JSON logs (one event per line).
    Json,
    /// Compact logs (single line, terse).
    Compact,
}

impl LogFormat {
    /// Parse from string (case-insensitive).
    #[must_use]
    pub fn from_arg(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

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

    /// Convert to tracing level.
    #[must_use]
    pub const fn as_tracing_level(self) -> Level {
        match self {
            Self::Trace => Level::TRACE,
            Self::Debug => Level::DEBUG,
            Self::Info => Level::INFO,
            Self::Warn => Level::WARN,
            Self::Error | Self::Critical => Level::ERROR,
        }
    }

    /// Convert from tracing level.
    #[must_use]
    pub const fn from_tracing_level(level: Level) -> Self {
        match level {
            Level::TRACE => Self::Trace,
            Level::DEBUG => Self::Debug,
            Level::INFO => Self::Info,
            Level::WARN => Self::Warn,
            Level::ERROR => Self::Error,
        }
    }
}

/// Parse log level from CAUT_LOG env var.
#[must_use]
pub fn parse_log_level_from_env() -> Option<Level> {
    std::env::var(LOG_LEVEL_ENV).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            LogLevel::from_arg(trimmed).map(LogLevel::as_tracing_level)
        }
    })
}

/// Parse log format from CAUT_LOG_FORMAT env var.
#[must_use]
pub fn parse_log_format_from_env() -> Option<LogFormat> {
    std::env::var(LOG_FORMAT_ENV).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            LogFormat::from_arg(trimmed)
        }
    })
}

/// Parse log file path from CAUT_LOG_FILE env var.
#[must_use]
pub fn parse_log_file_from_env() -> Option<PathBuf> {
    std::env::var(LOG_FILE_ENV).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    })
}

/// Initialize logging with the given settings.
pub fn init(level: LogLevel, format: LogFormat, log_file: Option<PathBuf>, verbose: bool) {
    let level = if verbose && matches!(level, LogLevel::Error) {
        LogLevel::Debug
    } else {
        level
    };

    let file = log_file.and_then(|path| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
    });

    let make_writer = |file: Option<&std::fs::File>| -> BoxMakeWriter {
        if let Some(file) = file.and_then(|inner| inner.try_clone().ok()) {
            BoxMakeWriter::new(file)
        } else {
            BoxMakeWriter::new(std::io::stderr)
        }
    };

    let make_filter = || {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("caut={}", level.as_filter())))
    };

    match format {
        LogFormat::Json => {
            let filter = make_filter();
            let writer = make_writer(file.as_ref());
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .with_writer(writer)
                .with_span_events(FmtSpan::CLOSE)
                .try_init()
                .ok();
        }
        LogFormat::Compact => {
            let filter = make_filter();
            let writer = make_writer(file.as_ref());
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .compact()
                .with_writer(writer)
                .with_target(true)
                .try_init()
                .ok();
        }
        LogFormat::Human => {
            let filter = make_filter();
            let writer = make_writer(file.as_ref());
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_target(false)
                .without_time()
                .try_init()
                .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[allow(unsafe_code)]
    fn with_env_var(key: &str, value: &str, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prior = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        f();
        match prior {
            Some(val) => unsafe {
                std::env::set_var(key, val);
            },
            None => unsafe {
                std::env::remove_var(key);
            },
        }
    }

    #[test]
    fn test_env_var_log_level_parsing() {
        with_env_var(LOG_LEVEL_ENV, "trace", || {
            assert_eq!(parse_log_level_from_env(), Some(Level::TRACE));
        });

        with_env_var(LOG_LEVEL_ENV, "warn", || {
            assert_eq!(parse_log_level_from_env(), Some(Level::WARN));
        });
    }
}
