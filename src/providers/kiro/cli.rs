//! Parsing of `kiro-cli` output.
//!
//! Ports `KiroStatusProbe.parse`, `parseWhoAmIOutput` and
//! `validateWhoAmIOutput` from `CodexBar`. Everything here is pure, so tests
//! feed captured CLI output straight in.

use std::sync::LazyLock;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use regex::{Captures, Regex};

use super::report::KiroUsageReport;

/// Why a `kiro-cli` probe failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The CLI reported that no one is logged in.
    NotLoggedIn,
    /// The CLI ran but failed; carries its message.
    Failed(String),
    /// The `/usage` output matched no known format.
    Parse(String),
}

/// Account details reported by `kiro-cli whoami`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KiroAccountInfo {
    /// How the account signed in, e.g. `Google` or `Builder ID`.
    pub auth_method: Option<String>,
    /// The signed-in email address.
    pub email: Option<String>,
}

// =============================================================================
// Patterns
// =============================================================================

/// A compiled pattern. Compilation cannot fail for the literals below (a test
/// checks every one), but a failure degrades to "no match" instead of a panic.
type Pattern = LazyLock<Option<Regex>>;

static ANSI: Pattern = LazyLock::new(|| Regex::new(r"\x1B\[[0-9;?]*[A-Za-z]|\x1B\].*?\x07").ok());
static INLINE_RESIDUE: Pattern = LazyLock::new(|| Regex::new(r"\x1B|\[[0-9;?]*[A-Za-z]").ok());
static WHITESPACE_RUN: Pattern = LazyLock::new(|| Regex::new(r"\s+").ok());
/// Kiro CLI 2.20 summary: `Plan: KIRO PRO MAX | 1 usage breakdowns`.
static SUMMARY_PLAN: Pattern = LazyLock::new(|| {
    Regex::new(
        r"(?mR)^[ \t]*Plan:[ \t]*([^|\r\n]+?)[ \t]*\|[ \t]*[0-9]+[ \t]+usage breakdowns?[ \t]*$",
    )
    .ok()
});
/// Legacy boxed header: `| KIRO FREE`. Horizontal whitespace only, so the
/// match cannot bridge a newline into the next line.
static LEGACY_PLAN: Pattern = LazyLock::new(|| Regex::new(r"\|[ \t]*(KIRO[ \t]+\w+)").ok());
/// kiro-cli 2.x: `Estimated Usage | resets on 2026-06-01 | KIRO FREE`.
static ESTIMATED_PLAN: Pattern =
    LazyLock::new(|| Regex::new(r"Estimated Usage[ \t]*\|[^\n|]*\|[ \t]*([A-Z][A-Z0-9 ]+)").ok());
/// kiro-cli 1.24+: `Plan: Q Developer Pro`.
static NEW_PLAN: Pattern = LazyLock::new(|| Regex::new(r"Plan:[ \t]*(.+)").ok());
static RESET_DATE: Pattern =
    LazyLock::new(|| Regex::new(r"resets on (\d{4}-\d{2}-\d{2}|\d{2}/\d{2})").ok());
static PERCENT_BAR: Pattern = LazyLock::new(|| Regex::new(r"█+\s*(\d+)%").ok());
static PLAN_CREDITS: Pattern =
    LazyLock::new(|| Regex::new(r"\((\d+\.?\d*)\s+of\s+(\d+)\s+covered").ok());
static BONUS_CREDITS: Pattern =
    LazyLock::new(|| Regex::new(r"Bonus credits:\s*(\d+\.?\d*)/(\d+)").ok());
static BONUS_EXPIRY: Pattern = LazyLock::new(|| Regex::new(r"expires in (\d+) days?").ok());
static OVERAGES: Pattern = LazyLock::new(|| Regex::new(r"(?i)Overages:\s*([^\n]+)").ok());
static OVERAGE_CREDITS: Pattern =
    LazyLock::new(|| Regex::new(r"(?i)Credits used:\s*(\d+\.?\d*)").ok());
static OVERAGE_COST: Pattern =
    LazyLock::new(|| Regex::new(r"(?i)Est\.\s*cost:\s*\$?(\d+\.?\d*)\s*USD").ok());
static WHOAMI_AUTH_PREFIX: Pattern =
    LazyLock::new(|| Regex::new(r"(?i)^\s*logged in with\s+").ok());
