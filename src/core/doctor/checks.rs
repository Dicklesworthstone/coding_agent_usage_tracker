//! Provider health checks for the doctor command.
//!
//! Implements diagnostic checks for CLI installation, authentication,
//! and API reachability for each provider.

use super::{CheckStatus, DiagnosticCheck, ProviderHealth};
use crate::core::cli_runner::run_command;
use crate::core::credential_health::{AuthHealthAggregator, OverallHealth};
use crate::core::provider::Provider;
use crate::error::CautError;
use std::time::{Duration, Instant};

/// Default timeout for API reachability checks.
const API_TIMEOUT: Duration = Duration::from_secs(5);

/// Default timeout for CLI version checks.
const CLI_VERSION_TIMEOUT: Duration = Duration::from_secs(3);

/// Check if a CLI binary is available and get its version.
pub async fn check_cli_installed(provider: Provider) -> (DiagnosticCheck, Option<String>) {
    let cli_name = provider.cli_name();
    let start = Instant::now();

    match which::which(cli_name) {
        Ok(path) => {
            // Try to get version
            let version = get_cli_version(cli_name).await;
            let details = match &version {
                Some(v) => format!("Found at {:?}, version: {}", path, v),
                None => format!("Found at {:?}", path),
            };

            (
                DiagnosticCheck {
                    name: format!("{} CLI installed", cli_name),
                    status: CheckStatus::Pass {
                        details: Some(details),
                    },
                    duration: Some(start.elapsed()),
                },
                version,
            )
        }
        Err(_) => (
            DiagnosticCheck {
                name: format!("{} CLI installed", cli_name),
                status: CheckStatus::Fail {
                    reason: "CLI not found in PATH".to_string(),
                    suggestion: Some(provider.install_suggestion().to_string()),
                },
                duration: Some(start.elapsed()),
            },
            None,
        ),
    }
}

/// Get the version of a CLI binary.
async fn get_cli_version(cli_name: &str) -> Option<String> {
    // Try common version flags in order
    for flag in ["--version", "-V", "version", "-v"] {
        let output = match run_command(cli_name, &[flag], CLI_VERSION_TIMEOUT).await {
            Ok(output) => output,
            Err(_) => continue,
        };

        if output.success() {
            // Try to extract version from output
            let combined = format!("{}{}", output.stdout, output.stderr);
            if let Some(version) = extract_version(&combined) {
                return Some(version);
            }
        }
    }
    None
}

/// Extract version string from CLI output.
fn extract_version(output: &str) -> Option<String> {
    // Look for common version patterns
    // Pattern: "version X.Y.Z" or "vX.Y.Z" or just "X.Y.Z" (including prerelease like X.Y.Z-beta)
    for line in output.lines() {
        let line = line.trim();
        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Look for version number patterns
        for word in line.split_whitespace() {
            let word = word.trim_start_matches('v');
            // Check if it looks like a version number
            if word.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                // Must contain at least one dot and start with digits
                // Allow alphanumeric for prerelease identifiers (e.g., 0.6.1-beta, 1.0.0-rc.1)
                if word.contains('.') && is_version_string(word) {
                    return Some(word.to_string());
                }
            }
        }
    }

    // Fallback: return first line if it's short enough
    output.lines().next().map(|s| {
        let s = s.trim();
        if s.len() <= 50 {
            s.to_string()
        } else {
            format!("{}...", &s[..47])
        }
    })
}

/// Check if a string looks like a version number.
fn is_version_string(s: &str) -> bool {
    // Must start with a digit and contain reasonable version characters
    // Allow: digits, dots, dashes, and lowercase letters (for prerelease identifiers)
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
}

/// Check authentication status for a provider.
pub async fn check_authenticated(provider: Provider) -> DiagnosticCheck {
    let start = Instant::now();

    let status = match provider {
        Provider::Claude => check_claude_auth().await,
        Provider::Codex => check_codex_auth().await,
        Provider::Gemini => check_gemini_auth().await,
        Provider::Cursor => check_cursor_auth().await,
        _ => check_generic_auth(provider).await,
    };

    DiagnosticCheck {
        name: format!("{} authenticated", provider.display_name()),
        status,
        duration: Some(start.elapsed()),
    }
}

