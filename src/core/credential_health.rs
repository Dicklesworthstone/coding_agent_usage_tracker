//! Credential health monitoring for JWT and OAuth tokens.
//!
//! Provides proactive detection of expiring, expired, or invalid credentials
//! to warn users before authentication failures occur.

use std::fmt::Write;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::core::provider::Provider;

// =============================================================================
// JWT Health Types
// =============================================================================

/// Health status of a JWT token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtHealth {
    /// Token is valid with plenty of time remaining.
    Valid {
        /// Time until expiration.
        expires_in: Duration,
        /// Expiration timestamp.
        expires_at: DateTime<Utc>,
    },
    /// Token expires within 24 hours.
    ExpiringToday {
        /// Time until expiration.
        expires_in: Duration,
        /// Expiration timestamp.
        expires_at: DateTime<Utc>,
    },
    /// Token expires within 1 hour.
    ExpiringSoon {
        /// Time until expiration.
        expires_in: Duration,
        /// Expiration timestamp.
        expires_at: DateTime<Utc>,
    },
    /// Token has expired.
    Expired {
        /// When the token expired.
        expired_at: DateTime<Utc>,
    },
    /// Token has no expiration claim (never expires or unknown).
    NoExpiration,
    /// Token is malformed or cannot be decoded.
    Invalid {
        /// Reason the token is invalid.
        reason: String,
    },
}

impl JwtHealth {
    /// Returns true if the token needs attention (expiring soon, expired, or invalid).
    #[must_use]
    pub const fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::ExpiringSoon { .. } | Self::Expired { .. } | Self::Invalid { .. }
        )
    }

    /// Returns true if the token is still valid (not expired).
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(
            self,
            Self::Valid { .. }
                | Self::ExpiringToday { .. }
                | Self::ExpiringSoon { .. }
                | Self::NoExpiration
        )
    }

    /// Returns the time until expiration, if known.
    #[must_use]
    pub const fn expires_in(&self) -> Option<Duration> {
        match self {
            Self::Valid { expires_in, .. }
            | Self::ExpiringToday { expires_in, .. }
            | Self::ExpiringSoon { expires_in, .. } => Some(*expires_in),
            _ => None,
        }
    }

    /// Returns a human-readable status description.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Valid { expires_in, .. } => {
                let days = expires_in.as_secs() / 86400;
                if days > 1 {
                    format!("valid (expires in {days} days)")
                } else {
                    format!("valid (expires in {} hours)", expires_in.as_secs() / 3600)
                }
            }
            Self::ExpiringToday { expires_in, .. } => {
                let hours = expires_in.as_secs() / 3600;
                format!("expiring today (in {hours} hours)")
            }
            Self::ExpiringSoon { expires_in, .. } => {
                let mins = expires_in.as_secs() / 60;
                format!("expiring soon (in {mins} minutes)")
            }
            Self::Expired { expired_at } => {
                format!("expired at {}", expired_at.format("%Y-%m-%d %H:%M UTC"))
            }
            Self::NoExpiration => "no expiration set".to_string(),
            Self::Invalid { reason } => format!("invalid: {reason}"),
        }
    }

    /// Returns the severity level (for output formatting).
    #[must_use]
    pub const fn severity(&self) -> HealthSeverity {
        match self {
            Self::Valid { .. } | Self::NoExpiration => HealthSeverity::Ok,
            Self::ExpiringToday { .. } => HealthSeverity::Warning,
            Self::ExpiringSoon { .. } => HealthSeverity::Urgent,
            Self::Expired { .. } | Self::Invalid { .. } => HealthSeverity::Critical,
        }
    }
}

/// Severity level for health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthSeverity {
    /// Everything is fine.
    Ok,
    /// Attention needed soon.
    Warning,
    /// Immediate attention required.
    Urgent,
    /// Critical issue, action required now.
    Critical,
}

impl HealthSeverity {
    /// Returns an icon for the severity level.
    #[must_use]
    pub const fn icon(&self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Warning => "⚠",
            Self::Urgent => "⚡",
            Self::Critical => "✗",
        }
    }

    /// Returns a color name for the severity level.
    #[must_use]
    pub const fn color(&self) -> &'static str {
        match self {
            Self::Ok => "green",
            Self::Warning => "yellow",
            Self::Urgent => "orange",
            Self::Critical => "red",
        }
    }
}

// =============================================================================
// JWT Claims
// =============================================================================

/// Minimal JWT claims for expiration checking.
/// Only includes the `exp` claim which is the standard expiration timestamp.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // iat/nbf reserved for future use
struct JwtExpClaims {
    /// Expiration time (Unix timestamp in seconds).
    /// This is the standard JWT exp claim (RFC 7519).
    #[serde(default)]
    exp: Option<i64>,
    /// Issued at time (Unix timestamp in seconds).
    #[serde(default)]
    iat: Option<i64>,
    /// Not before time (Unix timestamp in seconds).
    #[serde(default)]
    nbf: Option<i64>,
}

// =============================================================================
// JWT Health Checker
// =============================================================================

