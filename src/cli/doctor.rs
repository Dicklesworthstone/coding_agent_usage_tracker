//! Doctor command implementation.

use crate::cli::args::{DoctorArgs, OutputFormat};
use crate::core::doctor::checks::check_all_providers;
use crate::core::doctor::{CheckStatus, DiagnosticCheck, DoctorReport};
use crate::core::provider::{Provider, ProviderSelection};
use crate::error::Result;
use crate::render::doctor;
use crate::storage::config::Config;
use std::time::Instant;

/// Execute the doctor command.
pub async fn execute(
    args: &DoctorArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    let start = Instant::now();

    tracing::debug!(?args.provider, ?args.timeout, "Starting doctor checks");

    // Parse provider selection
    let providers = if let Some(provider_args) = &args.provider {
        let mut providers = Vec::new();
        for name in provider_args {
            let selection = ProviderSelection::from_arg(name)?;
            providers.extend(selection.providers());
        }
        providers
    } else {
        // Default to primary providers (Codex + Claude)
        Provider::PRIMARY.to_vec()
    };

    // Check config status
    let config_status = check_config();

    // Run provider checks in parallel
    let provider_health = check_all_providers(&providers).await;

    // Build report
    let report = DoctorReport {
        caut_version: env!("CARGO_PKG_VERSION").to_string(),
        caut_git_sha: option_env!("VERGEN_GIT_SHA")
            .unwrap_or("unknown")
            .to_string(),
        config_status,
        providers: provider_health,
        total_duration: start.elapsed(),
    };

    // Render output
    let output = doctor::render_human(&report, no_color)?;
    match format {
        OutputFormat::Human => {
            print!("{}", output);
        }
        OutputFormat::Json => {
            let json = doctor::render_json(&report, pretty)?;
            println!("{}", json);
        }
        OutputFormat::Md => {
            let md = doctor::render_md(&report)?;
            print!("{}", md);
        }
    }

    // Return exit code based on health status
    let (_, needs_attention) = report.summary();
    if needs_attention > 0 {
        // Non-zero exit for scripting
        std::process::exit(1);
    }

    Ok(())
}

/// Check if configuration is present and valid.
fn check_config() -> DiagnosticCheck {
    let start = Instant::now();

    match Config::load() {
        Ok(_config) => {
            let config_path = Config::config_path();
            if config_path.exists() {
                DiagnosticCheck {
                    name: "Config".to_string(),
                    status: CheckStatus::Pass {
                        details: Some(format!("{}", config_path.display())),
                    },
                    duration: Some(start.elapsed()),
                }
            } else {
                // Config loaded but using defaults (file doesn't exist)
                DiagnosticCheck {
                    name: "Config".to_string(),
                    status: CheckStatus::Pass {
                        details: Some("Using defaults".to_string()),
                    },
                    duration: Some(start.elapsed()),
                }
            }
        }
        Err(e) => DiagnosticCheck {
            name: "Config".to_string(),
            status: CheckStatus::Fail {
                reason: format!("Failed to load: {}", e),
                suggestion: Some("Check ~/.config/caut/config.toml".to_string()),
            },
            duration: Some(start.elapsed()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_config_returns_check() {
        let check = check_config();
        assert_eq!(check.name, "Config");
        assert!(check.duration.is_some());
    }
}
