//! Kiro usage report and its mapping into a [`UsageSnapshot`].
//!
//! Ports `KiroUsageSnapshot` (`withUsageLimits`, `toUsageSnapshot`) from
//! `CodexBar`.

use chrono::{DateTime, TimeDelta, Utc};

use super::cli::display_plan_name;
use super::limits::KiroUsageLimits;
use crate::core::models::{
    ProviderCostSnapshot, ProviderIdentity, RateWindow, ScopedWindow, UsageSnapshot,
};
use crate::util::time::format_countdown;

/// Label of the overage window, as `CodexBar` titles it.
pub const OVERAGE_LABEL: &str = "Overage";

/// `CodexBar`'s id for the overage window.
pub const OVERAGE_KIND: &str = "kiro-overage";

/// Unit label for amounts metered in Kiro credits rather than money.
pub const CREDITS_UNIT: &str = "credits";

/// Usage as reported by `kiro-cli /usage`, optionally enriched by the
/// `GetUsageLimits` API.
#[derive(Debug, Clone, PartialEq)]
pub struct KiroUsageReport {
    /// Raw plan name, e.g. `KIRO FREE` or `Q Developer Pro`.
    pub plan_name: String,
    pub account_email: Option<String>,
    pub auth_method: Option<String>,
    /// Plan credits spent, excluding bonus and overage.
    pub credits_used: f64,
    pub credits_total: f64,
    pub credits_percent: f64,
    /// False when the CLI reports a plan but withholds its credit metrics.
    pub has_usage_metrics: bool,
    pub bonus_credits_used: Option<f64>,
    pub bonus_credits_total: Option<f64>,
    pub bonus_expiry_days: Option<i64>,
    /// The CLI's `Overages:` line, e.g. `Disabled` or `Enabled billed at ...`.
    pub overages_status: Option<String>,
    pub overage_credits_used: Option<f64>,
    pub estimated_overage_cost_usd: Option<f64>,
    /// Plan and overage ceilings from `GetUsageLimits`, which the CLI report
    /// cannot express.
    pub usage_limits: Option<KiroUsageLimits>,
    pub resets_at: Option<DateTime<Utc>>,
}

impl KiroUsageReport {
    /// A plan-only report for when the CLI is not involved: the API supplies
    /// every metric through [`Self::with_usage_limits`].
    #[must_use]
    pub fn plan_only(plan_name: Option<&str>) -> Self {
        Self {
            plan_name: plan_name
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .unwrap_or("Kiro")
                .to_string(),
            account_email: None,
            auth_method: None,
            credits_used: 0.0,
            credits_total: 0.0,
            credits_percent: 0.0,
            has_usage_metrics: false,
            bonus_credits_used: None,
            bonus_credits_total: None,
            bonus_expiry_days: None,
            overages_status: None,
            overage_credits_used: None,
            estimated_overage_cost_usd: None,
            usage_limits: None,
            resets_at: None,
        }
    }

    /// Fold in the API's plan / overage ceilings.
    ///
    /// The API's plan numbers replace the CLI's only when they are
    /// unambiguous: no bonus spend folded into the total and a positive plan
    /// allowance. Otherwise the CLI's plan gauge stands, and only the overage
    /// data is taken from the API.
    #[must_use]
    pub fn with_usage_limits(self, limits: Option<KiroUsageLimits>) -> Self {
        let Some(limits) = limits else {
            return self;
        };
        let has_plan_metrics = !limits.has_unseparated_bonus && limits.plan_limit > 0.0;
        let overages_status = match limits.overage_enabled {
            Some(false) => Some("Disabled".to_string()),
            Some(true) => self.overages_status.or_else(|| Some("Enabled".to_string())),
            None => self.overages_status,
        };
        let estimated_overage_cost_usd = limits.overage_charges.or_else(|| {
            limits
                .currency_code
                .eq_ignore_ascii_case("USD")
                .then_some(self.estimated_overage_cost_usd)
                .flatten()
        });
        Self {
            credits_used: if has_plan_metrics {
                limits.plan_used
            } else {
                self.credits_used
            },
            credits_total: if has_plan_metrics {
                limits.plan_limit
            } else {
                self.credits_total
            },
            credits_percent: if has_plan_metrics {
                limits.plan_used / limits.plan_limit * 100.0
            } else {
                self.credits_percent
            },
            has_usage_metrics: self.has_usage_metrics || has_plan_metrics,
            overages_status,
            overage_credits_used: Some(limits.overage_used),
            estimated_overage_cost_usd,
            resets_at: Some(limits.resets_at),
            usage_limits: Some(limits),
            ..self
        }
    }