/// Checker for JWT token health and expiration.
#[derive(Debug, Default)]
pub struct JwtHealthChecker;

impl JwtHealthChecker {
    /// Create a new JWT health checker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Check the health of a JWT token.
    ///
    /// This decodes the JWT payload without signature verification
    /// (we don't have the keys) and checks the `exp` claim.
    #[must_use]
    pub fn check(&self, token: &str) -> JwtHealth {
        // Decode the JWT payload
        let claims = match decode_jwt_exp_claims(token) {
            Ok(c) => c,
            Err(reason) => return JwtHealth::Invalid { reason },
        };

        // Check expiration
        claims.exp.map_or(JwtHealth::NoExpiration, |exp_timestamp| {
            let now = Utc::now().timestamp();
            let remaining_secs = exp_timestamp - now;

            if remaining_secs < 0 {
                // Token has expired
                let expired_at =
                    DateTime::from_timestamp(exp_timestamp, 0).unwrap_or_else(Utc::now);
                JwtHealth::Expired { expired_at }
            } else {
                let expires_in = Duration::from_secs(remaining_secs.unsigned_abs());
                let expires_at =
                    DateTime::from_timestamp(exp_timestamp, 0).unwrap_or_else(Utc::now);

                if remaining_secs < 3600 {
                    // Expires within 1 hour
                    JwtHealth::ExpiringSoon {
                        expires_in,
                        expires_at,
                    }
                } else if remaining_secs < 86400 {
                    // Expires within 24 hours
                    JwtHealth::ExpiringToday {
                        expires_in,
                        expires_at,
                    }
                } else {
                    // Valid with time to spare
                    JwtHealth::Valid {
                        expires_in,
                        expires_at,
                    }
                }
            }
        })
    }

    /// Check multiple tokens and return the worst health status.
    #[must_use]
    pub fn check_worst(&self, tokens: &[&str]) -> JwtHealth {
        tokens
            .iter()
            .map(|t| self.check(t))
            .max_by_key(|h| match h {
                JwtHealth::Invalid { .. } => 5,
                JwtHealth::Expired { .. } => 4,
                JwtHealth::ExpiringSoon { .. } => 3,
                JwtHealth::ExpiringToday { .. } => 2,
                JwtHealth::NoExpiration => 1,
                JwtHealth::Valid { .. } => 0,
            })
            .unwrap_or_else(|| JwtHealth::Invalid {
                reason: "no tokens provided".to_string(),
            })
    }
}

/// Decode JWT expiration claims from a token.
fn decode_jwt_exp_claims(token: &str) -> Result<JwtExpClaims, String> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "invalid JWT format: expected 3 parts, got {}",
            parts.len()
        ));
    }

    let payload = parts[1];

    // Base64url decode: replace - with + and _ with /
    let mut payload_std = payload.replace('-', "+").replace('_', "/");

    // Add padding if needed
    let padding = (4 - payload_std.len() % 4) % 4;
    for _ in 0..padding {
        payload_std.push('=');
    }

    // Decode base64
    let decoded =
        base64_decode(&payload_std).ok_or_else(|| "failed to decode base64 payload".to_string())?;

    // Parse JSON
    serde_json::from_slice::<JwtExpClaims>(&decoded)
        .map_err(|e| format!("failed to parse JWT claims: {e}"))
}

/// Simple base64 decoder (standard alphabet with padding).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let idx = ALPHABET.iter().position(|&c| c == byte)?;
        #[allow(clippy::cast_possible_truncation)] // index into 64-element table always fits u32
        let idx_u32 = idx as u32;
        buffer = (buffer << 6) | idx_u32;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            result.push(((buffer >> bits) & 0xFF) as u8);
        }
    }

    Some(result)
}

// =============================================================================
// OAuth Token Health
// =============================================================================

/// Overall health of OAuth credentials (access + refresh tokens).
#[derive(Debug, Clone)]
pub struct OAuthHealth {
    /// Health of the access token.
    pub access: JwtHealth,
    /// Health of the refresh token (if present).
    pub refresh: Option<JwtHealth>,
    /// Whether the tokens can be refreshed.
    pub can_refresh: bool,
}

impl OAuthHealth {
    /// Create OAuth health with only an access token.
    #[must_use]
    pub const fn access_only(access: JwtHealth) -> Self {
        Self {
            access,
            refresh: None,
            can_refresh: false,
        }
    }

    /// Create OAuth health with both access and refresh tokens.
    #[must_use]
    pub const fn with_refresh(access: JwtHealth, refresh: JwtHealth) -> Self {
        let can_refresh = refresh.is_valid();
        Self {
            access,
            refresh: Some(refresh),
            can_refresh,
        }
    }

    /// Returns the overall health status (worst of access/refresh).
    #[must_use]
    pub fn overall(&self) -> &JwtHealth {
        if let Some(ref refresh) = self.refresh {
            // If access is expired but refresh is valid, we can recover
            if matches!(self.access, JwtHealth::Expired { .. }) && refresh.is_valid() {
                return &self.access; // Still report access expired for visibility
            }
            // Return the more severe status
            if refresh.severity() > self.access.severity() {
                return refresh;
            }
        }
        &self.access
    }

