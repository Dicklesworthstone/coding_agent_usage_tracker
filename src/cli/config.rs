//! `caut config` command: locate, show and create the config file.

use std::path::PathBuf;

use crate::cli::args::{ConfigCommand, OutputFormat};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::config::{Config, ENV_CONFIG};

/// Execute a `config` subcommand.
///
/// # Errors
/// Returns an error if the config file is invalid (`show`), already exists
/// without `--force` (`init`), or cannot be written.
pub fn execute(cmd: &ConfigCommand, format: OutputFormat, pretty: bool) -> Result<()> {
    let path = config_path();
    match cmd {
        ConfigCommand::Path => {
            println!("{}", path.display());
            Ok(())
        }
        ConfigCommand::Show => {
            let config = Config::load_from(&path)?;
            config.validate()?;
            let rendered = match format {
                OutputFormat::Json => {
                    if pretty {
                        serde_json::to_string_pretty(&config)?
                    } else {
                        serde_json::to_string(&config)?
                    }
                }
                OutputFormat::Human | OutputFormat::Md => {
                    let body = toml::to_string_pretty(&config).map_err(|e| {
                        CautError::Config(format!("Failed to serialize config: {e}"))
                    })?;
                    let origin = if path.exists() {
                        format!("# Loaded from {}", path.display())
                    } else {
                        format!("# No config file at {} — showing defaults", path.display())
                    };
                    format!("{origin}\n{body}")
                }
            };
            println!("{rendered}");
            Ok(())
        }
        ConfigCommand::Init { force } => {
            if path.exists() && !force {
                return Err(CautError::Config(format!(
                    "Config file already exists at {} (use --force to overwrite)",
                    path.display()
                )));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, starter_config())?;
            println!("Wrote {}", path.display());
            Ok(())
        }
    }
}

/// The config file path, honoring `CAUT_CONFIG`.
fn config_path() -> PathBuf {
    std::env::var(ENV_CONFIG)
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .map_or_else(Config::config_path, PathBuf::from)
}

/// A commented starter config that parses to the defaults.
fn starter_config() -> String {
    let providers = Provider::ALL
        .iter()
        .map(|p| p.cli_name())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"# caut configuration. CLI flags and CAUT_* environment variables override it.

[general]
# Timeout for each provider fetch, in seconds (1-300).
timeout_seconds = 30
# Fetch provider status pages with every `caut usage` (same as --status).
include_status = false

[providers]
# Providers `caut usage` queries when --provider is not given.
# Available: {providers}
default_providers = ["claude", "codex"]

# Per-provider settings. `enabled = false` drops a provider from
# default_providers / CAUT_PROVIDERS; an explicit --provider still works.
# [providers.cursor]
# enabled = true
# timeout_seconds = 15

[output]
# Default output format: "human", "json" or "md".
# format = "human"
color = true
pretty = false
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_config_parses_to_valid_defaults() {
        let config: Config = toml::from_str(&starter_config()).unwrap();
        config.validate().unwrap();
        assert_eq!(config.general.timeout_seconds, 30);
        assert_eq!(config.providers.default_providers, ["claude", "codex"]);
        assert!(config.output.color);
    }

    #[test]
    fn starter_config_lists_every_provider() {
        let text = starter_config();
        for provider in Provider::ALL {
            assert!(text.contains(provider.cli_name()), "{provider:?}");
        }
    }
}