static WHOAMI_EMAIL_PREFIX: Pattern = LazyLock::new(|| Regex::new(r"(?i)^\s*email:\s*").ok());

fn captures<'t>(pattern: &Pattern, text: &'t str) -> Option<Captures<'t>> {
    pattern.as_ref()?.captures(text)
}

/// Capture group 1 (or the whole match for a group-less pattern), trimmed.
fn first_capture<'t>(pattern: &Pattern, text: &'t str) -> Option<&'t str> {
    let caps = captures(pattern, text)?;
    caps.get(1)
        .or_else(|| caps.get(0))
        .map(|m| m.as_str().trim())
}

fn replace_all(pattern: &Pattern, text: &str, with: &str) -> String {
    pattern.as_ref().map_or_else(
        || text.to_string(),
        |re| re.replace_all(text, with).into_owned(),
    )
}

fn parse_number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

// =============================================================================
// Shared helpers
// =============================================================================

/// Remove ANSI escape sequences (CSI and OSC) from CLI output.
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    replace_all(&ANSI, text, "")
}

/// Strip ANSI codes plus any `[1m`-style residue left by a lost escape byte.
fn clean_inline_value(text: &str) -> String {
    replace_all(&INLINE_RESIDUE, &strip_ansi(text), "")
        .trim()
        .to_string()
}

/// Whether CLI output says the user must log in first.
#[must_use]
pub fn is_login_required(output: &str) -> bool {
    contains_login_marker(&strip_ansi(output).to_lowercase())
}