    /// Returns true if credentials need attention.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.access.needs_attention()
            || self
                .refresh
                .as_ref()
                .is_some_and(JwtHealth::needs_attention)
    }

    /// Returns a description of the credential health.
    #[must_use]
    pub fn description(&self) -> String {
        let mut desc = format!("access token: {}", self.access.description());
        if let Some(ref refresh) = self.refresh {
            let _ = write!(desc, ", refresh token: {}", refresh.description());
        }
        if self.can_refresh {
            desc.push_str(" (can refresh)");
        }
        desc
    }
}

// =============================================================================
// Credential Health Report
// =============================================================================

/// Health report for a provider's credentials.
#[derive(Debug, Clone)]
pub struct CredentialHealthReport {
    /// Provider this report is for.
    pub provider: Provider,
    /// Type of credential.
    pub credential_type: CredentialType,
    /// Health status.
    pub health: CredentialHealth,
    /// Suggested action if attention needed.
    pub suggested_action: Option<String>,
    /// When this report was generated.
    pub checked_at: DateTime<Utc>,
}

impl CredentialHealthReport {
    /// Create a new credential health report.
    #[must_use]
    pub fn new(
        provider: Provider,
        credential_type: CredentialType,
        health: CredentialHealth,
    ) -> Self {
        let suggested_action = health.suggested_action(&provider);
        Self {
            provider,
            credential_type,
            health,
            suggested_action,
            checked_at: Utc::now(),
        }
    }

    /// Returns true if this credential needs attention.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.health.needs_attention()
    }

    /// Returns the severity level.
    #[must_use]
    pub fn severity(&self) -> HealthSeverity {
        self.health.severity()
    }
}

/// Type of credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialType {
    /// OAuth JWT tokens.
    OAuth,
    /// API key (typically no expiration).
    ApiKey,
    /// Browser cookies.
    Cookie,
    /// CLI session token.
    CliSession,
}

impl CredentialType {
    /// Returns a display name for the credential type.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::OAuth => "OAuth token",
            Self::ApiKey => "API key",
            Self::Cookie => "browser cookie",
            Self::CliSession => "CLI session",
        }
    }
}

/// Unified credential health status.
#[derive(Debug, Clone)]
pub enum CredentialHealth {
    /// OAuth tokens health.
    OAuth(OAuthHealth),
    /// JWT token health.
    Jwt(JwtHealth),
    /// API key (typically valid if present).
    ApiKeyPresent,
    /// Credential is missing.
    Missing,
    /// Unable to check credential.
    CheckFailed(String),
}

impl CredentialHealth {
    /// Returns true if attention is needed.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        match self {
            Self::OAuth(oauth) => oauth.needs_attention(),
            Self::Jwt(jwt) => jwt.needs_attention(),
            Self::ApiKeyPresent => false,
            Self::Missing | Self::CheckFailed(_) => true,
        }
    }

    /// Returns the severity level.
    #[must_use]
    pub fn severity(&self) -> HealthSeverity {
        match self {
            Self::OAuth(oauth) => oauth.overall().severity(),
            Self::Jwt(jwt) => jwt.severity(),
            Self::ApiKeyPresent => HealthSeverity::Ok,
            Self::Missing => HealthSeverity::Critical,
            Self::CheckFailed(_) => HealthSeverity::Warning,
        }
    }

    /// Returns a suggested action for the user.
    #[must_use]
    pub fn suggested_action(&self, provider: &Provider) -> Option<String> {
        match self {
            Self::OAuth(oauth) if oauth.access.needs_attention() => {
                if oauth.can_refresh {
                    Some(format!("Run: caut auth refresh {}", provider.cli_name()))
                } else {
                    Some(format!("Run: caut auth login {}", provider.cli_name()))
                }
            }
            Self::Jwt(jwt) if jwt.needs_attention() => {
                Some(format!("Run: {} auth login", provider.cli_name()))
            }
            Self::Missing => Some(format!("Run: {} auth login", provider.cli_name())),
            Self::CheckFailed(_) => Some(format!(
                "Run: caut doctor --provider {}",
                provider.cli_name()
            )),
            _ => None,
        }
    }

    /// Returns a description of the health status.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::OAuth(oauth) => oauth.description(),
            Self::Jwt(jwt) => jwt.description(),
            Self::ApiKeyPresent => "API key present".to_string(),
            Self::Missing => "credentials missing".to_string(),
            Self::CheckFailed(reason) => format!("check failed: {reason}"),
        }
    }
}

// =============================================================================
// File-based Credential Checking
// =============================================================================

/// Check OAuth credentials from a JSON auth file.
///
/// The file is expected to have `access_token`, `id_token`, or `refresh_token` fields.
#[must_use]
pub fn check_oauth_file(path: &Path) -> CredentialHealth {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return CredentialHealth::CheckFailed(format!("cannot read file: {e}")),
    };

    check_oauth_json(&content)
}

