//! CLI argument definitions using clap.
//!
//! Matches CodexBar CLI semantics.
//! See EXISTING_CODEXBAR_STRUCTURE.md section 2.

use clap::{Parser, Subcommand, ValueEnum};

/// Coding Agent Usage Tracker - Monitor LLM provider usage.
#[derive(Parser, Debug)]
#[command(name = "caut")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    // === Global flags ===
    /// Output format
    #[arg(long, value_enum, default_value = "human", global = true)]
    pub format: OutputFormat,

    /// Shorthand for --format json
    #[arg(long, global = true)]
    pub json: bool,

    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Log level
    #[arg(long, value_name = "LEVEL", global = true)]
    pub log_level: Option<String>,

    /// Emit JSONL logs to stderr
    #[arg(long, global = true)]
    pub json_output: bool,

    /// Verbose output (sets log level to debug)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Print rich output diagnostics and exit
    #[arg(long, global = true)]
    pub debug_rich: bool,
}

impl Cli {
    /// Resolve the effective output format.
    #[must_use]
    pub fn effective_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.format
        }
    }
}

/// Available commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Show usage for providers (default command)
    Usage(UsageArgs),

    /// Show local cost usage
    Cost(CostArgs),

    /// Show session cost attribution
    Session(SessionArgs),

    /// Manage usage history and retention
    #[command(subcommand)]
    History(HistoryCommand),

    /// Manage token accounts
    #[command(subcommand)]
    TokenAccounts(TokenAccountsCommand),

    /// Diagnose caut setup and provider health
    Doctor(DoctorArgs),

    /// Output usage for shell prompt integration (fast, cached)
    Prompt(PromptArgs),

    /// Launch interactive TUI dashboard
    Dashboard(DashboardArgs),
}

/// History subcommands.
#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    /// Display usage history with trend visualization
    Show(HistoryShowArgs),
    /// Prune old history data according to retention policy
    Prune(HistoryPruneArgs),
    /// Show history database statistics
    Stats,
    /// Export history data to JSON or CSV
    Export(HistoryExportArgs),
}

/// Arguments for `history show`.
#[derive(Parser, Debug)]
pub struct HistoryShowArgs {
    /// Provider to show history for (defaults to all)
    #[arg(short, long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Number of days to show (default: 7)
    #[arg(short, long, value_name = "DAYS", default_value = "7")]
    pub days: u32,

    /// Use ASCII characters instead of Unicode
    #[arg(long)]
    pub ascii: bool,
}

/// Arguments for `history prune`.
#[derive(Parser, Debug)]
pub struct HistoryPruneArgs {
    /// Preview what would be deleted without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Days to keep detailed snapshots (default: 30)
    #[arg(long, value_name = "DAYS")]
    pub keep_days: Option<i64>,

    /// Days to keep daily aggregates (default: 365)
    #[arg(long, value_name = "DAYS")]
    pub keep_aggregates: Option<i64>,

    /// Maximum database size in MB (default: 100)
    #[arg(long, value_name = "MB")]
    pub max_size_mb: Option<u64>,
}

/// Arguments for `history export`.
#[derive(Parser, Debug)]
pub struct HistoryExportArgs {
    /// Export format (json or csv)
    #[arg(short, long, value_name = "FORMAT", default_value = "json")]
    pub format: ExportFormat,

    /// Output file path (defaults to stdout)
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<std::path::PathBuf>,

    /// Start date (RFC3339 or YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,

    /// End date (RFC3339 or YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub until: Option<String>,

    /// Filter by provider
    #[arg(short, long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Maximum number of rows to export
    #[arg(short, long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Export format for history data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    #[default]
    Json,
    Csv,
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            _ => Err(format!("Unknown export format: {s}. Use 'json' or 'csv'.")),
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Csv => write!(f, "csv"),
        }
    }
}

/// Arguments for the `usage` command.
#[derive(Parser, Debug, Clone)]
pub struct UsageArgs {
    /// Provider to query (name, "both", or "all")
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Account label to use
    #[arg(long, value_name = "LABEL")]
    pub account: Option<String>,

    /// Account index to use (1-based)
    #[arg(long, value_name = "N")]
    pub account_index: Option<usize>,

    /// Query all accounts
    #[arg(long)]
    pub all_accounts: bool,

    /// Hide credits in text output
    #[arg(long)]
    pub no_credits: bool,

    /// Fetch provider status
    #[arg(long)]
    pub status: bool,

    /// Data source (auto, web, cli, oauth)
    #[arg(long, value_name = "SOURCE")]
    pub source: Option<String>,

    /// Shorthand for --source web
    #[arg(long)]
    pub web: bool,

    /// Timeout per provider fetch in seconds (overrides defaults)
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// Web fetch timeout in seconds
    #[arg(long, value_name = "SECONDS")]
    pub web_timeout: Option<u64>,

    /// Dump HTML for web debugging
    #[arg(long, hide = true)]
    pub web_debug_dump_html: bool,

    /// Run in watch mode, continuously updating display.
    #[arg(long, short = 'w')]
    pub watch: bool,

    /// Interval between updates in seconds (default: 30).
    #[arg(long, default_value = "30")]
    pub interval: u64,

    /// Use TUI dashboard mode (interactive terminal UI with ratatui).
    /// Implies --watch mode.
    #[arg(long, short = 't')]
    pub tui: bool,
}

impl UsageArgs {
    /// Validate argument combinations.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CautError;

        // --all-accounts conflicts with --account and --account-index
        if self.all_accounts && (self.account.is_some() || self.account_index.is_some()) {
            return Err(CautError::AllAccountsConflict);
        }

