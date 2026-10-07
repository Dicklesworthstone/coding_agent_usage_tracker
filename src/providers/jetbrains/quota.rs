//! Parsing of the AI Assistant quota file
//! (`options/AIAssistantQuotaManager2.xml`).
//!
//! Ports `JetBrainsStatusProbe.parseXMLData` from `CodexBar`. The file is
//! IDE-internal XML whose `quotaInfo` / `nextRefill` option values are JSON,
//! entity-escaped inside the attribute:
//!
//! ```xml
//! <component name="AIAssistantQuotaManager2">
//!   <option name="quotaInfo" value="{&#10;  &quot;current&quot;: &quot;7478.3&quot;, ...}" />
//!   <option name="nextRefill" value="{&quot;next&quot;: &quot;2026-01-16T14:00:54.939Z&quot;, ...}" />
//! </component>
//! ```
//!
//! There is no XML crate in the tree, so a small attribute-aware scanner
//! extracts the two values.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{Map, Value};

use crate::providers::common::parse_timestamp_str;

/// The component holding the quota options.
const COMPONENT_NAME: &str = "AIAssistantQuotaManager2";

/// Why the quota could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaError {
    /// The quota file does not exist.
    FileNotFound(PathBuf),
    /// The file or one of its JSON values is unreadable.
    Parse(String),
    /// The file has no `quotaInfo` value.
    NoQuotaInfo,
}

/// The monthly AI credit balance.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaInfo {
    /// Quota type, e.g. `Available`.
    pub quota_type: Option<String>,
    /// Credits used.
    pub used: f64,
    /// Credits granted.
    pub maximum: f64,
}

impl QuotaInfo {
    /// Percent of the quota used, clamped to 0..=100; 0 without a maximum.
    #[must_use]
    pub fn used_percent(&self) -> f64 {
        if self.maximum > 0.0 {
            (self.used / self.maximum * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

/// When and how the quota refills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefillInfo {
    /// Next refill time.
    pub next: Option<DateTime<Utc>>,
    /// Refill period as an ISO-8601 duration, e.g. `PT720H`.
    pub duration: Option<String>,
}

/// Everything read from the quota file.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaSnapshot {
    pub quota: QuotaInfo,
    pub refill: Option<RefillInfo>,
}

// =============================================================================
// XML scanning
// =============================================================================

/// An element start tag whose quoted attribute values may contain `>`.
static COMPONENT_TAG: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"<component\b((?:[^>"']|"[^"]*"|'[^']*')*)>"#).ok());
static OPTION_TAG: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"<option\b((?:[^>"']|"[^"]*"|'[^']*')*)>"#).ok());
static ATTRIBUTE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).ok()
});

/// The raw (still entity-encoded) value of attribute `name`.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    ATTRIBUTE
        .as_ref()?
        .captures_iter(attributes)
        .find(|caps| caps.get(1).is_some_and(|m| m.as_str() == name))
        .and_then(|caps| caps.get(2).or_else(|| caps.get(3)))
        .map(|m| m.as_str())
}

/// The body of `<component name="AIAssistantQuotaManager2">`; empty for a
/// self-closing tag.
fn component_body(xml: &str) -> Option<&str> {
    for caps in COMPONENT_TAG.as_ref()?.captures_iter(xml) {
        let (Some(tag), Some(attributes)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        if attribute(attributes.as_str(), "name") != Some(COMPONENT_NAME) {
            continue;
        }
        if attributes.as_str().trim_end().ends_with('/') {
            return Some("");
        }
        let rest = &xml[tag.end()..];
        return rest.find("</component>").map(|end| &rest[..end]);
    }
    None
}

/// The decoded `value` of `<option name="{name}" .../>`.
fn option_value(component: &str, name: &str) -> Option<String> {
    OPTION_TAG
        .as_ref()?
        .captures_iter(component)
        .filter_map(|caps| caps.get(1))
        .find(|attrs| attribute(attrs.as_str(), "name") == Some(name))
        .and_then(|attrs| attribute(attrs.as_str(), "value"))
        .map(decode_entities)
}

/// Decode XML character and entity references (`&quot;`, `&#10;`,
/// `&#x27;`, ...) in one pass, so `&amp;lt;` stays `&lt;`. Unknown
/// references are kept verbatim.
#[must_use]
pub fn decode_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let decoded = after.find(';').and_then(|end| {
            let entity = &after[..end];
            let ch = match entity {
                "quot" => Some('"'),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .map_or_else(
                        || entity.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()),
                        |hex| u32::from_str_radix(hex, 16).ok(),
                    )
                    .and_then(char::from_u32),
            }?;
            Some((ch, end))
        });
        // An unrecognized reference keeps its `&` and is scanned on from there.
        let (ch, consumed) = decoded.map_or(('&', 0), |(ch, end)| (ch, end + 1));
        out.push(ch);
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