/// Check OAuth credentials from JSON content.
#[must_use]
pub fn check_oauth_json(json: &str) -> CredentialHealth {
    #[derive(Deserialize)]
    #[allow(clippy::struct_field_names)]
    struct OAuthTokens {
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default)]
        id_token: Option<String>,
        #[serde(default)]
        refresh_token: Option<String>,
    }

    // Try to parse as direct tokens
    let tokens: OAuthTokens = match serde_json::from_str(json) {
        Ok(t) => t,
        Err(e) => return CredentialHealth::CheckFailed(format!("invalid JSON: {e}")),
    };

    let checker = JwtHealthChecker::new();

    // Check access/id token (use id_token if access_token not present)
    let access_token = tokens.access_token.as_ref().or(tokens.id_token.as_ref());
    let access_health = match access_token {
        Some(token) => checker.check(token),
        None => return CredentialHealth::Missing,
    };

    // Check refresh token if present
    match tokens.refresh_token.as_ref() {
        Some(refresh_token) => {
            let refresh_health = checker.check(refresh_token);
            CredentialHealth::OAuth(OAuthHealth::with_refresh(access_health, refresh_health))
        }
        None => CredentialHealth::OAuth(OAuthHealth::access_only(access_health)),
    }
}

// =============================================================================
// Auth Health Aggregator
// =============================================================================

/// Overall health status for a provider's authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverallHealth {
    /// All credentials are healthy.
    Healthy,
    /// Some credentials are expiring soon.
    ExpiringSoon,
    /// Some credentials have expired.
    Expired,
    /// Credentials are missing.
    Missing,
    /// Unable to determine health.
    Unknown,
}

impl OverallHealth {
    /// Returns the severity level for this health status.
    #[must_use]
    pub const fn severity(&self) -> HealthSeverity {
        match self {
            Self::Healthy => HealthSeverity::Ok,
            Self::ExpiringSoon | Self::Unknown => HealthSeverity::Warning,
            Self::Expired | Self::Missing => HealthSeverity::Critical,
        }
    }

    /// Returns an icon for this health status.
    #[must_use]
    pub const fn icon(&self) -> &'static str {
        self.severity().icon()
    }
}

/// Health status for a specific credential source.
#[derive(Debug, Clone)]
pub struct SourceHealth {
    /// Type of credential source.
    pub source_type: String,
    /// Health of the source.
    pub health: CredentialHealth,
}

impl SourceHealth {
    /// Returns true if the source is expired.
    #[must_use]
    pub const fn is_expired(&self) -> bool {
        match &self.health {
            CredentialHealth::OAuth(oauth) => matches!(oauth.access, JwtHealth::Expired { .. }),
            CredentialHealth::Jwt(jwt) => matches!(jwt, JwtHealth::Expired { .. }),
            CredentialHealth::Missing => true,
            _ => false,
        }
    }

    /// Returns true if the source is expiring soon.
    #[must_use]
    pub const fn is_expiring_soon(&self) -> bool {
        match &self.health {
            CredentialHealth::OAuth(oauth) => {
                matches!(
                    oauth.access,
                    JwtHealth::ExpiringSoon { .. } | JwtHealth::ExpiringToday { .. }
                )
            }
            CredentialHealth::Jwt(jwt) => {
                matches!(
                    jwt,
                    JwtHealth::ExpiringSoon { .. } | JwtHealth::ExpiringToday { .. }
                )
            }
            _ => false,
        }
    }

    /// Returns true if the source is valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        match &self.health {
            CredentialHealth::OAuth(oauth) => oauth.overall().is_valid(),
            CredentialHealth::Jwt(jwt) => jwt.is_valid(),
            CredentialHealth::ApiKeyPresent => true,
            _ => false,
        }
    }
}

/// Authentication health for a provider.
#[derive(Debug, Clone)]
pub struct ProviderAuthHealth {
    /// Provider this health report is for.
    pub provider: Provider,
    /// Overall health status.
    pub overall: OverallHealth,
    /// Health of each credential source.
    pub sources: Vec<SourceHealth>,
    /// Recommended action to fix issues.
    pub recommended_action: Option<String>,
}

impl ProviderAuthHealth {
    /// Returns true if any credentials need attention.
    #[must_use]
    pub const fn needs_attention(&self) -> bool {
        !matches!(self.overall, OverallHealth::Healthy)
    }

    /// Returns a human-readable warning message if attention is needed.
    #[must_use]
    pub fn warning_message(&self) -> Option<String> {
        match self.overall {
            OverallHealth::Expired => Some(format!(
                "Auth expired! {}",
                self.recommended_action
                    .as_deref()
                    .unwrap_or("Re-authenticate")
            )),
            OverallHealth::ExpiringSoon => {
                // Find the source that's expiring and get its time
                let expiring_desc = self
                    .sources
                    .iter()
                    .find(|s| s.is_expiring_soon())
                    .map_or_else(|| "expiring soon".to_string(), |s| s.health.description());
                Some(format!("Auth {expiring_desc}. Consider re-authenticating."))
            }
            OverallHealth::Missing => Some(format!(
                "Auth missing! {}",
                self.recommended_action
                    .as_deref()
                    .unwrap_or("Please authenticate")
            )),
            OverallHealth::Unknown => Some("Auth status unknown".to_string()),
            OverallHealth::Healthy => None,
        }
    }
}

