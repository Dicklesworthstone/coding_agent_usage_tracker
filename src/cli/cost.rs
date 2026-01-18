//! Cost command implementation.

use crate::cli::args::{CostArgs, OutputFormat};
use crate::core::cost_scanner::CostScanner;
use crate::core::models::{CostPayload, RobotOutput};
use crate::core::provider::ProviderSelection;
use crate::error::{CautError, Result};
use crate::render::{human, robot};

/// Execute the cost command.
pub async fn execute(
    args: &CostArgs,
    format: OutputFormat,
    pretty: bool,
    no_color: bool,
) -> Result<()> {
    // Parse provider selection
    let selection = args
        .provider
        .as_deref()
        .map(ProviderSelection::from_arg)
        .transpose()?
        .unwrap_or_default();

    // Filter to providers that support cost scanning
    let providers: Vec<_> = selection
        .providers()
        .into_iter()
        .filter(|p| p.supports_cost_scan())
        .collect();

    if providers.is_empty() {
        return Err(CautError::Config(
            "No selected providers support local cost scanning. Only Claude and Codex are supported."
                .to_string(),
        ));
    }

    tracing::debug!(
        ?providers,
        refresh = args.refresh,
        ?format,
        "Starting cost scan"
    );

    // Scan each provider
    let scanner = CostScanner::new();
    let mut results: Vec<CostPayload> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for provider in &providers {
        match scanner.scan(*provider, args.refresh).await {
            Ok(payload) => results.push(payload),
            Err(e) => {
                tracing::warn!(?provider, error = %e, "Failed to scan cost data");
                errors.push(format!("{}: {}", provider.cli_name(), e));
            }
        }
    }

    // Render output based on format
    match format {
        OutputFormat::Human => {
            let output = human::render_cost(&results, no_color)?;
            print!("{}", output);

            // Print errors to stderr
            for error in &errors {
                eprintln!("Error: {}", error);
            }
        }
        OutputFormat::Json => {
            let robot_output = RobotOutput::cost(results, errors);
            let json = if pretty {
                serde_json::to_string_pretty(&robot_output)
            } else {
                serde_json::to_string(&robot_output)
            }
            .map_err(|e| CautError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json);
        }
        OutputFormat::Md => {
            let output = robot::render_markdown_cost(&results)?;
            print!("{}", output);

            // Print errors as markdown
            if !errors.is_empty() {
                println!("\n## Errors\n");
                for error in &errors {
                    println!("- {}", error);
                }
            }
        }
    }

    Ok(())
}