fn contains_login_marker(lowered: &str) -> bool {
    [
        "not logged in",
        "login required",
        "failed to initialize auth portal",
        "kiro-cli login",
        "oauth error",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

/// Join trimmed stdout and stderr, skipping empty streams.
#[must_use]
pub fn combine_output(stdout: &str, stderr: &str) -> String {
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Human form of a plan name: `KIRO FREE` becomes `Kiro Free`; names that do
/// not mention Kiro (`Q Developer Pro`) only get whitespace cleanup.
#[must_use]
pub fn display_plan_name(plan_name: &str) -> String {
    let cleaned = replace_all(&WHITESPACE_RUN, &clean_inline_value(plan_name), " ")
        .trim()
        .to_string();
    if !cleaned.to_lowercase().contains("kiro") {
        return if cleaned.is_empty() {
            plan_name.to_string()
        } else {
            cleaned
        };
    }
    cleaned
        .split(' ')
        .map(|word| {
            if word.eq_ignore_ascii_case("kiro") {
                return "Kiro".to_string();
            }
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// =============================================================================
// whoami
// =============================================================================

/// Validate `kiro-cli whoami` output and extract the account.
///
/// # Errors
/// [`CliError::NotLoggedIn`] when the output asks for a login,
/// [`CliError::Failed`] for a non-zero exit or empty output.
pub fn validate_whoami(
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) -> Result<KiroAccountInfo, CliError> {
    let combined = combine_output(stdout, stderr);
    if is_login_required(&combined) {
        return Err(CliError::NotLoggedIn);
    }
    if exit_code != 0 {
        return Err(CliError::Failed(if combined.is_empty() {
            format!("Kiro CLI failed with status {exit_code}.")
        } else {
            combined
        }));
    }
    if combined.is_empty() {
        return Err(CliError::Failed(
            "Kiro CLI whoami returned no output.".to_string(),
        ));
    }
    Ok(parse_whoami(&combined))
}

/// Extract the sign-in method and email from `kiro-cli whoami` output.
///
/// Accepts `Logged in with Google` / `Email: a@b.c` lines and the legacy
/// output that is just a bare email address.
#[must_use]
pub fn parse_whoami(output: &str) -> KiroAccountInfo {
    let stripped = strip_ansi(output);
    let mut auth_method = None;
    let mut email = None;
    for line in stripped.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let lowered = line.to_lowercase();
        if lowered.contains("logged in with") {
            auth_method = Some(
                replace_all(&WHOAMI_AUTH_PREFIX, line, "")
                    .trim()
                    .to_string(),
            );
        } else if lowered.contains("email:") {
            email = Some(
                replace_all(&WHOAMI_EMAIL_PREFIX, line, "")
                    .trim()
                    .to_string(),
            );
        } else if email.is_none() && !line.contains(' ') && line.contains('@') {
            email = Some(line.to_string());
        }
    }
    KiroAccountInfo {
        auth_method: non_empty(auth_method.as_deref()),
        email: non_empty(email.as_deref()),
    }
}

// =============================================================================
// /usage
// =============================================================================

struct PlanName {
    name: String,
    matched_new_format: bool,
    is_summary: bool,
}

fn parse_plan_name(text: &str) -> PlanName {
    if let Some(name) = first_capture(&SUMMARY_PLAN, text).filter(|n| !n.is_empty()) {
        return PlanName {
            name: name.to_string(),
            matched_new_format: true,
            is_summary: true,
        };
    }
    let mut name = "Kiro".to_string();
    let mut matched_new_format = false;

    if let Some(m) = captures(&LEGACY_PLAN, text).and_then(|c| c.get(0)) {
        name = m.as_str().replace('|', "").trim().to_string();
    }
    if let Some(m) = captures(&ESTIMATED_PLAN, text).and_then(|c| c.get(0))
        && let Some(plan) = m.as_str().split('|').next_back().map(str::trim)
        && !plan.is_empty()
    {
        name = plan.to_string();
    }
    if let Some(m) = captures(&NEW_PLAN, text).and_then(|c| c.get(0)) {
        let line = m.as_str().replace("Plan:", "");
        if let Some(first_line) = line.lines().next() {
            name = first_line.trim().to_string();
            matched_new_format = true;
        }
    }
    PlanName {
        name,
        matched_new_format,
        is_summary: false,
    }
}

fn local_midnight<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> Option<DateTime<Utc>> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parse `2026-06-01` or `MM/DD` as local midnight. `MM/DD` means the next
/// such date: this year if still ahead of `now`, otherwise next year.
fn parse_reset_date<Tz: TimeZone>(value: &str, now: &DateTime<Tz>) -> Option<DateTime<Utc>> {
    let tz = now.timezone();
    if value.contains('-') {
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
        return local_midnight(&tz, date);
    }
    let (month, day) = value.split_once('/')?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    let year = now.year();
    let now_utc = now.with_timezone(&Utc);
    if let Some(this_year) =
        NaiveDate::from_ymd_opt(year, month, day).and_then(|d| local_midnight(&tz, d))
        && this_year > now_utc
    {
        return Some(this_year);
    }
    NaiveDate::from_ymd_opt(year + 1, month, day).and_then(|d| local_midnight(&tz, d))
}

struct BonusCredits {
    used: Option<f64>,
    total: Option<f64>,
    expiry_days: Option<i64>,
}

fn parse_bonus_credits(text: &str) -> BonusCredits {
    let (used, total) = captures(&BONUS_CREDITS, text).map_or((None, None), |caps| {
        (
            caps.get(1).and_then(|m| parse_number(m.as_str())),
            caps.get(2).and_then(|m| parse_number(m.as_str())),
        )
    });
    let expiry_days = first_capture(&BONUS_EXPIRY, text).and_then(|d| d.parse::<i64>().ok());
    BonusCredits {
        used,
        total,
        expiry_days,
    }
}

/// Parse `kiro-cli chat --no-interactive /usage` output.
///
/// `now` anchors `resets on MM/DD` dates and supplies the local time zone.
///
/// # Errors
/// [`CliError::NotLoggedIn`] when the output asks for a login, otherwise
/// [`CliError::Parse`] for empty output, a backend warning, or output that
/// matches no known usage format.
pub fn parse_usage_output<Tz: TimeZone>(
    output: &str,
    account: &KiroAccountInfo,
    now: &DateTime<Tz>,
) -> Result<KiroUsageReport, CliError> {
    let stripped = strip_ansi(output);
    if stripped.trim().is_empty() {
        return Err(CliError::Parse("Empty output from kiro-cli.".to_string()));
    }
    let lowered = stripped.to_lowercase();
    if lowered.contains("could not retrieve usage information") {
        return Err(CliError::Parse(
            "Kiro CLI could not retrieve usage information.".to_string(),
        ));
    }
    if contains_login_marker(&lowered) {
        return Err(CliError::NotLoggedIn);
    }

    let plan = parse_plan_name(&stripped);
    let is_managed_plan =
        lowered.contains("managed by admin") || lowered.contains("managed by organization");
    let resets_at =
        first_capture(&RESET_DATE, &stripped).and_then(|value| parse_reset_date(value, now));

    // "████...█ X%" bar meter.
    let (mut credits_percent, matched_percent) = first_capture(&PERCENT_BAR, &stripped)
        .map_or((0.0, false), |value| {
            (parse_number(value).unwrap_or(0.0), true)
        });

    // "(X.XX of Y covered in plan)"; 50 is the free-tier allowance.
    let (credits_used, credits_total, matched_credits) =
        captures(&PLAN_CREDITS, &stripped).map_or((0.0, 50.0, false), |caps| {
            let number = |i: usize| caps.get(i).and_then(|m| parse_number(m.as_str()));
            (number(1).unwrap_or(0.0), number(2).unwrap_or(50.0), true)
        });
    if !matched_percent && matched_credits && credits_total > 0.0 {
        credits_percent = credits_used / credits_total * 100.0;
    }

    let bonus = parse_bonus_credits(&stripped);
    let overages_status = first_capture(&OVERAGES, &stripped)
        .map(clean_inline_value)
        .filter(|s| !s.is_empty());
    let overage_credits_used = first_capture(&OVERAGE_CREDITS, &stripped).and_then(parse_number);
    let estimated_overage_cost_usd = first_capture(&OVERAGE_COST, &stripped).and_then(parse_number);

    let mut report = KiroUsageReport {
        plan_name: plan.name,
        account_email: non_empty(account.email.as_deref()),
        auth_method: non_empty(account.auth_method.as_deref()),
        credits_used,
        credits_total,
        credits_percent,
        has_usage_metrics: true,
        bonus_credits_used: bonus.used,
        bonus_credits_total: bonus.total,
        bonus_expiry_days: bonus.expiry_days,
        overages_status,
        overage_credits_used,
        estimated_overage_cost_usd,
        usage_limits: None,
        resets_at,
    };

    // A managed report or an explicit breakdown summary may withhold metrics
    // without that being a format change.
    if plan.matched_new_format
        && (is_managed_plan || plan.is_summary)
        && !matched_percent
        && !matched_credits
    {
        report.credits_used = 0.0;
        report.credits_total = 0.0;
        report.credits_percent = 0.0;
        report.has_usage_metrics = false;
        report.resets_at = None;
        return Ok(report);
    }

    // Require at least one usage pattern so a format change fails loudly
    // instead of reporting an idle account.
    if !matched_percent && !matched_credits {
        return Err(CliError::Parse(
            "No recognizable usage patterns found. Kiro CLI output format may have changed."
                .to_string(),
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap()
    }

    fn parse(output: &str) -> Result<KiroUsageReport, CliError> {
        parse_usage_output(output, &KiroAccountInfo::default(), &now())
    }

    #[test]
    fn every_pattern_compiles() {
        for pattern in [
            &ANSI,
            &INLINE_RESIDUE,
            &WHITESPACE_RUN,
            &SUMMARY_PLAN,
            &LEGACY_PLAN,
            &ESTIMATED_PLAN,
            &NEW_PLAN,
            &RESET_DATE,
            &PERCENT_BAR,
            &PLAN_CREDITS,
            &BONUS_CREDITS,
            &BONUS_EXPIRY,
            &OVERAGES,
            &OVERAGE_CREDITS,
            &OVERAGE_COST,
            &WHOAMI_AUTH_PREFIX,
            &WHOAMI_EMAIL_PREFIX,
        ] {
            assert!(pattern.is_some());
        }
    }

    #[test]
    fn parses_basic_legacy_output() {
        let output = "| KIRO FREE                                          |\n\
                      ████████████████████████████████████████████████████ 25%\n\
                      (12.50 of 50 covered in plan), resets on 01/15";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "KIRO FREE");
        assert_eq!(display_plan_name(&report.plan_name), "Kiro Free");
        assert!((report.credits_percent - 25.0).abs() < f64::EPSILON);
        assert!((report.credits_used - 12.5).abs() < f64::EPSILON);
        assert!((report.credits_total - 50.0).abs() < f64::EPSILON);
        assert!(report.has_usage_metrics);
        assert_eq!(report.bonus_credits_used, None);
        assert_eq!(report.bonus_credits_total, None);
        assert_eq!(report.bonus_expiry_days, None);
        // 01/15 is already past on 2026-05-20, so it rolls to next year.
        assert_eq!(
            report.resets_at,
            Some(Utc.with_ymd_and_hms(2027, 1, 15, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn parses_bonus_credits_with_expiry() {
        let output = "| KIRO PRO                                           |\n\
                      ████████████████████████████████████████████████████ 80%\n\
                      (40.00 of 50 covered in plan), resets on 02/01\n\
                      Bonus credits: 5.00/10 credits used, expires in 7 days";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "KIRO PRO");
        assert!((report.credits_percent - 80.0).abs() < f64::EPSILON);
        assert_eq!(report.bonus_credits_used, Some(5.0));
        assert_eq!(report.bonus_credits_total, Some(10.0));
        assert_eq!(report.bonus_expiry_days, Some(7));
    }

    #[test]
    fn percent_falls_back_to_credit_ratio() {
        let output = "| KIRO FREE |\n(12.50 of 50 covered in plan), resets on 01/15";
        let report = parse(output).unwrap();
        assert!((report.credits_percent - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn bonus_without_expiry_and_single_day() {
        let report =
            parse("| KIRO FREE |\n█████ 60%\n(30.00 of 50 covered in plan)\nBonus credits: 2.00/5 credits used")
                .unwrap();
        assert_eq!(report.bonus_credits_used, Some(2.0));
        assert_eq!(report.bonus_credits_total, Some(5.0));
        assert_eq!(report.bonus_expiry_days, None);

        let report = parse(
            "| KIRO FREE |\n█████ 10%\n(5.00 of 50 covered in plan)\n\
             Bonus credits: 2.00/5 credits used, expires in 1 day",
        )
        .unwrap();
        assert_eq!(report.bonus_expiry_days, Some(1));
    }

    #[test]
    fn strips_ansi_before_parsing() {
        let output = "\u{1b}[32m| KIRO FREE                                          |\u{1b}[0m\n\
                      \u{1b}[38;5;11m████████████████████████████████████████████████████\u{1b}[0m 50%\n\
                      (25.00 of 50 covered in plan), resets on 03/15";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "KIRO FREE");
        assert!((report.credits_percent - 50.0).abs() < f64::EPSILON);
        assert!((report.credits_used - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        assert_eq!(strip_ansi("\u{1b}[1mBold\u{1b}[0m"), "Bold");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(strip_ansi("\u{1b}[?25lhidden"), "hidden");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn header_only_output_is_a_parse_error() {
        assert!(matches!(parse("| KIRO FREE |"), Err(CliError::Parse(_))));
        assert!(matches!(
            parse("Plan: Q Developer Pro\nTip: to see context window usage, run /context"),
            Err(CliError::Parse(_))
        ));
    }

    #[test]
    fn managed_plan_without_metrics_is_plan_only() {
        let output = "Plan: Q Developer Pro\nYour plan is managed by admin\n\n\
                      Tip: to see context window usage, run /context";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "Q Developer Pro");
        assert!(!report.has_usage_metrics);
        assert!(report.credits_percent.abs() < f64::EPSILON);
        assert!(report.credits_total.abs() < f64::EPSILON);
        assert_eq!(report.bonus_credits_used, None);
        assert_eq!(report.resets_at, None);

        let report =
            parse("\u{1b}[38;5;141mPlan: Q Developer Free\u{1b}[0m\nYour plan is managed by admin")
                .unwrap();
        assert_eq!(report.plan_name, "Q Developer Free");
        assert_eq!(display_plan_name(&report.plan_name), "Q Developer Free");
    }

    #[test]
    fn managed_plan_with_metrics_keeps_them() {
        let output = "Plan: Q Developer Enterprise\nYour plan is managed by admin\n\
                      ████████████████████ 40%\n(20.00 of 50 covered in plan), resets on 03/15";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "Q Developer Enterprise");
        assert!(report.has_usage_metrics);
        assert!((report.credits_percent - 40.0).abs() < f64::EPSILON);
        assert!((report.credits_used - 20.0).abs() < f64::EPSILON);
        assert!(report.resets_at.is_some());
    }

    #[test]
    fn parses_kiro_cli_two_format() {
        let output = include_str!("../../../tests/fixtures/kiro/usage_cli_v2.txt");
        let account = KiroAccountInfo {
            auth_method: Some("Google".to_string()),
            email: Some("person@example.com".to_string()),
        };
        let report = parse_usage_output(output, &account, &now()).unwrap();
        assert_eq!(report.plan_name, "KIRO FREE");
        assert_eq!(display_plan_name(&report.plan_name), "Kiro Free");
        assert_eq!(report.account_email.as_deref(), Some("person@example.com"));
        assert_eq!(report.auth_method.as_deref(), Some("Google"));
        assert!((report.credits_used - 0.17).abs() < 1e-9);
        assert!((report.credits_total - 50.0).abs() < f64::EPSILON);
        assert!(report.credits_percent.abs() < f64::EPSILON);
        assert_eq!(report.bonus_credits_used, Some(45.53));
        assert_eq!(report.bonus_credits_total, Some(2000.0));
        assert_eq!(report.bonus_expiry_days, Some(19));
        assert_eq!(report.overages_status.as_deref(), Some("Disabled"));
        assert_eq!(
            report.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn parses_overage_credits_and_estimated_cost() {
        let output = "Estimated Usage | resets on 2026-06-01 | KIRO PRO\n\
                      Credits (1000.00 of 1000 covered in plan)\n\
                      ████████████████████████████████ 100%\n\n\
                      Overages: Enabled  billed at $0.04 per request\n\
                      Credits used: 40.29\n\
                      Est. cost: $1.61 USD\n";
        let report = parse(output).unwrap();
        assert_eq!(report.plan_name, "KIRO PRO");
        assert!((report.credits_used - 1000.0).abs() < f64::EPSILON);
        assert_eq!(
            report.overages_status.as_deref(),
            Some("Enabled  billed at $0.04 per request")
        );
        assert_eq!(report.overage_credits_used, Some(40.29));
        assert_eq!(report.estimated_overage_cost_usd, Some(1.61));
    }

    #[test]
    fn bonus_line_is_not_read_as_overage_credits() {
        let report = parse(
            "Estimated Usage | resets on 2026-06-01 | KIRO FREE\n\
             Bonus credits: 45.53/2000 credits used, expires in 19 days\n\
             Credits (0.17 of 50 covered in plan)\n████ 0%",
        )
        .unwrap();
        assert_eq!(report.overage_credits_used, None);
    }

    #[test]
    fn summary_preserves_plan_without_inventing_usage() {
        for count in ["0", "1", "2"] {
            let output =
                format!("\u{1b}[32mPlan: KIRO PRO MAX | {count} usage breakdowns\u{1b}[0m\n");
            let report = parse(&output).unwrap();
            assert_eq!(report.plan_name, "KIRO PRO MAX");
            assert!(!report.has_usage_metrics);
        }
        let report = parse("Plan: KIRO PRO MAX | 1 usage breakdown").unwrap();
        assert!(!report.has_usage_metrics);
    }

    #[test]
    fn malformed_summaries_remain_errors() {
        for output in [
            "Plan: KIRO PRO MAX",
            "Plan: KIRO PRO MAX | usage breakdowns",
            "Plan: KIRO PRO MAX | -1 usage breakdowns",
            "Plan: KIRO PRO MAX | 1.5 usage breakdowns",
            "Plan: KIRO PRO MAX | 1 usage breakdowns failed",
            "Plan: | 1 usage breakdowns",
            "echo Plan: KIRO PRO MAX | 1 usage breakdowns",
            "Plan: KIRO PRO MAX |\n1 usage breakdowns",
        ] {
            assert!(parse(output).is_err(), "accepted {output:?}");
        }
    }

    #[test]
    fn summary_with_real_zero_usage_keeps_allowance() {
        let report =
            parse("Plan: KIRO PRO MAX | 1 usage breakdowns\nCredits (0 of 5000 covered in plan)")
                .unwrap();
        assert_eq!(report.plan_name, "KIRO PRO MAX");
        assert!(report.has_usage_metrics);
        assert!((report.credits_total - 5000.0).abs() < f64::EPSILON);
        assert!(report.credits_percent.abs() < f64::EPSILON);
    }

    #[test]
    fn error_outputs() {
        assert!(matches!(parse(""), Err(CliError::Parse(_))));
        assert!(matches!(parse("  \n\t"), Err(CliError::Parse(_))));
        let warning = "\u{1b}[38;5;11m⚠️  Warning: Could not retrieve usage information from backend\n\
                       \u{1b}[38;5;8mError: dispatch failure (io error): an i/o error occurred";
        assert!(matches!(parse(warning), Err(CliError::Parse(_))));
        match parse("Welcome to Kiro!\nYour account is active.\nUsage: unknown format") {
            Err(CliError::Parse(msg)) => assert!(msg.contains("No recognizable usage patterns")),
            other => panic!("unexpected: {other:?}"),
        }
        let login = "Failed to initialize auth portal.\n\
                     Please try again with: kiro-cli login --use-device-flow\n\
                     error: OAuth error: All callback ports are in use.";
        assert_eq!(parse(login).unwrap_err(), CliError::NotLoggedIn);
    }

    #[test]
    fn whoami_validation() {
        assert_eq!(
            validate_whoami("Not logged in", "", 1),
            Err(CliError::NotLoggedIn)
        );
        assert_eq!(
            validate_whoami("login required", "", 1),
            Err(CliError::NotLoggedIn)
        );
        assert!(matches!(
            validate_whoami("", "", 0),
            Err(CliError::Failed(msg)) if msg.contains("no output")
        ));
        assert!(matches!(
            validate_whoami("", "Connection error", 1),
            Err(CliError::Failed(msg)) if msg == "Connection error"
        ));
        assert!(matches!(
            validate_whoami("", "", 7),
            Err(CliError::Failed(msg)) if msg.contains("status 7")
        ));
        let account =
            validate_whoami("Logged in with Google\nEmail: user@example.com", "", 0).unwrap();
        assert_eq!(account.auth_method.as_deref(), Some("Google"));
        assert_eq!(account.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn whoami_legacy_bare_email_and_stderr_warnings() {
        let account = validate_whoami("user@example.com", "", 0).unwrap();
        assert_eq!(account.auth_method, None);
        assert_eq!(account.email.as_deref(), Some("user@example.com"));

        let account = validate_whoami(
            "\u{1b}[1mLogged in with Builder ID\u{1b}[0m\nEmail: a@b.co\n",
            "warning: cached session",
            0,
        )
        .unwrap();
        assert_eq!(account.auth_method.as_deref(), Some("Builder ID"));
        assert_eq!(account.email.as_deref(), Some("a@b.co"));
    }

    #[test]
    fn display_plan_name_title_cases_kiro_plans() {
        assert_eq!(display_plan_name("KIRO PRO MAX"), "Kiro Pro Max");
        assert_eq!(display_plan_name("KIRO   POWER"), "Kiro Power");
        assert_eq!(
            display_plan_name("\u{1b}[1mKIRO FREE\u{1b}[0m"),
            "Kiro Free"
        );
        assert_eq!(display_plan_name("Q Developer Pro"), "Q Developer Pro");
        assert_eq!(display_plan_name("Kiro"), "Kiro");
        assert_eq!(display_plan_name(""), "");
    }

    #[test]
    fn reset_dates_use_the_local_zone_and_roll_over() {
        let tokyo = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        let now_tokyo = tokyo.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap();
        let reset = parse_reset_date("2026-06-01", &now_tokyo).unwrap();
        assert_eq!(reset, Utc.with_ymd_and_hms(2026, 5, 31, 15, 0, 0).unwrap());

        // Still ahead this year.
        let reset = parse_reset_date("12/31", &now()).unwrap();
        assert_eq!(reset.year(), 2026);
        assert_eq!(reset.hour(), 0);
        // Already past: next year.
        assert_eq!(parse_reset_date("05/01", &now()).unwrap().year(), 2027);
        assert_eq!(parse_reset_date("13/45", &now()), None);
        assert_eq!(parse_reset_date("2026-02-30", &now()), None);
    }

    #[test]
    fn combine_output_skips_empty_streams() {
        assert_eq!(combine_output(" a \n", ""), "a");
        assert_eq!(combine_output("", " b "), "b");
        assert_eq!(combine_output("a", "b"), "a\nb");
        assert_eq!(combine_output("", ""), "");
    }
}