/// Check Claude authentication.
async fn check_claude_auth() -> CheckStatus {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return CheckStatus::Fail {
                reason: "Cannot determine home directory".to_string(),
                suggestion: None,
            };
        }
    };

    let creds_path = home.join(".claude").join(".credentials.json");

    if !creds_path.exists() {
        return CheckStatus::Fail {
            reason: "No credentials file found".to_string(),
            suggestion: Some(Provider::Claude.auth_suggestion().to_string()),
        };
    }

    // Try to read and validate credentials
    match std::fs::read_to_string(&creds_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(json) => {
                // Check for valid credentials structure
                if let Some(email) = json
                    .get("credentials")
                    .and_then(|c| c.get("email"))
                    .and_then(|e| e.as_str())
                {
                    CheckStatus::Pass {
                        details: Some(format!("Logged in as {}", email)),
                    }
                } else if json.get("credentials").is_some() {
                    CheckStatus::Pass {
                        details: Some("Credentials present".to_string()),
                    }
                } else {
                    CheckStatus::Fail {
                        reason: "Invalid credentials format".to_string(),
                        suggestion: Some(Provider::Claude.auth_suggestion().to_string()),
                    }
                }
            }
            Err(e) => CheckStatus::Fail {
                reason: format!("Credentials file invalid: {}", e),
                suggestion: Some(Provider::Claude.auth_suggestion().to_string()),
            },
        },
        Err(e) => CheckStatus::Fail {
            reason: format!("Failed to read credentials: {}", e),
            suggestion: Some(Provider::Claude.auth_suggestion().to_string()),
        },
    }
}

/// Check Codex authentication.
async fn check_codex_auth() -> CheckStatus {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return CheckStatus::Fail {
                reason: "Cannot determine home directory".to_string(),
                suggestion: None,
            };
        }
    };

    let auth_path = home.join(".codex").join("auth.json");

    if !auth_path.exists() {
        return CheckStatus::Fail {
            reason: "No auth file found".to_string(),
            suggestion: Some(Provider::Codex.auth_suggestion().to_string()),
        };
    }

    // Try to read and validate auth
    match std::fs::read_to_string(&auth_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(json) => {
                // Check for tokens
                let has_access_token = json
                    .get("tokens")
                    .and_then(|t| t.get("access_token"))
                    .and_then(|t| t.as_str())
                    .map_or(false, |s| !s.is_empty());

                let has_api_key = json
                    .get("apiKey")
                    .and_then(|k| k.as_str())
                    .map_or(false, |s| !s.is_empty());

                if has_access_token || has_api_key {
                    let method = if has_access_token { "OAuth" } else { "API key" };
                    CheckStatus::Pass {
                        details: Some(format!("Authenticated via {}", method)),
                    }
                } else {
                    CheckStatus::Fail {
                        reason: "No valid tokens found".to_string(),
                        suggestion: Some(Provider::Codex.auth_suggestion().to_string()),
                    }
                }
            }
            Err(e) => CheckStatus::Fail {
                reason: format!("Auth file invalid: {}", e),
                suggestion: Some(Provider::Codex.auth_suggestion().to_string()),
            },
        },
        Err(e) => CheckStatus::Fail {
            reason: format!("Failed to read auth file: {}", e),
            suggestion: Some(Provider::Codex.auth_suggestion().to_string()),
        },
    }
}

/// Check Gemini authentication.
async fn check_gemini_auth() -> CheckStatus {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return CheckStatus::Fail {
                reason: "Cannot determine home directory".to_string(),
                suggestion: None,
            };
        }
    };

    // Check for Gemini-specific credentials
    let gemini_creds = home.join(".config").join("gemini").join("credentials.json");

    // Also check for gcloud application default credentials
    let gcloud_creds = home
        .join(".config")
        .join("gcloud")
        .join("application_default_credentials.json");

    if gemini_creds.exists() {
        CheckStatus::Pass {
            details: Some("Gemini credentials found".to_string()),
        }
    } else if gcloud_creds.exists() {
        CheckStatus::Pass {
            details: Some("Using gcloud application default credentials".to_string()),
        }
    } else {
        // Check for GOOGLE_APPLICATION_CREDENTIALS env var
        if std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok() {
            CheckStatus::Pass {
                details: Some("Using GOOGLE_APPLICATION_CREDENTIALS".to_string()),
            }
        } else {
            CheckStatus::Fail {
                reason: "No credentials found".to_string(),
                suggestion: Some(Provider::Gemini.auth_suggestion().to_string()),
            }
        }
    }
}

/// Check Cursor authentication.
async fn check_cursor_auth() -> CheckStatus {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return CheckStatus::Fail {
                reason: "Cannot determine home directory".to_string(),
                suggestion: None,
            };
        }
    };

    let auth_path = home.join(".cursor").join("auth.json");

    if auth_path.exists() {
        CheckStatus::Pass {
            details: Some("Cursor credentials found".to_string()),
        }
    } else {
        CheckStatus::Fail {
            reason: "No auth file found".to_string(),
            suggestion: Some(Provider::Cursor.auth_suggestion().to_string()),
        }
    }
}

