//! caut - Coding Agent Usage Tracker
//!
//! CLI entry point.

#![forbid(unsafe_code)]
// The usage pipeline dispatches to one async fetcher per provider, which makes
// the command futures deep enough to overflow the default limit when
// `clippy::future_not_send` proves them `Send`.
#![recursion_limit = "256"]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

use clap::Parser;
use std::process::ExitCode;

use caut::cli::{Cli, Commands};
use caut::core::logging;

/// Build information embedded at compile time.
mod build_info {
    /// Build timestamp.
    #[expect(dead_code, reason = "reserved for future version display")]
    pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
    /// Git SHA.
    pub const GIT_SHA: &str = env!("VERGEN_GIT_SHA");
    /// Whether the build is dirty.
    pub const GIT_DIRTY: &str = env!("VERGEN_GIT_DIRTY");
    /// Rustc version.
    #[expect(dead_code, reason = "reserved for future version display")]
    pub const RUSTC_SEMVER: &str = env!("VERGEN_RUSTC_SEMVER");
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = cli
        .log_level
        .as_deref()
        .and_then(logging::LogLevel::from_arg)
        .or_else(|| logging::parse_log_level_from_env().map(logging::LogLevel::from_tracing_level))
        .unwrap_or_default();
    let log_format = if cli.json_output {
        logging::LogFormat::Json
    } else {
        logging::parse_log_format_from_env().unwrap_or_default()
    };
    let log_file = logging::parse_log_file_from_env();
    logging::init(log_level, log_format, log_file, cli.verbose);

    let format = cli.effective_format();
    let no_color = cli.no_color;
    let pretty = cli.pretty;
    let _rich_enabled = caut::rich::should_use_rich_output(format, no_color);

    if cli.debug_rich {
        let diagnostics = caut::rich::collect_rich_diagnostics(format, no_color);
        println!("{diagnostics}");
        return ExitCode::SUCCESS;
    }

    // Execute command
    let result = run(cli).await;

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{}", e);
            // Use rich error rendering (respects format, no_color, TTY, and pretty)
            let error_output = caut::render::error::render_error_full(&e, format, no_color, pretty);
            eprintln!("{error_output}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

async fn run(cli: Cli) -> caut::Result<()> {
    let format = cli.effective_format();
    let pretty = cli.pretty;
    let no_color = cli.no_color || !caut::util::env::should_use_color(cli.no_color);

    match cli.command {
        // Default to usage command
        None => {
            print_quickstart();
            Ok(())
        }

        Some(Commands::Usage(ref args)) => {
            // Output settings follow CLI > env > config file > defaults.
            let resolved = caut::storage::ResolvedConfig::resolve(&cli, Some(args))?;
            let no_color = resolved.no_color || no_color;
            caut::cli::usage::execute(args, resolved.format, resolved.pretty, no_color).await
        }

        Some(Commands::Cost(args)) => {
            caut::cli::cost::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::TokenAccounts(cmd)) => caut::cli::token_accounts::execute(cmd),

        Some(Commands::Doctor(args)) => {
            caut::cli::doctor::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::History(cmd)) => caut::cli::history::execute(&cmd, format, pretty, no_color),

        Some(Commands::Prompt(args)) => caut::cli::prompt::execute(&args),

        Some(Commands::Session(args)) => {
            caut::cli::session::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::Dashboard(args)) => {
            let usage_args = args.to_usage_args();
            caut::tui::run_dashboard(&usage_args, args.interval).await
        }

        Some(Commands::Serve(args)) => caut::cli::serve::execute(&args).await,

        Some(Commands::Query(args)) => caut::cli::query::execute(&args, pretty).await,
    }
}

/// Print quickstart help when no command is given.
fn print_quickstart() {
    println!(
        r"caut - Coding Agent Usage Tracker

Track your LLM provider usage (Codex, Claude, Gemini, and more).

USAGE:
    caut [OPTIONS] <COMMAND>

COMMANDS:
    usage           Show usage for providers (default)
    cost            Show local cost usage
    session         Show session cost attribution
    dashboard       Launch interactive TUI dashboard
    serve           Start background HTTP server for programmatic queries
    query           Query a running caut server (prints JSON to stdout)
    history         Manage usage history and retention
    token-accounts  Manage token accounts
    doctor          Diagnose caut setup and provider health
    prompt          Output usage for shell prompt integration

QUICK START:
    caut usage                    # Show usage for primary providers
    caut usage --provider all     # Show usage for all providers
    caut usage --status           # Include provider status
    caut dashboard                # Launch interactive TUI dashboard
    caut cost --provider claude   # Show Claude cost usage
    caut session                  # Show last session cost attribution
    caut session --list           # List recent sessions with costs
    caut doctor                   # Check setup and provider health

SHELL PROMPT INTEGRATION:
    caut prompt                   # Output for shell prompt (fast, cached)
    caut prompt --install bash    # Generate bash integration snippet

BACKGROUND SERVER (for plugins and scripts):
    caut serve                    # Start HTTP server on localhost:19485
    caut serve --port 8080        # Custom port
    caut query usage              # Query cached usage data (JSON)
    caut query cost               # Query cost data (JSON)
    caut query health             # Check server health

ROBOT MODE (for AI agents):
    caut usage --json             # JSON output
    caut usage --format md        # Markdown output

For more help: caut --help
"
    );

    // Print version info
    println!(
        "Version: {} ({}{})",
        env!("CARGO_PKG_VERSION"),
        &build_info::GIT_SHA[..7],
        if build_info::GIT_DIRTY == "true" {
            "-dirty"
        } else {
            ""
        }
    );
}