    /// Whether overage spend counts. The API states it outright (and needs a
    /// cap to be usable); the CLI's status line is only consulted when the
    /// API is unavailable, since the CLI omits overage entirely for
    /// organization accounts.
    #[must_use]
    pub fn overages_enabled(&self) -> bool {
        let overage_cap = self.usage_limits.as_ref().and_then(|l| l.overage_cap);
        self.usage_limits
            .as_ref()
            .and_then(|l| l.overage_enabled)
            .map_or_else(
                || {
                    self.overages_status
                        .as_deref()
                        .is_some_and(|s| s.trim().to_lowercase().starts_with("enabled"))
                },
                |enabled| enabled && overage_cap.is_some(),
            )
    }

    /// Map into caut's snapshot.
    ///
    /// - `primary`: monthly plan credits (omitted when the CLI withheld them
    ///   and the API could not supply them).
    /// - `secondary`: bonus credits, expiring `bonus_expiry_days` from `now`.
    /// - `scoped["Overage"]`: overage credits spent against the API's cap.
    /// - `provider_cost`: overage charges against `cap × rate` (API), else
    ///   the CLI's estimated overage cost or overage credits with no cap.
    /// - identity: whoami email; the display plan name as login method.
    #[must_use]
    pub fn to_usage_snapshot(&self, now: DateTime<Utc>) -> UsageSnapshot {
        let primary = self.has_usage_metrics.then(|| RateWindow {
            used_percent: self.credits_percent,
            window_minutes: None,
            resets_at: self.resets_at,
            reset_description: self.resets_at.map(format_countdown),
        });

        let secondary = match (self.bonus_credits_used, self.bonus_credits_total) {
            (Some(used), Some(total)) if total > 0.0 => Some(RateWindow {
                used_percent: used / total * 100.0,
                window_minutes: None,
                resets_at: self
                    .bonus_expiry_days
                    .and_then(TimeDelta::try_days)
                    .and_then(|d| now.checked_add_signed(d)),
                reset_description: self.bonus_expiry_days.map(|d| format!("expires in {d}d")),
            }),
            _ => None,
        };

        // Overage is spendable headroom above the plan with its own ceiling,
        // so it is a window of its own; `credits_used` already excludes it.
        let scoped = self
            .usage_limits
            .as_ref()
            .and_then(|limits| {
                let cap = limits.overage_cap.filter(|c| *c > 0.0)?;
                Some(ScopedWindow {
                    label: OVERAGE_LABEL.to_string(),
                    kind: Some(OVERAGE_KIND.to_string()),
                    severity: None,
                    is_active: false,
                    window: RateWindow {
                        used_percent: (limits.overage_used / cap * 100.0).min(100.0),
                        window_minutes: None,
                        resets_at: Some(limits.resets_at),
                        reset_description: Some(format_countdown(limits.resets_at)),
                    },
                })
            })
            .into_iter()
            .collect();

        UsageSnapshot {
            primary,
            secondary,
            scoped,
            provider_cost: self.provider_cost(now),
            updated_at: now,
            identity: Some(ProviderIdentity {
                account_email: self.account_email.clone(),
                account_organization: None,
                login_method: Some(display_plan_name(&self.plan_name)),
            }),
            ..UsageSnapshot::empty()
        }
    }

