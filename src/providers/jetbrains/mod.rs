//! `JetBrains` AI provider.
//!
//! Ports `CodexBar`'s `JetBrainsStatusProbe` / `JetBrainsIDEDetector`: a
//! local-only probe that reads the AI Assistant quota the IDE caches in
//! `<IDE config dir>/options/AIAssistantQuotaManager2.xml`.
//!
//! - `jetbrains-local` ([`FetchKind::LocalProbe`]): picks the IDE whose
//!   quota file changed most recently (`IntelliJ IDEA`, `PyCharm`,
//!   `WebStorm`, ..., Android Studio), or the IDE config directory named by
//!   `JETBRAINS_IDE_BASE_PATH` (`CodexBar`'s "custom path" setting).
//!
//! Mapping: `primary` is the monthly credit quota (`tariffQuota`
//! used/maximum, else the totals), resetting at `nextRefill.next`;
//! identity carries the IDE (`IntelliJ IDEA 2025.3`) as organization and
//! the quota type (`Available`) as login method.

mod detector;
mod quota;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use self::detector::IdeInfo;
use self::quota::{QuotaError, QuotaSnapshot};
use crate::core::fetch_plan::{FetchContext, FetchKind, FetchPlan, FetchStrategy};
use crate::core::models::{ProviderIdentity, RateWindow, UsageSnapshot};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::providers::common;
use crate::util::time::format_countdown;

/// Strategy id for the local quota-file probe.
pub const STRATEGY_LOCAL: &str = "jetbrains-local";

/// Environment override for the IDE config directory
/// (e.g. `~/.config/JetBrains/IntelliJIdea2025.3`).
const IDE_BASE_PATH_ENV: &str = "JETBRAINS_IDE_BASE_PATH";

// =============================================================================
// Fetch Plan
// =============================================================================

/// Create the fetch plan for `JetBrains` AI.
#[must_use]
pub fn fetch_plan() -> FetchPlan {
    FetchPlan::new(
        Provider::JetBrainsAI,
        vec![FetchStrategy {
            id: STRATEGY_LOCAL,
            kind: FetchKind::LocalProbe,
            // The quota file belongs to whichever account the IDE is signed
            // into; a token account cannot select anything here. Otherwise
            // always available, so a missing IDE is reported as such.
            is_available: |ctx| ctx.account_token().is_none(),
            should_fallback: |_| false,
        }],
    )
}

/// Run one strategy from [`fetch_plan`].
///
/// # Errors
/// Returns an error when no IDE or quota file is found, when the quota file
/// cannot be parsed, or for an unknown strategy id.
pub async fn fetch(strategy_id: &str, _ctx: &FetchContext) -> Result<UsageSnapshot> {
    match strategy_id {
        STRATEGY_LOCAL => fetch_local(),
        _ => Err(super::unknown_strategy(Provider::JetBrainsAI, strategy_id)),
    }
}

// =============================================================================
// Probe
// =============================================================================

fn provider_name() -> String {
    Provider::JetBrainsAI.cli_name().to_string()
}

fn no_ide_detected() -> CautError {
    CautError::FetchFailed {
        provider: provider_name(),
        reason: "No JetBrains IDE with AI Assistant detected. \
                 Install a JetBrains IDE and enable AI Assistant."
            .to_string(),
    }
}

fn from_quota_error(err: QuotaError) -> CautError {
    match err {
        QuotaError::FileNotFound(path) => CautError::FetchFailed {
            provider: provider_name(),
            reason: format!(
                "JetBrains AI quota file not found at {}. Enable AI Assistant in your IDE.",
                path.display()
            ),
        },
        QuotaError::Parse(message) => CautError::ParseResponse(format!(
            "{}: Could not parse JetBrains AI quota: {message}",
            provider_name()
        )),
        QuotaError::NoQuotaInfo => CautError::FetchFailed {
            provider: provider_name(),
            reason: "No quota information found in the JetBrains AI configuration.".to_string(),
        },
    }
}