// =============================================================================
// JSON values
// =============================================================================

fn parse_json_object(text: &str) -> Result<Map<String, Value>, QuotaError> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(QuotaError::Parse("Invalid JSON format".to_string())),
    }
}

/// A finite number written as a JSON string (the IDE's format) or number.
fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn string(value: Option<&Value>) -> Option<String> {
    value?.as_str().map(str::to_string)
}

/// Parse the `quotaInfo` JSON.
///
/// The monthly `tariffQuota` balance wins when both its `current` and
/// `maximum` are usable; otherwise the top-level totals are used together.
/// Monthly and total values (which include top-up credits) are never mixed.
///
/// # Errors
/// [`QuotaError::Parse`] when the text is not a JSON object.
pub fn parse_quota_info(text: &str) -> Result<QuotaInfo, QuotaError> {
    let json = parse_json_object(text)?;
    let tariff = json
        .get("tariffQuota")
        .and_then(Value::as_object)
        .filter(|q| number(q.get("current")).is_some() && number(q.get("maximum")).is_some());
    let quota = tariff.unwrap_or(&json);
    Ok(QuotaInfo {
        quota_type: string(json.get("type")),
        used: number(quota.get("current")).unwrap_or(0.0),
        maximum: number(quota.get("maximum")).unwrap_or(0.0),
    })
}

/// Parse the `nextRefill` JSON. Flat `duration` wins over `tariff.duration`.
///
/// # Errors
/// [`QuotaError::Parse`] when the text is not a JSON object.
pub fn parse_refill_info(text: &str) -> Result<RefillInfo, QuotaError> {
    let json = parse_json_object(text)?;
    let tariff = json.get("tariff").and_then(Value::as_object);
    Ok(RefillInfo {
        next: json
            .get("next")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_str),
        duration: string(json.get("duration"))
            .or_else(|| tariff.and_then(|t| string(t.get("duration")))),
    })
}

/// Parse the quota file's XML text.
///
/// # Errors
/// [`QuotaError::NoQuotaInfo`] when there is no non-empty `quotaInfo`
/// value, [`QuotaError::Parse`] when it is not a JSON object. An unreadable
/// `nextRefill` only drops the refill date.
pub fn parse_quota_xml(xml: &str) -> Result<QuotaSnapshot, QuotaError> {
    let component = component_body(xml).ok_or(QuotaError::NoQuotaInfo)?;
    let quota_raw = option_value(component, "quotaInfo")
        .filter(|v| !v.is_empty())
        .ok_or(QuotaError::NoQuotaInfo)?;
    let quota = parse_quota_info(&quota_raw)?;
    let refill = option_value(component, "nextRefill")
        .filter(|v| !v.is_empty())
        .and_then(|raw| parse_refill_info(&raw).ok());
    Ok(QuotaSnapshot { quota, refill })
}

/// Read and parse a quota file.
///
/// # Errors
/// [`QuotaError::FileNotFound`] when the file is missing,
/// [`QuotaError::Parse`] when it cannot be read as UTF-8, otherwise the
/// errors of [`parse_quota_xml`].
pub fn read_quota_file(path: &Path) -> Result<QuotaSnapshot, QuotaError> {
    if !path.exists() {
        return Err(QuotaError::FileNotFound(path.to_path_buf()));
    }
    let xml = std::fs::read_to_string(path)
        .map_err(|e| QuotaError::Parse(format!("Failed to read file: {e}")))?;
    parse_quota_xml(&xml)
}