        if self.timeout == Some(0) {
            return Err(CautError::Config(
                "Timeout must be greater than 0 seconds".to_string(),
            ));
        }

        if self.web_timeout == Some(0) {
            return Err(CautError::Config(
                "Web timeout must be greater than 0 seconds".to_string(),
            ));
        }

        if self.watch && self.interval == 0 {
            return Err(CautError::Config(
                "Watch interval must be greater than 0 seconds".to_string(),
            ));
        }

        Ok(())
    }

    /// Get effective source mode.
    #[must_use]
    pub fn effective_source(&self) -> crate::core::fetch_plan::SourceMode {
        use crate::core::fetch_plan::SourceMode;

        if self.web {
            return SourceMode::Web;
        }

        self.source
            .as_deref()
            .and_then(SourceMode::from_arg)
            .unwrap_or_default()
    }

    /// Resolve timeout override (CLI --timeout takes precedence over --web-timeout).
    #[must_use]
    pub fn effective_timeout_override(&self) -> Option<u64> {
        self.timeout.or(self.web_timeout)
    }
}

/// Arguments for the `cost` command.
#[derive(Parser, Debug)]
pub struct CostArgs {
    /// Provider to query (name, "both", or "all")
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Refresh cached cost data
    #[arg(long)]
    pub refresh: bool,
}

/// Arguments for the `doctor` command.
#[derive(Parser, Debug)]
pub struct DoctorArgs {
    /// Only check specific provider(s)
    #[arg(short, long, value_name = "PROVIDER")]
    pub provider: Option<Vec<String>>,

    /// Timeout for each provider check in seconds
    #[arg(long, default_value = "5")]
    pub timeout: u64,
}

/// Arguments for the `prompt` command.
#[derive(Parser, Debug)]
pub struct PromptArgs {
    /// Provider to show (defaults to primary configured provider)
    #[arg(short, long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value = "compact")]
    pub prompt_format: PromptFormat,

    /// Include ANSI color codes
    #[arg(long)]
    pub color: bool,

    /// Disable ANSI color codes
    #[arg(long)]
    pub no_color: bool,

    /// Maximum cache age in seconds before showing empty (default: 60)
    /// Only used when --strict-freshness is enabled.
    #[arg(long, value_name = "SECONDS", default_value = "60")]
    pub cache_max_age: u64,

    /// Strict freshness mode: show nothing if cache exceeds max_age.
    /// Without this flag, stale data is shown with a staleness indicator (~/?).
    #[arg(long)]
    pub strict_freshness: bool,

    /// Generate shell integration snippet and exit
    #[arg(long, value_enum)]
    pub install: Option<ShellType>,
}

/// Arguments for the `dashboard` command.
#[derive(Parser, Debug)]
pub struct DashboardArgs {
    /// Provider to query (name, "both", or "all")
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Data source (auto, web, cli, oauth)
    #[arg(long, value_name = "SOURCE")]
    pub source: Option<String>,

    /// Interval between updates in seconds (default: 30)
    #[arg(long, default_value = "30")]
    pub interval: u64,
}

/// Arguments for the `session` command.
#[derive(Parser, Debug, Clone, Default)]
pub struct SessionArgs {
    /// Show specific session by ID (partial match supported)
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    /// List recent sessions
    #[arg(long, short = 'l')]
    pub list: bool,

    /// Show all sessions from today
    #[arg(long)]
    pub today: bool,

    /// Filter by provider (claude, codex)
    #[arg(long, short = 'p', value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Maximum number of sessions to show in list mode (default: 10)
    #[arg(long, short = 'n', value_name = "N", default_value = "10")]
    pub limit: usize,
}

impl DashboardArgs {
    /// Convert to UsageArgs for the TUI runner.
    #[must_use]
    pub fn to_usage_args(&self) -> UsageArgs {
        UsageArgs {
            provider: self.provider.clone(),
            account: None,
            account_index: None,
            all_accounts: false,
            no_credits: false,
            status: true, // Always show status in dashboard
            source: self.source.clone(),
            web: false,
            timeout: None,
            web_timeout: None,
            web_debug_dump_html: false,
            watch: true,
            interval: self.interval,
            tui: true,
        }
    }
}

/// Prompt output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum PromptFormat {
    /// Compact format: "claude:45%|$12"
    #[default]
    Compact,
    /// Full format: "claude:45%/67% codex:$12"
    Full,
    /// Minimal format: "45%"
    Minimal,
    /// Icon format: "⚡45%"
    Icon,
}

/// Shell type for installation snippets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellType {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
}

/// Token account subcommands.
#[derive(Subcommand, Debug)]
pub enum TokenAccountsCommand {
    /// List configured accounts
    List {
        /// Provider to list accounts for
        #[arg(long)]
        provider: Option<String>,
    },

    /// Convert between CodexBar and caut formats
    Convert {
        /// Source format
        #[arg(long, value_name = "FORMAT")]
        from: String,

        /// Target format
        #[arg(long, value_name = "FORMAT")]
        to: String,
    },
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable rich output
    #[default]
    Human,
    /// JSON output
    Json,
    /// Markdown output
    Md,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses() {
        Cli::command().debug_assert();
    }

    #[test]
    fn usage_args_validate() {
        let args = UsageArgs {
            provider: None,
            account: Some("test".to_string()),
            account_index: None,
            all_accounts: true,
            no_credits: false,
            status: false,
            source: None,
            web: false,
            timeout: None,
            web_timeout: None,
            web_debug_dump_html: false,
            watch: false,
            interval: 30,
            tui: false,
        };
        assert!(args.validate().is_err());
    }
}
