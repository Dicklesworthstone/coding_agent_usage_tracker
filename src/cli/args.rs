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

    /// Manage token accounts
    #[command(subcommand)]
    TokenAccounts(TokenAccountsCommand),

    /// Diagnose caut setup and provider health
    Doctor(DoctorArgs),
}

/// Arguments for the `usage` command.
#[derive(Parser, Debug)]
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
}

impl UsageArgs {
    /// Validate argument combinations.
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::CautError;

        // --all-accounts conflicts with --account and --account-index
        if self.all_accounts && (self.account.is_some() || self.account_index.is_some()) {
            return Err(CautError::AllAccountsConflict);
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
            web_timeout: None,
            web_debug_dump_html: false,
            watch: false,
            interval: 30,
        };
        assert!(args.validate().is_err());
    }
}