/// Expand a leading `~` against `home`.
fn expand_tilde(path: &str, home: Option<&Path>) -> PathBuf {
    if path == "~" {
        if let Some(home) = home {
            return home.to_path_buf();
        }
    } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
        && let Some(home) = home
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// The quota file to read and the IDE it belongs to. A custom IDE path wins
/// (with no IDE identity, as in `CodexBar`); otherwise the most recently
/// used detected IDE.
fn resolve_quota_file(
    custom_base_path: Option<&str>,
    home: Option<&Path>,
    base_paths: &[PathBuf],
) -> Result<(PathBuf, Option<IdeInfo>)> {
    if let Some(custom) = custom_base_path.map(str::trim).filter(|p| !p.is_empty()) {
        let base = expand_tilde(custom, home);
        return Ok((detector::quota_file_path(&base), None));
    }
    let ide = detector::detect_latest_ide(base_paths).ok_or_else(no_ide_detected)?;
    Ok((ide.quota_file_path.clone(), Some(ide)))
}

/// Map the quota into caut's snapshot.
fn to_usage_snapshot(
    snapshot: &QuotaSnapshot,
    ide: Option<&IdeInfo>,
    now: DateTime<Utc>,
) -> UsageSnapshot {
    let refill = snapshot.refill.as_ref();
    let resets_at = refill.and_then(|r| r.next);
    let primary = RateWindow {
        used_percent: snapshot.quota.used_percent(),
        // CodexBar leaves this empty; the refill period (e.g. `PT720H`) is
        // exactly the window length, so caut fills it in when it is fixed.
        window_minutes: refill
            .and_then(|r| r.duration.as_deref())
            .and_then(quota::duration_minutes),
        resets_at,
        reset_description: resets_at.map(format_countdown),
    };
    UsageSnapshot {
        primary: Some(primary),
        updated_at: now,
        identity: Some(ProviderIdentity {
            account_email: None,
            account_organization: ide.map(IdeInfo::display_name),
            login_method: snapshot.quota.quota_type.clone(),
        }),
        ..UsageSnapshot::empty()
    }
}

fn probe(
    custom_base_path: Option<&str>,
    home: Option<&Path>,
    base_paths: &[PathBuf],
) -> Result<UsageSnapshot> {
    let (quota_path, ide) = resolve_quota_file(custom_base_path, home, base_paths)?;
    let snapshot = quota::read_quota_file(&quota_path).map_err(from_quota_error)?;
    Ok(to_usage_snapshot(&snapshot, ide.as_ref(), Utc::now()))
}

fn fetch_local() -> Result<UsageSnapshot> {
    let custom = std::env::var(IDE_BASE_PATH_ENV).ok();
    let home = common::home_dir();
    probe(
        custom.as_deref(),
        home.as_deref(),
        &detector::default_config_base_paths(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const QUOTA_XML: &str =
        include_str!("../../../tests/fixtures/jetbrains/AIAssistantQuotaManager2.xml");

    fn write_quota(ide_dir: &Path, xml: &str) -> PathBuf {
        let options = ide_dir.join("options");
        std::fs::create_dir_all(&options).unwrap();
        let path = options.join(detector::QUOTA_FILE_NAME);
        std::fs::write(&path, xml).unwrap();
        path
    }

    #[test]
    fn fetch_plan_is_one_local_probe() {
        let plan = fetch_plan();
        assert_eq!(plan.provider, Provider::JetBrainsAI);
        assert_eq!(plan.strategies.len(), 1);
        let strategy = &plan.strategies[0];
        assert_eq!(strategy.id, STRATEGY_LOCAL);
        assert_eq!(strategy.kind, FetchKind::LocalProbe);
        assert!((strategy.is_available)(&FetchContext::default()));
        assert!(!(strategy.is_available)(&FetchContext::with_account(
            "work", "tok"
        )));
        assert!(!(strategy.should_fallback)(&no_ide_detected()));
    }

    #[tokio::test]
    async fn unknown_strategy_is_rejected() {
        let err = fetch("jetbrains.nope", &FetchContext::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Unknown strategy"));
    }

    #[test]
    fn maps_quota_refill_and_ide() {
        let snapshot = quota::parse_quota_xml(QUOTA_XML).unwrap();
        let ide = detector::parse_ide_directory("IntelliJIdea2025.3", Path::new("/test")).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 10, 0, 0, 0).unwrap();
        let usage = to_usage_snapshot(&snapshot, Some(&ide), now);

        let primary = usage.primary.unwrap();
        assert!(primary.used_percent < 1.0);
        assert!((primary.used_percent - 0.747_83).abs() < 1e-9);
        // The reset is the refill date, not the subscription's `until`.
        assert_eq!(
            primary.resets_at,
            Some(Utc.timestamp_millis_opt(1_768_572_054_939).unwrap())
        );
        assert_eq!(primary.window_minutes, Some(43_200));
        assert!(primary.reset_description.is_some());
        assert!(usage.secondary.is_none());
        assert!(usage.tertiary.is_none());
        assert!(usage.provider_cost.is_none());
        assert_eq!(usage.updated_at, now);
        let identity = usage.identity.unwrap();
        assert_eq!(
            identity.account_organization.as_deref(),
            Some("IntelliJ IDEA 2025.3")
        );
        assert_eq!(identity.login_method.as_deref(), Some("Available"));
        assert_eq!(identity.account_email, None);
    }

    #[test]
    fn maps_quota_without_refill_or_ide() {
        let xml =
            include_str!("../../../tests/fixtures/jetbrains/AIAssistantQuotaManager2_topup.xml");
        let snapshot = quota::parse_quota_xml(xml).unwrap();
        let usage = to_usage_snapshot(&snapshot, None, Utc::now());
        let primary = usage.primary.unwrap();
        assert!((primary.used_percent - 34.6).abs() < 1e-9);
        assert_eq!(primary.resets_at, None);
        assert_eq!(primary.window_minutes, None);
        assert_eq!(primary.reset_description, None);
        assert_eq!(usage.identity.unwrap().account_organization, None);
    }

    #[test]
    fn probe_reads_the_most_recently_used_ide() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("JetBrains");
        write_quota(
            &base.join("PyCharm2024.2"),
            "<application><component name=\"AIAssistantQuotaManager2\">\
             <option name=\"quotaInfo\" value=\"{&quot;type&quot;:&quot;free&quot;,&quot;current&quot;:&quot;10&quot;,&quot;maximum&quot;:&quot;100&quot;}\"/>\
             </component></application>",
        );
        let newest = write_quota(&base.join("IntelliJIdea2025.3"), QUOTA_XML);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&newest)
            .unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();

        let usage = probe(None, None, &[base]).unwrap();
        let identity = usage.identity.unwrap();
        assert_eq!(
            identity.account_organization.as_deref(),
            Some("IntelliJ IDEA 2025.3")
        );
        assert_eq!(identity.login_method.as_deref(), Some("Available"));
    }

    #[test]
    fn custom_path_wins_and_expands_tilde() {
        let home = tempfile::tempdir().unwrap();
        let ide_dir = home.path().join("custom").join("IDE");
        write_quota(
            &ide_dir,
            "<application><component name=\"AIAssistantQuotaManager2\">\
             <option name=\"quotaInfo\" value=\"{&quot;type&quot;:&quot;free&quot;,&quot;current&quot;:&quot;0&quot;,&quot;maximum&quot;:&quot;100000&quot;}\"/>\
             </component></application>",
        );
        let usage = probe(Some("  ~/custom/IDE  "), Some(home.path()), &[]).unwrap();
        assert!(usage.primary.unwrap().used_percent.abs() < f64::EPSILON);
        let identity = usage.identity.unwrap();
        assert_eq!(identity.account_organization, None);
        assert_eq!(identity.login_method.as_deref(), Some("free"));

        // A blank override falls back to detection.
        assert!(matches!(
            probe(Some("   "), Some(home.path()), &[]),
            Err(CautError::FetchFailed { reason, .. }) if reason.contains("No JetBrains IDE")
        ));
    }

    #[test]
    fn error_mapping() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nowhere");
        match probe(missing.to_str(), None, &[]) {
            Err(CautError::FetchFailed { provider, reason }) => {
                assert_eq!(provider, "jetbrains");
                assert!(reason.contains("quota file not found"));
                assert!(reason.contains("AIAssistantQuotaManager2.xml"));
            }
            other => panic!("unexpected: {other:?}"),
        }

        let empty = root.path().join("empty");
        write_quota(&empty, "<application></application>");
        assert!(matches!(
            probe(empty.to_str(), None, &[]),
            Err(CautError::FetchFailed { reason, .. }) if reason.contains("No quota information")
        ));

        let broken = root.path().join("broken");
        write_quota(
            &broken,
            "<application><component name=\"AIAssistantQuotaManager2\">\
             <option name=\"quotaInfo\" value=\"oops\"/></component></application>",
        );
        assert!(matches!(
            probe(broken.to_str(), None, &[]),
            Err(CautError::ParseResponse(msg)) if msg.starts_with("jetbrains:")
        ));
    }

    #[test]
    fn expand_tilde_variants() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde("~", Some(home)), home);
        assert_eq!(expand_tilde("~/a/b", Some(home)), home.join("a/b"));
        assert_eq!(expand_tilde("/abs", Some(home)), Path::new("/abs"));
        assert_eq!(expand_tilde("~/a", None), Path::new("~/a"));
        assert_eq!(expand_tilde("~other", Some(home)), Path::new("~other"));
    }
}