/// Aggregates authentication health across credential sources.
#[derive(Debug, Default)]
pub struct AuthHealthAggregator {
    #[expect(dead_code)]
    jwt_checker: JwtHealthChecker,
}

impl AuthHealthAggregator {
    /// Create a new auth health aggregator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            jwt_checker: JwtHealthChecker::new(),
        }
    }

    /// Check the authentication health for a provider.
    #[must_use]
    pub fn check_provider(&self, provider: Provider) -> ProviderAuthHealth {
        let sources = self.find_and_check_sources(provider);
        let overall = self.aggregate_health(&sources);
        let recommended_action = if overall == OverallHealth::Healthy {
            None
        } else {
            Some(provider.auth_suggestion().to_string())
        };

        ProviderAuthHealth {
            provider,
            overall,
            sources,
            recommended_action,
        }
    }

    /// Find and check all credential sources for a provider.
    #[allow(clippy::unused_self)]
    fn find_and_check_sources(&self, provider: Provider) -> Vec<SourceHealth> {
        let mut sources = Vec::new();

        // Check the primary credentials file if it exists
        if let Some(cred_path) = provider.credentials_path()
            && let Some(home) = dirs::home_dir()
        {
            let full_path = home.join(cred_path);
            if full_path.exists() {
                let health = check_oauth_file(&full_path);
                sources.push(SourceHealth {
                    source_type: "oauth".to_string(),
                    health,
                });
            }
        }

        // If no credentials found, mark as missing
        if sources.is_empty() {
            sources.push(SourceHealth {
                source_type: "credentials".to_string(),
                health: CredentialHealth::Missing,
            });
        }

        sources
    }

    /// Aggregate health from all sources into overall status.
    #[allow(clippy::unused_self)]
    fn aggregate_health(&self, sources: &[SourceHealth]) -> OverallHealth {
        // Empty sources means missing
        if sources.is_empty() {
            return OverallHealth::Missing;
        }

        // Check for worst status (worst wins)
        let has_missing = sources
            .iter()
            .any(|s| matches!(s.health, CredentialHealth::Missing));
        let has_expired = sources.iter().any(SourceHealth::is_expired);
        let has_expiring = sources.iter().any(SourceHealth::is_expiring_soon);
        let all_valid = sources.iter().all(SourceHealth::is_valid);
        if has_missing {
            OverallHealth::Missing
        } else if has_expired {
            OverallHealth::Expired
        } else if has_expiring {
            OverallHealth::ExpiringSoon
        } else if all_valid {
            OverallHealth::Healthy
        } else {
            OverallHealth::Unknown
        }
    }
}

/// Get re-authentication instructions for a provider and source type.
#[must_use]
pub fn get_reauth_instructions(provider: &Provider, source: &str) -> String {
    match (provider.cli_name(), source) {
        ("claude", "oauth" | _) => "Run: claude auth login".to_string(),
        ("codex", "oauth" | _) => "Run: codex auth login".to_string(),
        ("gemini", "oauth" | _) => "Run: gemini auth login".to_string(),
        ("cursor", _) => "Open Cursor and sign in".to_string(),
        ("copilot", _) => "Sign in with your GitHub account".to_string(),
        ("vertexai", _) => "Run: gcloud auth application-default login".to_string(),
        ("jetbrains", _) => "Configure in IDE Settings > AI Assistant".to_string(),
        _ => provider.auth_suggestion().to_string(),
    }
}