/// Minutes in an ISO-8601 duration with week/day/hour/minute/second parts
/// (`PT720H`, `P30D`, `P1W`). Calendar months and years have no fixed
/// length, so they yield `None`.
#[must_use]
pub fn duration_minutes(iso: &str) -> Option<i32> {
    let body = iso.trim().strip_prefix('P')?;
    let (date_part, time_part) = body.split_once('T').unwrap_or((body, ""));
    let mut seconds: i64 = 0;
    let mut parsed_any = false;
    for (part, units) in [
        (date_part, &[('W', 604_800), ('D', 86_400)][..]),
        (time_part, &[('H', 3_600), ('M', 60), ('S', 1)][..]),
    ] {
        let mut digits = String::new();
        for ch in part.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            let factor = units.iter().find(|(unit, _)| *unit == ch)?.1;
            let value: i64 = digits.parse().ok()?;
            seconds = seconds.checked_add(value.checked_mul(factor)?)?;
            digits.clear();
            parsed_any = true;
        }
        if !digits.is_empty() {
            return None;
        }
    }
    if !parsed_any || seconds <= 0 {
        return None;
    }
    i32::try_from(seconds / 60).ok().filter(|m| *m > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn xml_with(options: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<application>\n  \
             <component name=\"AIAssistantQuotaManager2\">\n{options}\n  </component>\n</application>\n"
        )
    }

    #[test]
    fn patterns_compile() {
        assert!(COMPONENT_TAG.is_some());
        assert!(OPTION_TAG.is_some());
        assert!(ATTRIBUTE.is_some());
    }

    #[test]
    fn parses_quota_xml_with_tariff_quota() {
        let xml = include_str!("../../../tests/fixtures/jetbrains/AIAssistantQuotaManager2.xml");
        let snapshot = parse_quota_xml(xml).unwrap();
        assert_eq!(snapshot.quota.quota_type.as_deref(), Some("Available"));
        assert!((snapshot.quota.used - 7478.3).abs() < 1e-9);
        assert!((snapshot.quota.maximum - 1_000_000.0).abs() < 1e-9);
        assert!((snapshot.quota.used_percent() - 0.747_83).abs() < 1e-9);
        let refill = snapshot.refill.unwrap();
        assert_eq!(
            refill.next,
            Some(Utc.timestamp_millis_opt(1_768_572_054_939).unwrap())
        );
        assert_eq!(refill.duration.as_deref(), Some("PT720H"));
    }

    #[test]
    fn parses_quota_xml_without_tariff_quota() {
        let quota = "{&#10;  &quot;type&quot;: &quot;paid&quot;,&#10;  &quot;current&quot;: &quot;50000&quot;,\
                     &#10;  &quot;maximum&quot;: &quot;100000&quot;,&#10;  &quot;until&quot;: &quot;2025-12-31T23:59:59Z&quot;&#10;}";
        let refill = "{&#10;  &quot;type&quot;: &quot;monthly&quot;,&#10;  &quot;next&quot;: &quot;2025-01-01T00:00:00Z&quot;,\
                      &#10;  &quot;tariff&quot;: {&#10;    &quot;amount&quot;: &quot;100000&quot;,\
                      &#10;    &quot;duration&quot;: &quot;monthly&quot;&#10;  }&#10;}";
        let xml = xml_with(&format!(
            "    <option\n      name=\"quotaInfo\"\n      value=\"{quota}\" />\n    \
             <option\n      name=\"nextRefill\"\n      value=\"{refill}\" />"
        ));
        let snapshot = parse_quota_xml(&xml).unwrap();
        assert_eq!(snapshot.quota.quota_type.as_deref(), Some("paid"));
        assert!((snapshot.quota.used - 50_000.0).abs() < f64::EPSILON);
        assert!((snapshot.quota.maximum - 100_000.0).abs() < f64::EPSILON);
        assert!((snapshot.quota.used_percent() - 50.0).abs() < f64::EPSILON);
        let refill = snapshot.refill.unwrap();
        assert_eq!(
            refill.next,
            Some(Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap())
        );
        assert_eq!(refill.duration.as_deref(), Some("monthly"));
    }

    #[test]
    fn monthly_tariff_wins_over_top_up_inflated_total() {
        let xml =
            include_str!("../../../tests/fixtures/jetbrains/AIAssistantQuotaManager2_topup.xml");
        let snapshot = parse_quota_xml(xml).unwrap();
        assert!((snapshot.quota.used - 346_000.0).abs() < f64::EPSILON);
        assert!((snapshot.quota.maximum - 1_000_000.0).abs() < f64::EPSILON);
        assert!((snapshot.quota.used_percent() - 34.6).abs() < 1e-9);
        assert_eq!(snapshot.refill, None);
    }

    #[test]
    fn incomplete_monthly_quota_falls_back_to_one_consistent_total() {
        for monthly in [
            r#"{"current":"25000","available":"75000"}"#,
            r#"{"maximum":"100000","available":"75000"}"#,
            r#"{"current":"NaN","maximum":"100000","available":"75000"}"#,
            r#"{"current":"25000","maximum":"invalid","available":"75000"}"#,
            r#""not an object""#,
        ] {
            let json = format!(
                r#"{{"type":"Available","current":"50000","maximum":"200000","tariffQuota":{monthly}}}"#
            );
            let encoded = json.replace('"', "&quot;");
            let xml = xml_with(&format!(
                "<option name=\"quotaInfo\" value=\"{encoded}\" />"
            ));
            let quota = parse_quota_xml(&xml).unwrap().quota;
            assert!((quota.used - 50_000.0).abs() < f64::EPSILON, "{monthly}");
            assert!(
                (quota.maximum - 200_000.0).abs() < f64::EPSILON,
                "{monthly}"
            );
            assert!(
                (quota.used_percent() - 25.0).abs() < f64::EPSILON,
                "{monthly}"
            );
        }
    }

    #[test]
    fn flat_refill_fields_win_over_nested_tariff_fields() {
        let xml = "<application><component name=\"AIAssistantQuotaManager2\">\n  \
            <option name=\"quotaInfo\" value=\"{&quot;current&quot;:&quot;0&quot;,&quot;maximum&quot;:&quot;100&quot;}\" />\n  \
            <option name=\"nextRefill\"\n    \
            value=\"{&quot;type&quot;:&quot;Known&quot;,&quot;amount&quot;:&quot;200&quot;,\n    \
            &quot;duration&quot;:&quot;PT720H&quot;,&quot;tariff&quot;:{&quot;amount&quot;:&quot;100&quot;,\n    \
            &quot;duration&quot;:&quot;PT24H&quot;}}\" />\n\
            </component></application>";
        let refill = parse_quota_xml(xml).unwrap().refill.unwrap();
        assert_eq!(refill.duration.as_deref(), Some("PT720H"));
        assert_eq!(refill.next, None);
    }

    #[test]
    fn handles_entities_attribute_order_and_single_quotes() {
        let xml = xml_with(
            "<option value='{&quot;type&quot;:&quot;free&quot;,&quot;current&quot;:&quot;0&quot;,\
             &quot;maximum&quot;:&quot;50000&quot;}' name='quotaInfo'/>",
        );
        let quota = parse_quota_xml(&xml).unwrap().quota;
        assert_eq!(quota.quota_type.as_deref(), Some("free"));
        assert!(quota.used.abs() < f64::EPSILON);
        assert!((quota.maximum - 50_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn accepts_numeric_json_values() {
        let xml = xml_with(
            "<option name=\"quotaInfo\" value=\"{&quot;current&quot;:250,&quot;maximum&quot;:1000}\"/>",
        );
        assert!((parse_quota_xml(&xml).unwrap().quota.used_percent() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn ignores_other_components_and_options() {
        let xml = "<application>\
            <component name=\"Other\"><option name=\"quotaInfo\" value=\"{&quot;current&quot;:&quot;9&quot;}\"/></component>\
            <component name=\"AIAssistantQuotaManager2\">\
            <option name=\"quotaInfoBackup\" value=\"x\"/>\
            <option name=\"quotaInfo\" value=\"{&quot;current&quot;:&quot;1&quot;,&quot;maximum&quot;:&quot;4&quot;}\"/>\
            </component></application>";
        let quota = parse_quota_xml(xml).unwrap().quota;
        assert!((quota.used - 1.0).abs() < f64::EPSILON);
        assert!((quota.maximum - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_or_empty_quota_info_is_no_quota_info() {
        assert_eq!(
            parse_quota_xml(&xml_with("")).unwrap_err(),
            QuotaError::NoQuotaInfo
        );
        assert_eq!(
            parse_quota_xml(&xml_with("<option name=\"quotaInfo\" value=\"\" />")).unwrap_err(),
            QuotaError::NoQuotaInfo
        );
        assert_eq!(
            parse_quota_xml(
                "<application><component name=\"AIAssistantQuotaManager2\"/></application>"
            )
            .unwrap_err(),
            QuotaError::NoQuotaInfo
        );
        assert_eq!(parse_quota_xml("").unwrap_err(), QuotaError::NoQuotaInfo);
        assert_eq!(
            parse_quota_xml("<application></application>").unwrap_err(),
            QuotaError::NoQuotaInfo
        );
    }

    #[test]
    fn invalid_quota_json_is_a_parse_error_but_bad_refill_is_dropped() {
        let xml = xml_with("<option name=\"quotaInfo\" value=\"not json\" />");
        assert!(matches!(parse_quota_xml(&xml), Err(QuotaError::Parse(_))));
        let xml = xml_with("<option name=\"quotaInfo\" value=\"[1,2]\" />");
        assert!(matches!(parse_quota_xml(&xml), Err(QuotaError::Parse(_))));

        let xml = xml_with(
            "<option name=\"quotaInfo\" value=\"{&quot;current&quot;:&quot;1&quot;,&quot;maximum&quot;:&quot;2&quot;}\" />\
             <option name=\"nextRefill\" value=\"{broken\" />",
        );
        let snapshot = parse_quota_xml(&xml).unwrap();
        assert_eq!(snapshot.refill, None);
    }

    #[test]
    fn used_percent_edges() {
        let quota = |used, maximum| QuotaInfo {
            quota_type: None,
            used,
            maximum,
        };
        assert!(quota(0.0, 100_000.0).used_percent().abs() < f64::EPSILON);
        assert!((quota(100_000.0, 100_000.0).used_percent() - 100.0).abs() < f64::EPSILON);
        assert!((quota(150_000.0, 100_000.0).used_percent() - 100.0).abs() < f64::EPSILON);
        assert!((quota(25_000.0, 100_000.0).used_percent() - 25.0).abs() < f64::EPSILON);
        // No maximum reads as nothing used, as in CodexBar.
        assert!(quota(1000.0, 0.0).used_percent().abs() < f64::EPSILON);
    }

    #[test]
    fn decodes_entities_in_one_pass() {
        assert_eq!(
            decode_entities("&quot;a&quot;&#10;&amp;&lt;&gt;&apos;"),
            "\"a\"\n&<>'"
        );
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
        assert_eq!(decode_entities("&#x41;&#65;&#X42;"), "AAB");
        assert_eq!(
            decode_entities("a & b &unknown; &#xZZ; &"),
            "a & b &unknown; &#xZZ; &"
        );
        assert_eq!(decode_entities("plain"), "plain");
        assert_eq!(decode_entities("é&quot;"), "é\"");
    }

    #[test]
    fn duration_minutes_parses_fixed_lengths() {
        assert_eq!(duration_minutes("PT720H"), Some(43_200));
        assert_eq!(duration_minutes("PT24H"), Some(1_440));
        assert_eq!(duration_minutes("P30D"), Some(43_200));
        assert_eq!(duration_minutes("P1W"), Some(10_080));
        assert_eq!(duration_minutes("P1DT12H30M"), Some(2_190));
        assert_eq!(duration_minutes("PT90S"), Some(1));
        assert_eq!(duration_minutes("P1M"), None);
        assert_eq!(duration_minutes("monthly"), None);
        assert_eq!(duration_minutes("P"), None);
        assert_eq!(duration_minutes("PT0H"), None);
        assert_eq!(duration_minutes("PT30"), None);
        assert_eq!(duration_minutes("PT10S"), None);
        assert_eq!(duration_minutes("PT99999999999999999999H"), None);
    }

    #[test]
    fn read_quota_file_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("options")
            .join("AIAssistantQuotaManager2.xml");
        assert_eq!(
            read_quota_file(&path).unwrap_err(),
            QuotaError::FileNotFound(path)
        );
    }
}
