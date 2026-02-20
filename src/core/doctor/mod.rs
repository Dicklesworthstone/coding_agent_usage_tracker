//! Doctor command diagnostic framework.
//!
//! Defines the core data structures for health checks and reporting.

pub mod checks;

use crate::core::provider::Provider;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Result of a single diagnostic check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CheckStatus {
    /// Check passed with optional details.
    Pass { details: Option<String> },
    /// Check passed but with a warning (e.g., credentials expiring soon).
    Warning {
        details: String,
        suggestion: Option<String>,
    },
    /// Check failed with reason and optional fix suggestion.
    Fail {
        reason: String,
        suggestion: Option<String>,
    },
    /// Check was skipped (e.g., not applicable on this platform).
    Skipped { reason: String },
    /// Check timed out.
    Timeout { after: Duration },
}

impl CheckStatus {
    /// Whether this status indicates the check is ready (functional).
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(
            self,
            Self::Pass { .. } | Self::Warning { .. } | Self::Skipped { .. }
        )
    }

    /// Whether this status requires attention (warning or worse).
    #[must_use]
    pub const fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::Warning { .. } | Self::Fail { .. } | Self::Timeout { .. }
        )
    }

    /// Whether this status is a warning (working but needs attention soon).
    #[must_use]
    pub const fn is_warning(&self) -> bool {
        matches!(self, Self::Warning { .. })
    }
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass { details } => {
                if let Some(details) = details {
                    write!(f, "pass ({details})")
                } else {
                    write!(f, "pass")
                }
            }
            Self::Warning {
                details,
                suggestion,
            } => {
                if let Some(suggestion) = suggestion {
                    write!(f, "warning: {details} (suggestion: {suggestion})")
                } else {
                    write!(f, "warning: {details}")
                }
            }
            Self::Fail { reason, suggestion } => {
                if let Some(suggestion) = suggestion {
                    write!(f, "fail: {reason} (suggestion: {suggestion})")
                } else {
                    write!(f, "fail: {reason}")
                }
            }
            Self::Skipped { reason } => write!(f, "skipped: {reason}"),
            Self::Timeout { after } => write!(f, "timeout after {}s", after.as_secs()),
        }
    }
}

/// A single diagnostic check result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: CheckStatus,
    pub duration: Option<Duration>,
}

impl DiagnosticCheck {
    /// Create a new diagnostic check.
    #[must_use]
    pub fn new(name: impl Into<String>, status: CheckStatus) -> Self {
        Self {
            name: name.into(),
            status,
            duration: None,
        }
    }

    /// Set duration for the check.
    #[must_use]
    pub const fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

/// Health status for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub provider: Provider,
    pub cli_installed: DiagnosticCheck,
    pub cli_version: Option<String>,
    pub authenticated: DiagnosticCheck,
    /// Credential health check (token expiration, etc.).
    pub credential_health: Option<DiagnosticCheck>,
    pub api_reachable: DiagnosticCheck,
}

impl ProviderHealth {
    /// Whether all checks for this provider are ready.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.cli_installed.status.is_ready()
            && self.authenticated.status.is_ready()
            && self
                .credential_health
                .as_ref()
                .is_none_or(|c| c.status.is_ready())
            && self.api_reachable.status.is_ready()
    }

    /// Whether any checks need attention (warning or worse).
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.cli_installed.status.needs_attention()
            || self.authenticated.status.needs_attention()
            || self
                .credential_health
                .as_ref()
                .is_some_and(|c| c.status.needs_attention())
            || self.api_reachable.status.needs_attention()
    }
}

/// Complete diagnostic report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub caut_version: String,
    pub caut_git_sha: String,
    pub config_status: DiagnosticCheck,
    pub providers: Vec<ProviderHealth>,
    pub total_duration: Duration,
}

impl DoctorReport {
    /// Returns (`ready_count`, `needs_attention_count`).
    ///
    /// Counts providers as ready only when all checks are pass/skip.
    /// Adds one needs-attention entry if `config_status` is not ready.
    #[must_use]
    pub fn summary(&self) -> (usize, usize) {
        let mut ready = 0;
        let mut needs_attention = 0;

        if self.config_status.status.needs_attention() {
            needs_attention += 1;
        }

        for provider in &self.providers {
            if provider.is_ready() {
                ready += 1;
            } else {
                needs_attention += 1;
            }
        }

        (ready, needs_attention)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::Provider;

    #[test]
    fn check_status_display_formats() {
        let pass = CheckStatus::Pass { details: None };
        let fail = CheckStatus::Fail {
            reason: "missing token".to_string(),
            suggestion: Some("run caut auth login".to_string()),
        };
        let skipped = CheckStatus::Skipped {
            reason: "unsupported platform".to_string(),
        };
        let timeout = CheckStatus::Timeout {
            after: Duration::from_secs(5),
        };

        assert!(pass.to_string().starts_with("pass"));
        assert!(fail.to_string().contains("missing token"));
        assert!(skipped.to_string().contains("unsupported platform"));
        assert!(timeout.to_string().contains("timeout after 5s"));
    }

    #[test]
    fn doctor_report_summary_counts() {
        let ok = DiagnosticCheck::new("ok", CheckStatus::Pass { details: None });
        let bad = DiagnosticCheck::new(
            "bad",
            CheckStatus::Fail {
                reason: "nope".to_string(),
                suggestion: None,
            },
        );

        let provider_ok = ProviderHealth {
            provider: Provider::Codex,
            cli_installed: ok.clone(),
            cli_version: None,
            authenticated: ok.clone(),
            credential_health: None,
            api_reachable: ok.clone(),
        };

        let provider_bad = ProviderHealth {
            provider: Provider::Claude,
            cli_installed: ok.clone(),
            cli_version: None,
            authenticated: bad,
            credential_health: None,
            api_reachable: ok.clone(),
        };

        let report = DoctorReport {
            caut_version: "0.1.0".to_string(),
            caut_git_sha: "deadbeef".to_string(),
            config_status: ok,
            providers: vec![provider_ok, provider_bad],
            total_duration: Duration::from_secs(1),
        };

        assert_eq!(report.summary(), (1, 1));
    }

    #[test]
    fn doctor_report_serializes_to_json() {
        let ok = DiagnosticCheck::new("ok", CheckStatus::Pass { details: None });
        let provider = ProviderHealth {
            provider: Provider::Codex,
            cli_installed: ok.clone(),
            cli_version: Some("1.0.0".to_string()),
            authenticated: ok.clone(),
            credential_health: None,
            api_reachable: ok.clone(),
        };

        let report = DoctorReport {
            caut_version: "0.1.0".to_string(),
            caut_git_sha: "deadbeef".to_string(),
            config_status: ok,
            providers: vec![provider],
            total_duration: Duration::from_secs(1),
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"cautVersion\""));
        assert!(json.contains("\"providers\""));
    }
}