/// Helper to get home directory (wrapper for dirs crate).
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        // Use directories crate which is already a dependency
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Helper for creating test JWTs
    // =========================================================================

    fn base64_url_encode(input: &str) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut result = String::new();
        let bytes = input.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            let b0 = u32::from(bytes[i]);
            let b1 = bytes.get(i + 1).map_or(0, |&b| u32::from(b));
            let b2 = bytes.get(i + 2).map_or(0, |&b| u32::from(b));

            let triple = (b0 << 16) | (b1 << 8) | b2;

            result.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
            result.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);

            if i + 1 < bytes.len() {
                result.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
            }
            if i + 2 < bytes.len() {
                result.push(ALPHABET[(triple & 0x3F) as usize] as char);
            }

            i += 3;
        }

        // Convert to base64url (no padding, URL-safe chars)
        result
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_string()
    }

    fn make_test_jwt(claims_json: &str) -> String {
        let header = base64_url_encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload = base64_url_encode(claims_json);
        format!("{header}.{payload}.sig")
    }

    fn make_jwt_with_exp(exp_secs_from_now: i64) -> String {
        let exp = Utc::now().timestamp() + exp_secs_from_now;
        make_test_jwt(&format!(r#"{{"exp":{exp}}}"#))
    }

    // =========================================================================
    // JwtHealth tests
    // =========================================================================

    #[test]
    fn jwt_health_valid_token() {
        let checker = JwtHealthChecker::new();
        let token = make_jwt_with_exp(86400 * 7); // 7 days

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::Valid { .. }));
        assert!(health.is_valid());
        assert!(!health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Ok);
    }

    #[test]
    fn jwt_health_expiring_today() {
        let checker = JwtHealthChecker::new();
        let token = make_jwt_with_exp(3600 * 12); // 12 hours

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::ExpiringToday { .. }));
        assert!(health.is_valid());
        assert!(!health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Warning);
    }

    #[test]
    fn jwt_health_expiring_soon() {
        let checker = JwtHealthChecker::new();
        let token = make_jwt_with_exp(1800); // 30 minutes

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::ExpiringSoon { .. }));
        assert!(health.is_valid());
        assert!(health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Urgent);
    }

    #[test]
    fn jwt_health_expired() {
        let checker = JwtHealthChecker::new();
        let token = make_jwt_with_exp(-3600); // 1 hour ago

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::Expired { .. }));
        assert!(!health.is_valid());
        assert!(health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Critical);
    }

    #[test]
    fn jwt_health_no_expiration() {
        let checker = JwtHealthChecker::new();
        let token = make_test_jwt(r#"{"sub":"user123"}"#);

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::NoExpiration));
        assert!(health.is_valid());
        assert!(!health.needs_attention());
    }

    #[test]
    fn jwt_health_invalid_format() {
        let checker = JwtHealthChecker::new();

        let health = checker.check("not-a-jwt");
        assert!(matches!(health, JwtHealth::Invalid { .. }));
        assert!(!health.is_valid());
        assert!(health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Critical);
    }

    #[test]
    fn jwt_health_too_few_parts() {
        let checker = JwtHealthChecker::new();

        assert!(matches!(
            checker.check("only.two"),
            JwtHealth::Invalid { .. }
        ));
        assert!(matches!(checker.check("one"), JwtHealth::Invalid { .. }));
    }

    #[test]
    fn jwt_health_invalid_base64() {
        let checker = JwtHealthChecker::new();
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let token = format!("{header}.!!!invalid!!!.sig");

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::Invalid { .. }));
    }

    #[test]
    fn jwt_health_invalid_json() {
        let checker = JwtHealthChecker::new();
        let header = base64_url_encode(r#"{"alg":"none"}"#);
        let payload = base64_url_encode("not valid json {{{");
        let token = format!("{header}.{payload}.sig");

        let health = checker.check(&token);
        assert!(matches!(health, JwtHealth::Invalid { .. }));
    }

    // =========================================================================
    // JwtHealthChecker::check_worst tests
    // =========================================================================

    #[test]
    fn check_worst_returns_most_severe() {
        let checker = JwtHealthChecker::new();

        let valid = make_jwt_with_exp(86400 * 7);
        let expiring = make_jwt_with_exp(1800);
        let expired = make_jwt_with_exp(-3600);

        // Expired is worst
        let health = checker.check_worst(&[&valid, &expiring, &expired]);
        assert!(matches!(health, JwtHealth::Expired { .. }));

        // Expiring soon is worst when no expired
        let health = checker.check_worst(&[&valid, &expiring]);
        assert!(matches!(health, JwtHealth::ExpiringSoon { .. }));
    }

    #[test]
    fn check_worst_empty_list() {
        let checker = JwtHealthChecker::new();

        let health = checker.check_worst(&[]);
        assert!(matches!(health, JwtHealth::Invalid { .. }));
    }

    // =========================================================================
    // OAuthHealth tests
    // =========================================================================

    #[test]
    fn oauth_health_access_only() {
        let checker = JwtHealthChecker::new();
        let token = make_jwt_with_exp(86400);

        let access = checker.check(&token);
        let oauth = OAuthHealth::access_only(access);

        assert!(!oauth.needs_attention());
        assert!(!oauth.can_refresh);
        assert!(oauth.refresh.is_none());
    }

    #[test]
    fn oauth_health_with_refresh() {
        let checker = JwtHealthChecker::new();
        let access_token = make_jwt_with_exp(-3600); // expired
        let refresh_token = make_jwt_with_exp(86400 * 30); // 30 days

        let access = checker.check(&access_token);
        let refresh = checker.check(&refresh_token);
        let oauth = OAuthHealth::with_refresh(access, refresh);

        assert!(oauth.needs_attention()); // access expired
        assert!(oauth.can_refresh); // but can refresh
    }

    // =========================================================================
    // CredentialHealth tests
    // =========================================================================

    #[test]
    fn credential_health_missing() {
        let health = CredentialHealth::Missing;
        assert!(health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Critical);
        assert!(health.suggested_action(&Provider::Claude).is_some());
    }

    #[test]
    fn credential_health_api_key_present() {
        let health = CredentialHealth::ApiKeyPresent;
        assert!(!health.needs_attention());
        assert_eq!(health.severity(), HealthSeverity::Ok);
        assert!(health.suggested_action(&Provider::Claude).is_none());
    }

    // =========================================================================
    // check_oauth_json tests
    // =========================================================================

    #[test]
    fn check_oauth_json_valid_tokens() {
        let access = make_jwt_with_exp(86400);
        let refresh = make_jwt_with_exp(86400 * 30);
        let json = format!(r#"{{"access_token":"{access}","refresh_token":"{refresh}"}}"#);

        let health = check_oauth_json(&json);
        assert!(matches!(health, CredentialHealth::OAuth(_)));
        assert!(!health.needs_attention());
    }

    #[test]
    fn check_oauth_json_id_token_fallback() {
        let id_token = make_jwt_with_exp(86400);
        let json = format!(r#"{{"id_token":"{id_token}"}}"#);

        let health = check_oauth_json(&json);
        assert!(matches!(health, CredentialHealth::OAuth(_)));
    }

    #[test]
    fn check_oauth_json_no_tokens() {
        let json = r#"{"other_field":"value"}"#;

        let health = check_oauth_json(json);
        assert!(matches!(health, CredentialHealth::Missing));
    }

    #[test]
    fn check_oauth_json_invalid_json() {
        let health = check_oauth_json("not json {{{");
        assert!(matches!(health, CredentialHealth::CheckFailed(_)));
    }

    // =========================================================================
    // Description and severity tests
    // =========================================================================

    #[test]
    fn jwt_health_description() {
        let checker = JwtHealthChecker::new();

        let valid = checker.check(&make_jwt_with_exp(86400 * 7));
        assert!(valid.description().contains("valid"));

        let expired = checker.check(&make_jwt_with_exp(-3600));
        assert!(expired.description().contains("expired"));
    }

    #[test]
    fn health_severity_ordering() {
        assert!(HealthSeverity::Critical > HealthSeverity::Urgent);
        assert!(HealthSeverity::Urgent > HealthSeverity::Warning);
        assert!(HealthSeverity::Warning > HealthSeverity::Ok);
    }

    #[test]
    fn severity_has_icon_and_color() {
        for severity in [
            HealthSeverity::Ok,
            HealthSeverity::Warning,
            HealthSeverity::Urgent,
            HealthSeverity::Critical,
        ] {
            assert!(!severity.icon().is_empty());
            assert!(!severity.color().is_empty());
        }
    }

    // =========================================================================
    // CredentialHealthReport tests
    // =========================================================================

    #[test]
    fn credential_health_report_creation() {
        let report = CredentialHealthReport::new(
            Provider::Claude,
            CredentialType::OAuth,
            CredentialHealth::Missing,
        );

        assert_eq!(report.provider, Provider::Claude);
        assert_eq!(report.credential_type, CredentialType::OAuth);
        assert!(report.needs_attention());
        assert!(report.suggested_action.is_some());
    }

    #[test]
    fn credential_type_display_name() {
        assert_eq!(CredentialType::OAuth.display_name(), "OAuth token");
        assert_eq!(CredentialType::ApiKey.display_name(), "API key");
        assert_eq!(CredentialType::Cookie.display_name(), "browser cookie");
        assert_eq!(CredentialType::CliSession.display_name(), "CLI session");
    }

    // =========================================================================
    // OverallHealth tests
    // =========================================================================

    #[test]
    fn overall_health_severity() {
        assert_eq!(OverallHealth::Healthy.severity(), HealthSeverity::Ok);
        assert_eq!(
            OverallHealth::ExpiringSoon.severity(),
            HealthSeverity::Warning
        );
        assert_eq!(OverallHealth::Expired.severity(), HealthSeverity::Critical);
        assert_eq!(OverallHealth::Missing.severity(), HealthSeverity::Critical);
        assert_eq!(OverallHealth::Unknown.severity(), HealthSeverity::Warning);
    }

    #[test]
    fn overall_health_icon() {
        // All statuses should have a non-empty icon
        assert!(!OverallHealth::Healthy.icon().is_empty());
        assert!(!OverallHealth::Expired.icon().is_empty());
        assert!(!OverallHealth::Missing.icon().is_empty());
    }

    // =========================================================================
    // SourceHealth tests
    // =========================================================================

    #[test]
    fn source_health_is_expired() {
        let checker = JwtHealthChecker::new();

        // Expired JWT
        let expired_jwt = checker.check(&make_jwt_with_exp(-3600));
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(expired_jwt),
        };
        assert!(source.is_expired());

        // Valid JWT
        let valid_jwt = checker.check(&make_jwt_with_exp(86400));
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(valid_jwt),
        };
        assert!(!source.is_expired());

        // Missing counts as expired
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Missing,
        };
        assert!(source.is_expired());
    }

    #[test]
    fn source_health_is_expiring_soon() {
        let checker = JwtHealthChecker::new();

        // Expiring soon
        let expiring_jwt = checker.check(&make_jwt_with_exp(1800)); // 30 min
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(expiring_jwt),
        };
        assert!(source.is_expiring_soon());

        // Valid for days
        let valid_jwt = checker.check(&make_jwt_with_exp(86400 * 7));
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(valid_jwt),
        };
        assert!(!source.is_expiring_soon());
    }

    #[test]
    fn source_health_is_valid() {
        let checker = JwtHealthChecker::new();

        // Valid JWT
        let valid_jwt = checker.check(&make_jwt_with_exp(86400));
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(valid_jwt),
        };
        assert!(source.is_valid());

        // API key present
        let source = SourceHealth {
            source_type: "api_key".to_string(),
            health: CredentialHealth::ApiKeyPresent,
        };
        assert!(source.is_valid());

        // Missing is not valid
        let source = SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Missing,
        };
        assert!(!source.is_valid());
    }

    // =========================================================================
    // ProviderAuthHealth tests
    // =========================================================================

    #[test]
    fn provider_auth_health_needs_attention() {
        // Healthy doesn't need attention
        let health = ProviderAuthHealth {
            provider: Provider::Claude,
            overall: OverallHealth::Healthy,
            sources: vec![],
            recommended_action: None,
        };
        assert!(!health.needs_attention());

        // Expired needs attention
        let health = ProviderAuthHealth {
            provider: Provider::Claude,
            overall: OverallHealth::Expired,
            sources: vec![],
            recommended_action: Some("Re-auth".to_string()),
        };
        assert!(health.needs_attention());
    }

    #[test]
    fn provider_auth_health_warning_message() {
        // Healthy has no warning
        let health = ProviderAuthHealth {
            provider: Provider::Claude,
            overall: OverallHealth::Healthy,
            sources: vec![],
            recommended_action: None,
        };
        assert!(health.warning_message().is_none());

        // Expired has warning
        let health = ProviderAuthHealth {
            provider: Provider::Claude,
            overall: OverallHealth::Expired,
            sources: vec![],
            recommended_action: Some("Run: claude auth login".to_string()),
        };
        let msg = health.warning_message().unwrap();
        assert!(msg.contains("expired") || msg.contains("Expired"));
        assert!(msg.contains("claude auth login"));

        // Missing has warning
        let health = ProviderAuthHealth {
            provider: Provider::Codex,
            overall: OverallHealth::Missing,
            sources: vec![],
            recommended_action: Some("Run: codex auth login".to_string()),
        };
        let msg = health.warning_message().unwrap();
        assert!(msg.contains("missing") || msg.contains("Missing"));
    }

    // =========================================================================
    // AuthHealthAggregator tests
    // =========================================================================

    #[test]
    fn auth_health_aggregator_new() {
        let aggregator = AuthHealthAggregator::new();
        // Just verify it can be created
        let _ = aggregator;
    }

    #[test]
    fn auth_health_aggregator_aggregate_missing() {
        let aggregator = AuthHealthAggregator::new();

        // Empty sources = missing
        let sources: Vec<SourceHealth> = vec![];
        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::Missing);

        // Source with missing credential
        let sources = vec![SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Missing,
        }];
        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::Missing);
    }

    #[test]
    fn auth_health_aggregator_aggregate_healthy() {
        let aggregator = AuthHealthAggregator::new();
        let checker = JwtHealthChecker::new();

        let valid_jwt = checker.check(&make_jwt_with_exp(86400 * 7));
        let sources = vec![SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(valid_jwt),
        }];

        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::Healthy);
    }

    #[test]
    fn auth_health_aggregator_aggregate_expiring() {
        let aggregator = AuthHealthAggregator::new();
        let checker = JwtHealthChecker::new();

        let expiring_jwt = checker.check(&make_jwt_with_exp(1800)); // 30 min
        let sources = vec![SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(expiring_jwt),
        }];

        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::ExpiringSoon);
    }

    #[test]
    fn auth_health_aggregator_aggregate_expired() {
        let aggregator = AuthHealthAggregator::new();
        let checker = JwtHealthChecker::new();

        let expired_jwt = checker.check(&make_jwt_with_exp(-3600));
        let sources = vec![SourceHealth {
            source_type: "oauth".to_string(),
            health: CredentialHealth::Jwt(expired_jwt),
        }];

        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::Expired);
    }

    #[test]
    fn auth_health_aggregator_worst_wins() {
        let aggregator = AuthHealthAggregator::new();
        let checker = JwtHealthChecker::new();

        // Mix of valid and expired - expired wins
        let valid_jwt = checker.check(&make_jwt_with_exp(86400 * 7));
        let expired_jwt = checker.check(&make_jwt_with_exp(-3600));

        let sources = vec![
            SourceHealth {
                source_type: "oauth".to_string(),
                health: CredentialHealth::Jwt(valid_jwt),
            },
            SourceHealth {
                source_type: "cookie".to_string(),
                health: CredentialHealth::Jwt(expired_jwt),
            },
        ];

        let health = aggregator.aggregate_health(&sources);
        assert_eq!(health, OverallHealth::Expired);
    }

    // =========================================================================
    // get_reauth_instructions tests
    // =========================================================================

    #[test]
    fn reauth_instructions_per_provider() {
        let claude_instr = get_reauth_instructions(&Provider::Claude, "oauth");
        assert!(claude_instr.contains("claude"));

        let codex_instr = get_reauth_instructions(&Provider::Codex, "oauth");
        assert!(codex_instr.contains("codex"));

        let cursor_instr = get_reauth_instructions(&Provider::Cursor, "any");
        assert!(cursor_instr.contains("Cursor"));
    }
}