    /// Overage spend. `CodexBar` charts the API's charges against their
    /// ceiling; its "Overage cost" / "Overage usage" detail rows, which caut
    /// has no slot for, become an uncapped cost when the API has no charges.
    fn provider_cost(&self, now: DateTime<Utc>) -> Option<ProviderCostSnapshot> {
        let limits = self.usage_limits.as_ref();
        let resets_at = limits.map(|l| l.resets_at);
        if let Some(limits) = limits
            && let (Some(charges), Some(charge_limit)) =
                (limits.overage_charges, limits.overage_charge_limit())
        {
            return Some(ProviderCostSnapshot {
                used: charges,
                limit: charge_limit,
                currency_code: limits.currency_code.clone(),
                period: Some(OVERAGE_LABEL.to_string()),
                resets_at,
                updated_at: now,
            });
        }
        if !self.overages_enabled() {
            return None;
        }
        if let Some(cost) = self.estimated_overage_cost_usd {
            return Some(ProviderCostSnapshot {
                used: cost,
                limit: 0.0,
                currency_code: limits
                    .map_or_else(|| "USD".to_string(), |l| l.currency_code.clone()),
                period: Some(OVERAGE_LABEL.to_string()),
                resets_at,
                updated_at: now,
            });
        }
        self.overage_credits_used.map(|used| ProviderCostSnapshot {
            used,
            limit: limits.and_then(|l| l.overage_cap).unwrap_or(0.0),
            currency_code: CREDITS_UNIT.to_string(),
            period: Some(OVERAGE_LABEL.to_string()),
            resets_at,
            updated_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::kiro::cli::{KiroAccountInfo, parse_usage_output};
    use crate::providers::kiro::limits::parse_usage_limits;
    use chrono::TimeZone;

    const OVERAGE_RESPONSE: &str =
        include_str!("../../../tests/fixtures/kiro/get_usage_limits_overage.json");

    const NO_SCOPED: [ScopedWindow; 0] = [];

    const POWER_FULL_PLAN: &str = "Estimated Usage | resets on 2026-09-01 | KIRO POWER\n\
         Credits (10000.00 of 10000 covered in plan)\n\
         ████████████████████████████████████████ 100%\n";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap()
    }

    fn cli(output: &str) -> KiroUsageReport {
        parse_usage_output(output, &KiroAccountInfo::default(), &now()).unwrap()
    }

    fn limits(json: &str) -> KiroUsageLimits {
        parse_usage_limits(json).unwrap()
    }

    fn synthetic_limits(has_unseparated_bonus: bool, plan_limit: f64) -> KiroUsageLimits {
        KiroUsageLimits {
            plan_limit,
            plan_used: if plan_limit == 0.0 { 0.0 } else { 282.49 },
            overage_used: 0.0,
            overage_cap: None,
            overage_enabled: None,
            overage_charges: None,
            overage_rate: None,
            currency_code: "USD".to_string(),
            resets_at: Utc.timestamp_opt(1_790_812_800, 0).unwrap(),
            has_unseparated_bonus,
            subscription_title: None,
        }
    }

    fn assert_plan_only(snapshot: &UsageSnapshot) {
        assert!(snapshot.primary.is_none());
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.scoped, NO_SCOPED);
        assert!(snapshot.provider_cost.is_none());
    }

    #[test]
    fn converts_report_with_bonus() {
        let resets_at = now() + TimeDelta::days(7);
        let report = KiroUsageReport {
            plan_name: "KIRO PRO".to_string(),
            credits_used: 25.0,
            credits_total: 100.0,
            credits_percent: 25.0,
            has_usage_metrics: true,
            bonus_credits_used: Some(5.0),
            bonus_credits_total: Some(20.0),
            bonus_expiry_days: Some(14),
            resets_at: Some(resets_at),
            ..KiroUsageReport::plan_only(None)
        };
        let usage = report.to_usage_snapshot(now());
        let primary = usage.primary.unwrap();
        assert!((primary.used_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(primary.resets_at, Some(resets_at));
        let secondary = usage.secondary.unwrap();
        assert!((secondary.used_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(secondary.resets_at, Some(now() + TimeDelta::days(14)));
        assert_eq!(
            secondary.reset_description.as_deref(),
            Some("expires in 14d")
        );
        let identity = usage.identity.unwrap();
        assert_eq!(identity.login_method.as_deref(), Some("Kiro Pro"));
        assert_eq!(identity.account_organization, None);
        assert_eq!(identity.account_email, None);
        assert!(usage.provider_cost.is_none());
    }

    #[test]
    fn converts_report_without_bonus_or_with_zero_bonus_total() {
        let report = KiroUsageReport {
            credits_percent: 20.0,
            has_usage_metrics: true,
            bonus_credits_used: Some(1.0),
            bonus_credits_total: Some(0.0),
            ..KiroUsageReport::plan_only(Some("KIRO FREE"))
        };
        let usage = report.to_usage_snapshot(now());
        assert!((usage.primary.unwrap().used_percent - 20.0).abs() < f64::EPSILON);
        assert!(usage.secondary.is_none());
    }

    #[test]
    fn overage_window_and_charges_against_their_ceilings() {
        let report = cli(POWER_FULL_PLAN).with_usage_limits(Some(limits(OVERAGE_RESPONSE)));
        let usage = report.to_usage_snapshot(now());

        // The plan gauge excludes overage, or 13603.49/10000 would read 136%.
        assert!((usage.primary.as_ref().unwrap().used_percent - 100.0).abs() < 1e-9);
        assert_eq!(
            usage.primary.unwrap().resets_at,
            Some(Utc.timestamp_opt(1_788_220_800, 0).unwrap())
        );
        assert_eq!(usage.scoped.len(), 1);
        let overage = &usage.scoped[0];
        assert_eq!(overage.label, "Overage");
        assert_eq!(overage.kind.as_deref(), Some("kiro-overage"));
        assert!((overage.window.used_percent - 36.0349).abs() < 1e-4);
        let cost = usage.provider_cost.unwrap();
        assert!((cost.used - 144.139_711_109_352).abs() < 1e-9);
        assert!((cost.limit - 400.0).abs() < 1e-9);
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(cost.period.as_deref(), Some("Overage"));
        assert_eq!(report.overages_status.as_deref(), Some("Enabled"));
        assert_eq!(report.overage_credits_used, Some(3603.49));
        assert!(report.overages_enabled());
    }

    #[test]
    fn overage_window_is_capped_at_full() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageCapWithPrecision\":10000.0",
            "\"overageCapWithPrecision\":100",
        );
        let usage = cli(POWER_FULL_PLAN)
            .with_usage_limits(Some(limits(&json)))
            .to_usage_snapshot(now());
        assert!((usage.scoped[0].window.used_percent - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cli_report_stands_without_the_api() {
        let report = cli("Estimated Usage | resets on 2026-06-01 | KIRO FREE\n\
             Credits (0.17 of 50 covered in plan)\n████ 0%\nOverages: Disabled")
        .with_usage_limits(None);
        assert!((report.credits_used - 0.17).abs() < 1e-9);
        assert!((report.credits_total - 50.0).abs() < f64::EPSILON);
        let usage = report.to_usage_snapshot(now());
        assert_eq!(usage.scoped, NO_SCOPED);
        assert!(usage.provider_cost.is_none());
        assert!(usage.primary.is_some());
    }

    #[test]
    fn api_disabled_overage_wins_over_stale_cli_status() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageStatus\":\"ENABLED\"",
            "\"overageStatus\":\"DISABLED\"",
        );
        let report = cli(&format!(
            "{POWER_FULL_PLAN}Overages: Enabled billed at $0.04 per request\n"
        ))
        .with_usage_limits(Some(limits(&json)));
        assert_eq!(report.overages_status.as_deref(), Some("Disabled"));
        assert!(!report.overages_enabled());
        let usage = report.to_usage_snapshot(now());
        assert_eq!(usage.scoped, NO_SCOPED);
        assert!(usage.provider_cost.is_none());
    }

    #[test]
    fn unknown_overage_status_defers_to_cli() {
        let json = OVERAGE_RESPONSE.replace(
            "\"overageStatus\":\"ENABLED\"",
            "\"overageStatus\":\"FUTURE_STATUS\"",
        );
        let report = cli(&format!(
            "{POWER_FULL_PLAN}Overages: Enabled billed at $0.04 per request\n"
        ))
        .with_usage_limits(Some(limits(&json)));
        assert!(
            report
                .overages_status
                .as_deref()
                .unwrap()
                .starts_with("Enabled")
        );
        assert!(report.overages_enabled());
        let usage = report.to_usage_snapshot(now());
        // No cap without an ENABLED status, so no window; the API's charges
        // have no ceiling either, so the CLI-style uncapped cost applies.
        assert_eq!(usage.scoped, NO_SCOPED);
        let cost = usage.provider_cost.unwrap();
        assert!((cost.used - 144.139_711_109_352).abs() < 1e-9);
        assert!(cost.limit.abs() < f64::EPSILON);
    }

    #[test]
    fn enabled_overage_without_cap_keeps_cli_overage() {
        let json = OVERAGE_RESPONSE.replace("\"overageCapWithPrecision\":10000.0,", "");
        let limits = limits(&json);
        assert_eq!(limits.overage_enabled, None);
        let report = cli(&format!(
            "{POWER_FULL_PLAN}Overages: Enabled billed at $0.04 per request\n"
        ))
        .with_usage_limits(Some(limits));
        assert!(report.overages_enabled());
        assert_eq!(report.overage_credits_used, Some(3603.49));
        assert_eq!(report.to_usage_snapshot(now()).scoped, NO_SCOPED);
    }

    #[test]
    fn bonus_entries_keep_cli_plan_usage() {
        let json = OVERAGE_RESPONSE.replace("\"bonuses\":[]", "\"bonuses\":[{}]");
        let report = cli("Estimated Usage | resets on 2026-09-01 | KIRO POWER\n\
             Credits (40.00 of 10000 covered in plan)\n████ 0%\n\
             Bonus credits: 5.00/10 credits used\n\
             Overages: Enabled billed at $0.04 per request\n")
        .with_usage_limits(Some(limits(&json)));
        assert!((report.credits_used - 40.0).abs() < f64::EPSILON);
        assert!((report.credits_total - 10000.0).abs() < f64::EPSILON);
        assert_eq!(report.bonus_credits_used, Some(5.0));
        assert_eq!(report.overage_credits_used, Some(3603.49));
        let usage = report.to_usage_snapshot(now());
        assert!(usage.primary.unwrap().used_percent.abs() < f64::EPSILON);
        assert!((usage.secondary.unwrap().used_percent - 50.0).abs() < f64::EPSILON);
        assert_eq!(usage.scoped.len(), 1);
    }

    #[test]
    fn non_usd_api_without_charges_drops_the_cli_usd_estimate() {
        let json = OVERAGE_RESPONSE
            .replace("\"currency\":\"USD\"", "\"currency\":\"EUR\"")
            .replace("\"overageCharges\":144.139711109352,", "");
        let cli_report = cli(&format!(
            "{POWER_FULL_PLAN}\nOverages: Enabled billed at $0.04 per request\n\
             Credits used: 40.29\nEst. cost: $1.61 USD\n"
        ));
        assert_eq!(cli_report.estimated_overage_cost_usd, Some(1.61));
        let report = cli_report.with_usage_limits(Some(limits(&json)));
        assert_eq!(report.estimated_overage_cost_usd, None);
        assert_eq!(
            report
                .usage_limits
                .as_ref()
                .map(|l| l.currency_code.as_str()),
            Some("EUR")
        );
        // No charges: overage credits against the cap stand in.
        let cost = report.to_usage_snapshot(now()).provider_cost.unwrap();
        assert_eq!(cost.currency_code, "credits");
        assert!((cost.used - 3603.49).abs() < 1e-9);
        assert!((cost.limit - 10000.0).abs() < 1e-9);
    }

    #[test]
    fn cli_only_overage_cost_is_uncapped() {
        let report = cli(&format!(
            "{POWER_FULL_PLAN}\nOverages: Enabled  billed at $0.04 per request\n\
             Credits used: 40.29\nEst. cost: $1.61 USD\n"
        ));
        let cost = report.to_usage_snapshot(now()).provider_cost.unwrap();
        assert!((cost.used - 1.61).abs() < 1e-9);
        assert!(cost.limit.abs() < f64::EPSILON);
        assert_eq!(cost.currency_code, "USD");

        let credits_only = cli(&format!(
            "{POWER_FULL_PLAN}\nOverages: Enabled\nCredits used: 40.29\n"
        ));
        let cost = credits_only.to_usage_snapshot(now()).provider_cost.unwrap();
        assert_eq!(cost.currency_code, "credits");
        assert!((cost.used - 40.29).abs() < 1e-9);

        let disabled = cli(&format!(
            "{POWER_FULL_PLAN}\nOverages: Disabled\nCredits used: 40.29\nEst. cost: $1.61 USD\n"
        ));
        assert!(disabled.to_usage_snapshot(now()).provider_cost.is_none());
    }

    #[test]
    fn summary_enrichment_supplies_or_withholds_plan_gauge() {
        let summary = cli("Plan: KIRO PRO MAX | 1 usage breakdowns");
        assert_plan_only(&summary.to_usage_snapshot(now()));

        let enriched = summary
            .clone()
            .with_usage_limits(Some(synthetic_limits(false, 5000.0)));
        assert!((enriched.credits_used - 282.49).abs() < 1e-9);
        assert!((enriched.credits_total - 5000.0).abs() < f64::EPSILON);
        let primary = enriched.to_usage_snapshot(now()).primary.unwrap();
        assert!((primary.used_percent - 5.6498).abs() < 1e-4);

        // Bonus-inclusive totals or a zero allowance cannot invent a gauge.
        assert_plan_only(
            &summary
                .clone()
                .with_usage_limits(Some(synthetic_limits(true, 5000.0)))
                .to_usage_snapshot(now()),
        );
        assert_plan_only(
            &summary
                .with_usage_limits(Some(synthetic_limits(false, 0.0)))
                .to_usage_snapshot(now()),
        );
    }

    #[test]
    fn zero_api_allowance_keeps_existing_cli_metrics() {
        let known = cli("Credits (20 of 50 covered in plan)")
            .with_usage_limits(Some(synthetic_limits(false, 0.0)));
        assert!((known.credits_used - 20.0).abs() < f64::EPSILON);
        assert!((known.credits_total - 50.0).abs() < f64::EPSILON);
        let primary = known.to_usage_snapshot(now()).primary.unwrap();
        assert!((primary.used_percent - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_only_report_uses_subscription_title() {
        let report = KiroUsageReport::plan_only(Some("KIRO POWER"))
            .with_usage_limits(Some(limits(OVERAGE_RESPONSE)));
        let usage = report.to_usage_snapshot(now());
        assert!((usage.primary.unwrap().used_percent - 100.0).abs() < 1e-9);
        assert_eq!(
            usage.identity.unwrap().login_method.as_deref(),
            Some("Kiro Power")
        );
        assert_eq!(KiroUsageReport::plan_only(Some("  ")).plan_name, "Kiro");
        assert_eq!(KiroUsageReport::plan_only(None).plan_name, "Kiro");
    }
}
