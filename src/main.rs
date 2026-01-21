//! caut - Coding Agent Usage Tracker
//!
//! CLI entry point.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

use clap::Parser;
use std::process::ExitCode;

use caut::cli::{Cli, Commands};
use caut::core::logging;
use caut::error::CautError;

/// Build information embedded at compile time.
mod build_info {
    /// Build timestamp.
    pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
    /// Git SHA.
    pub const GIT_SHA: &str = env!("VERGEN_GIT_SHA");
    /// Whether the build is dirty.
    pub const GIT_DIRTY: &str = env!("VERGEN_GIT_DIRTY");
    /// Rustc version.
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
        println!("{}", diagnostics);
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
            eprintln!("{}", error_output);
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

        Some(Commands::Usage(args)) => {
            caut::cli::usage::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::Cost(args)) => {
            caut::cli::cost::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::TokenAccounts(cmd)) => handle_token_accounts(cmd),

        Some(Commands::Doctor(args)) => {
            caut::cli::doctor::execute(&args, format, pretty, no_color).await
        }

        Some(Commands::History(cmd)) => caut::cli::history::execute(&cmd, format, pretty, no_color),
    }
}

fn handle_token_accounts(cmd: caut::cli::args::TokenAccountsCommand) -> caut::Result<()> {
    use caut::cli::args::TokenAccountsCommand;
    use caut::core::provider::Provider;
    use caut::storage::paths::AppPaths;
    use caut::storage::token_accounts::{TokenAccountStore, convert};

    let paths = AppPaths::new();

    match cmd {
        TokenAccountsCommand::List { provider } => {
            let store = TokenAccountStore::load(&paths.token_accounts_file())?;

            // If a provider is specified, list only that provider's accounts
            if let Some(provider_name) = provider {
                let provider = Provider::from_cli_name(&provider_name)?;

                let accounts = store.get_all(provider);
                if accounts.is_empty() {
                    println!("No accounts configured for provider: {}", provider_name);
                } else {
                    println!("Accounts for {}:", provider.display_name());
                    println!("{:<20} {:<40} {}", "Label", "ID", "Added");
                    println!("{:-<20} {:-<40} {:-<20}", "", "", "");
                    for account in accounts {
                        let added = account.added_at.format("%Y-%m-%d %H:%M");
                        println!("{:<20} {:<40} {}", account.label, account.id, added);
                    }
                }
            } else {
                // List accounts for all providers
                let mut found_any = false;
                for &provider in Provider::ALL {
                    let accounts = store.get_all(provider);
                    if !accounts.is_empty() {
                        found_any = true;
                        println!("\n{}:", provider.display_name());
                        println!("{:<20} {:<40} {}", "Label", "ID", "Added");
                        println!("{:-<20} {:-<40} {:-<20}", "", "", "");
                        for account in accounts {
                            let added = account.added_at.format("%Y-%m-%d %H:%M");
                            println!("{:<20} {:<40} {}", account.label, account.id, added);
                        }
                    }
                }
                if !found_any {
                    println!("No token accounts configured.");
                    println!(
                        "\nToken accounts file: {}",
                        paths.token_accounts_file().display()
                    );
                }
            }
            Ok(())
        }
        TokenAccountsCommand::Convert { from, to } => {
            let from_lower = from.to_lowercase();
            let to_lower = to.to_lowercase();

            // Validate formats
            if !["codexbar", "caut"].contains(&from_lower.as_str()) {
                return Err(CautError::Config(format!(
                    "Unknown source format '{from}'. Valid formats: codexbar, caut"
                )));
            }
            if !["codexbar", "caut"].contains(&to_lower.as_str()) {
                return Err(CautError::Config(format!(
                    "Unknown target format '{to}'. Valid formats: codexbar, caut"
                )));
            }

            // Determine source and destination paths
            let (src_path, dst_path) = match (from_lower.as_str(), to_lower.as_str()) {
                ("codexbar", "caut") => {
                    let src = AppPaths::codexbar_token_accounts_file().ok_or_else(|| {
                        CautError::Config(
                            "CodexBar token accounts path not available (macOS only)".to_string(),
                        )
                    })?;
                    let dst = paths.token_accounts_file();
                    (src, dst)
                }
                ("caut", "codexbar") => {
                    let dst = AppPaths::codexbar_token_accounts_file().ok_or_else(|| {
                        CautError::Config(
                            "CodexBar token accounts path not available (macOS only)".to_string(),
                        )
                    })?;
                    let src = paths.token_accounts_file();
                    (src, dst)
                }
                _ => {
                    return Err(CautError::Config(format!(
                        "Cannot convert from '{from}' to '{to}' (same format)"
                    )));
                }
            };

            // Check source exists
            if !src_path.exists() {
                return Err(CautError::Config(format!(
                    "Source file not found: {}",
                    src_path.display()
                )));
            }

            // Read and convert
            let content = std::fs::read_to_string(&src_path)?;
            let data = convert::from_codexbar(&content)?;
            let output = convert::to_codexbar(&data)?;

            // Ensure destination directory exists
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Write output
            std::fs::write(&dst_path, &output)?;

            println!("Converted {} -> {}", src_path.display(), dst_path.display());
            println!("Providers converted: {}", data.providers.len());
            Ok(())
        }
    }
}

/// Print quickstart help when no command is given.
fn print_quickstart() {
    println!(
        r#"caut - Coding Agent Usage Tracker

Track your LLM provider usage (Codex, Claude, Gemini, and more).

USAGE:
    caut [OPTIONS] <COMMAND>

COMMANDS:
    usage           Show usage for providers (default)
    cost            Show local cost usage
    token-accounts  Manage token accounts
    doctor          Diagnose caut setup and provider health

QUICK START:
    caut usage                    # Show usage for primary providers
    caut usage --provider all     # Show usage for all providers
    caut usage --status           # Include provider status
    caut cost --provider claude   # Show Claude cost usage
    caut doctor                   # Check setup and provider health

ROBOT MODE (for AI agents):
    caut usage --json             # JSON output
    caut usage --format md        # Markdown output

For more help: caut --help
"#
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