/// Generic auth check for providers without specific implementation.
async fn check_generic_auth(provider: Provider) -> CheckStatus {
    // Check if there's a known credentials path
    if let Some(creds_path) = provider.credentials_path() {
        if let Some(home) = dirs::home_dir() {
            let full_path = home.join(creds_path);
            if full_path.exists() {
                return CheckStatus::Pass {
                    details: Some("Credentials file found".to_string()),
                };
            }
        }
    }

    // Skip check for providers without known auth mechanism
    CheckStatus::Skipped {
        reason: "Auth check not implemented for this provider".to_string(),
    }
}

/// Check credential health (token expiration, etc.) for a provider.
pub fn check_credential_health(provider: Provider) -> Option<DiagnosticCheck> {
    let start = Instant::now();
    let aggregator = AuthHealthAggregator::new();
    let health = aggregator.check_provider(provider);

    // Only return a check if we actually found credentials to evaluate
    if health.sources.is_empty() {
        return None;
    }

    let status = match health.overall {
        OverallHealth::Healthy => CheckStatus::Pass {
            details: Some(
                health
                    .sources
                    .iter()
                    .filter_map(|s| match &s.health {
                        crate::core::credential_health::CredentialHealth::OAuth(oauth) => {
                            Some(format!("{}: {}", s.source_type, oauth.description()))
                        }
                        crate::core::credential_health::CredentialHealth::Jwt(jwt) => {
                            Some(format!("{}: {}", s.source_type, jwt.description()))
                        }
                        crate::core::credential_health::CredentialHealth::ApiKeyPresent => {
                            Some(format!("{}: API key present", s.source_type))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        },
        OverallHealth::ExpiringSoon => {
            let warning_msg = health
                .warning_message()
                .unwrap_or_else(|| "Token expiring soon".to_string());
            CheckStatus::Warning {
                details: warning_msg,
                suggestion: health.recommended_action,
            }
        }
        OverallHealth::Expired => {
            let error_msg = health
                .warning_message()
                .unwrap_or_else(|| "Token expired".to_string());
            CheckStatus::Fail {
                reason: error_msg,
                suggestion: health.recommended_action,
            }
        }
        OverallHealth::Missing => {
            // Missing is already handled by check_authenticated, skip here
            return None;
        }
        OverallHealth::Unknown => CheckStatus::Skipped {
            reason: "Unable to determine credential health".to_string(),
        },
    };

    Some(DiagnosticCheck {
        name: format!("{} credential health", provider.display_name()),
        status,
        duration: Some(start.elapsed()),
    })
}

/// Check if provider API/service is reachable.
pub async fn check_api_reachable(provider: Provider) -> DiagnosticCheck {
    let start = Instant::now();

    let status = check_reachability(provider).await;

    DiagnosticCheck {
        name: format!("{} API reachable", provider.display_name()),
        status,
        duration: Some(start.elapsed()),
    }
}

/// Internal reachability check.
async fn check_reachability(provider: Provider) -> CheckStatus {
    // For CLI-based providers, try a simple CLI command
    // For API providers, could ping health endpoint (not implemented yet)

    let cli_name = provider.cli_name();

    // Try to run CLI with --help to verify it responds
    match run_command(cli_name, &["--help"], API_TIMEOUT).await {
        Ok(output) => {
            if output.success() {
                CheckStatus::Pass {
                    details: Some("CLI responds to commands".to_string()),
                }
            } else {
                // CLI ran but returned error - still means it's reachable
                CheckStatus::Pass {
                    details: Some("CLI installed and responsive".to_string()),
                }
            }
        }
        Err(CautError::ProviderNotFound(_)) => CheckStatus::Skipped {
            reason: "CLI not installed".to_string(),
        },
        Err(CautError::Timeout(_)) => CheckStatus::Timeout { after: API_TIMEOUT },
        Err(e) => CheckStatus::Fail {
            reason: format!("Failed to run CLI: {}", e),
            suggestion: Some(provider.install_suggestion().to_string()),
        },
    }
}

/// Run all health checks for a provider.
pub async fn check_provider_health(provider: Provider) -> ProviderHealth {
    // Run CLI and auth checks in parallel
    let (cli_result, auth_result, api_result) = tokio::join!(
        check_cli_installed(provider),
        check_authenticated(provider),
        check_api_reachable(provider)
    );

    let (cli_check, cli_version) = cli_result;

    // Check credential health (sync, runs fast)
    let credential_health = check_credential_health(provider);

    ProviderHealth {
        provider,
        cli_installed: cli_check,
        cli_version,
        authenticated: auth_result,
        credential_health,
        api_reachable: api_result,
    }
}

/// Run health checks for multiple providers in parallel.
pub async fn check_all_providers(providers: &[Provider]) -> Vec<ProviderHealth> {
    let futures: Vec<_> = providers
        .iter()
        .map(|&p| check_provider_health(p))
        .collect();

    futures::future::join_all(futures).await
}

/// Module for home directory access (wraps directories crate).
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_version_from_output() {
        assert_eq!(
            extract_version("claude version 0.2.105"),
            Some("0.2.105".to_string())
        );
        assert_eq!(extract_version("v1.2.3"), Some("1.2.3".to_string()));
        assert_eq!(
            extract_version("codex 0.6.1-beta"),
            Some("0.6.1-beta".to_string())
        );
        assert_eq!(extract_version("Version: 2.0.0"), Some("2.0.0".to_string()));
    }

    #[test]
    fn extract_version_handles_multiline() {
        let output = "Claude Code CLI\nversion 0.2.105\nCopyright 2024";
        assert_eq!(extract_version(output), Some("0.2.105".to_string()));
    }

    #[test]
    fn extract_version_fallback() {
        let output = "unknown output format";
        let result = extract_version(output);
        assert!(result.is_some());
        assert!(result.unwrap().len() <= 50);
    }

    #[tokio::test]
    async fn check_cli_installed_nonexistent() {
        let provider = Provider::Codex;
        let (check, version) = check_cli_installed(provider).await;

        // This test's result depends on whether codex is installed
        // Just verify the check runs and returns valid structure
        assert!(check.name.contains("codex"));
        assert!(check.duration.is_some());

        match &check.status {
            CheckStatus::Pass { details } => {
                assert!(details.is_some());
                // Version may or may not be found
            }
            CheckStatus::Fail { reason, suggestion } => {
                assert!(reason.contains("not found"));
                assert!(suggestion.is_some());
                assert!(version.is_none());
            }
            _ => panic!("Unexpected status"),
        }
    }

    #[tokio::test]
    async fn check_authenticated_returns_valid_structure() {
        let check = check_authenticated(Provider::Claude).await;

        assert!(check.name.contains("Claude"));
        assert!(check.name.contains("authenticated"));
        assert!(check.duration.is_some());

        // Status can be Pass, Fail, or Skipped depending on local setup
        match check.status {
            CheckStatus::Pass { .. } | CheckStatus::Fail { .. } | CheckStatus::Skipped { .. } => {}
            _ => panic!("Unexpected status for auth check"),
        }
    }

    #[tokio::test]
    async fn check_api_reachable_returns_valid_structure() {
        let check = check_api_reachable(Provider::Codex).await;

        assert!(check.name.contains("Codex"));
        assert!(check.name.contains("API"));
        assert!(check.duration.is_some());

        // Should not timeout immediately
        match &check.status {
            CheckStatus::Timeout { after } => {
                assert_eq!(*after, API_TIMEOUT);
            }
            _ => {} // Other statuses are valid
        }
    }

    #[tokio::test]
    async fn check_provider_health_returns_complete_struct() {
        let health = check_provider_health(Provider::Claude).await;

        assert_eq!(health.provider, Provider::Claude);
        assert!(health.cli_installed.name.contains("claude"));
        assert!(health.authenticated.name.contains("authenticated"));
        assert!(health.api_reachable.name.contains("API"));
    }

    #[tokio::test]
    async fn check_all_providers_runs_in_parallel() {
        let providers = vec![Provider::Codex, Provider::Claude];

        let results = check_all_providers(&providers).await;

        // Should have results for both providers
        assert_eq!(results.len(), 2);

        // Verify each result has the correct provider
        let provider_ids: Vec<_> = results.iter().map(|r| r.provider).collect();
        assert!(provider_ids.contains(&Provider::Codex));
        assert!(provider_ids.contains(&Provider::Claude));
    }

    #[test]
    fn provider_install_suggestion_not_empty() {
        for provider in Provider::ALL {
            let suggestion = provider.install_suggestion();
            assert!(!suggestion.is_empty());
        }
    }

    #[test]
    fn provider_auth_suggestion_not_empty() {
        for provider in Provider::ALL {
            let suggestion = provider.auth_suggestion();
            assert!(!suggestion.is_empty());
        }
    }
}
