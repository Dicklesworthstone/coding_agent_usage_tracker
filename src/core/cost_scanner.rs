//! Local cost scanning for Claude and Codex.
//!
//! Scans local JSONL history files and stats caches to compute
//! usage statistics for the cost command.

use crate::core::models::{CostDailyEntry, CostPayload, CostTotals};
use crate::core::pricing::{PricingTable, ResponseTokens, canonical_model_id};
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::paths::AppPaths;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Days of history the cost command reports.
const COST_WINDOW_DAYS: i64 = 30;

/// How deep under `projects/` session logs are searched: `<project>/<session>.jsonl` and
/// subagent transcripts at `<project>/<session>/subagents/<agent>.jsonl`.
const CLAUDE_LOG_MAX_DEPTH: usize = 4;

/// Claude stats cache format (from ~/.claude/stats-cache.json).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeStatsCache {
    #[allow(dead_code)]
    version: i32,
    #[allow(dead_code)]
    last_computed_date: String,
    daily_activity: Vec<ClaudeDailyActivity>,
    /// Tokens per model per day (newer caches only; there is no input/output split).
    #[serde(default)]
    daily_model_tokens: Vec<ClaudeDailyModelTokens>,
}

/// Per-day token totals by model from Claude stats cache.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDailyModelTokens {
    date: String,
    #[serde(default)]
    tokens_by_model: HashMap<String, i64>,
}

/// One line of a Claude Code session log. Only assistant entries carry `message.usage`.
#[derive(Debug, Deserialize)]
struct ClaudeLogEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<ClaudeLogMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeLogMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeLogUsage>,
}

/// The API `usage` object Claude Code records for each response.
#[derive(Debug, Deserialize)]
struct ClaudeLogUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    cache_creation: Option<ClaudeLogCacheCreation>,
    speed: Option<String>,
    inference_geo: Option<String>,
}

/// Cache writes split by TTL.
#[derive(Debug, Deserialize)]
struct ClaudeLogCacheCreation {
    ephemeral_5m_input_tokens: Option<i64>,
    ephemeral_1h_input_tokens: Option<i64>,
}

impl ClaudeLogUsage {
    /// Token counts as priced. Cache writes the log does not split by TTL are 5-minute
    /// writes, the API default.
    fn response_tokens(&self) -> ResponseTokens {
        let cache_creation = self.cache_creation_input_tokens.unwrap_or(0).max(0);
        let (mut short_ttl, long_ttl) = self.cache_creation.as_ref().map_or((0, 0), |split| {
            (
                split.ephemeral_5m_input_tokens.unwrap_or(0).max(0),
                split.ephemeral_1h_input_tokens.unwrap_or(0).max(0),
            )
        });
        short_ttl += (cache_creation - short_ttl - long_ttl).max(0);
        ResponseTokens {
            input: self.input_tokens.unwrap_or(0).max(0),
            output: self.output_tokens.unwrap_or(0).max(0),
            cache_read: self.cache_read_input_tokens.unwrap_or(0).max(0),
            cache_write_5m: short_ttl,
            cache_write_1h: long_ttl,
            fast: self.speed.as_deref() == Some("fast"),
            us_only_inference: self.inference_geo.as_deref() == Some("us"),
        }
    }
}

/// One day of Claude usage read from session logs.
#[derive(Debug, Default)]
struct ClaudeDayUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
    cost_usd: f64,
    priced_responses: u64,
    models: BTreeSet<String>,
}

impl ClaudeDayUsage {
    const fn total_tokens(&self) -> i64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }

    /// Cost of the priced responses, or `None` when no response that day had a known price.
    fn cost(&self) -> Option<f64> {
        (self.priced_responses > 0).then_some(self.cost_usd)
    }
}

/// What a scan of Claude Code session logs found.
#[derive(Debug, Default)]
struct ClaudeLogScan {
    /// Session log files found, inside the window or not.
    log_files: usize,
    /// Usage per UTC date (`YYYY-MM-DD`).
    days: BTreeMap<String, ClaudeDayUsage>,
    /// Models whose tokens are counted but have no price in the table.
    unpriced_models: BTreeSet<String>,
}

/// Daily activity entry from Claude stats cache.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDailyActivity {
    date: String,
    // Activity counts are parsed but not reported: they are not token counts.
    #[allow(dead_code)]
    message_count: i64,
    #[allow(dead_code)]
    session_count: i64,
    #[allow(dead_code)]
    tool_call_count: i64,
}

/// Codex session event from JSONL files.
#[derive(Debug, Deserialize)]
struct CodexEvent {
    timestamp: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    event_type: String,
    #[serde(default)]
    #[allow(dead_code)]
    payload: Option<serde_json::Value>,
}

/// Cost scanner for local usage data.
pub struct CostScanner {
    #[allow(dead_code)] // reserved for future path-based scanning
    paths: AppPaths,
    pricing: PricingTable,
}

impl CostScanner {
    /// Create a new cost scanner.
    #[must_use]
    pub fn new() -> Self {
        Self {
            paths: AppPaths::new(),
            pricing: PricingTable::current(),
        }
    }

    /// Scan cost data for a provider.
    ///
    /// # Errors
    /// Returns an error if the provider does not support local cost scanning
    /// or if the stats cache or session files cannot be read.
    pub async fn scan(&self, provider: Provider, _refresh: bool) -> Result<CostPayload> {
        match provider {
            Provider::Claude => self.scan_claude().await,
            Provider::Codex => self.scan_codex().await,
            _ => Err(CautError::Config(format!(
                "Provider {} does not support local cost scanning",
                provider.cli_name()
            ))),
        }
    }

    /// Scan Claude Code's local usage data.
    async fn scan_claude(&self) -> Result<CostPayload> {
        let config_dirs = claude_config_dirs()?;
        self.scan_claude_dirs(&config_dirs, Utc::now())
    }

    /// Scan Claude usage under the given Claude Code config directories.
    ///
    /// Session logs (`<dir>/projects/**/*.jsonl`) are the primary source: each assistant
    /// entry records the model and the API `usage` of one response. Responses are
    /// deduplicated by message and request id, because Claude Code writes one entry per
    /// content block and resumed sessions repeat earlier entries. Without session logs,
    /// `stats-cache.json` gives per-day token totals but no cost. With neither, this is an
    /// error, so a missing source is not reported as zero usage.
    fn scan_claude_dirs(&self, config_dirs: &[PathBuf], now: DateTime<Utc>) -> Result<CostPayload> {
        let cutoff_date = (now - Duration::days(COST_WINDOW_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let projects_dirs: Vec<PathBuf> = config_dirs
            .iter()
            .map(|dir| dir.join("projects"))
            .filter(|dir| dir.is_dir())
            .collect();

        if !projects_dirs.is_empty() {
            let scan = scan_claude_session_logs(&projects_dirs, &cutoff_date, &self.pricing);
            if scan.log_files > 0 {
                if !scan.unpriced_models.is_empty() {
                    tracing::warn!(
                        models = ?scan.unpriced_models,
                        "No price for these Claude models: their tokens are counted, not costed"
                    );
                }
                return Ok(claude_payload_from_logs(&scan, now));
            }
            tracing::debug!(?projects_dirs, "No Claude session logs found");
        }

        if let Some(stats_path) = config_dirs
            .iter()
            .map(|dir| dir.join("stats-cache.json"))
            .find(|path| path.is_file())
        {
            return Self::scan_claude_stats_cache(&stats_path, &cutoff_date, now);
        }

        let searched: Vec<String> = config_dirs
            .iter()
            .map(|dir| dir.display().to_string())
            .collect();
        Err(CautError::Config(format!(
            "no Claude usage data found: no session logs (projects/**/*.jsonl) or \
             stats-cache.json under {}",
            searched.join(" or ")
        )))
    }

    /// Fallback: per-day token totals from Claude Code's `stats-cache.json`.
    ///
    /// The cache has no input/output split, so no cost can be derived from it. Days with
    /// only message counts get no token count: a message count is not a token count.
    fn scan_claude_stats_cache(
        stats_path: &Path,
        cutoff_date: &str,
        now: DateTime<Utc>,
    ) -> Result<CostPayload> {
        tracing::debug!(?stats_path, "Reading Claude stats cache");

        let file = File::open(stats_path)
            .map_err(|e| CautError::Config(format!("Failed to open Claude stats cache: {e}")))?;

        let stats: ClaudeStatsCache = serde_json::from_reader(file).map_err(|e| {
            CautError::ParseResponse(format!("Failed to parse Claude stats cache: {e}"))
        })?;

        let mut days: BTreeMap<String, Option<(i64, BTreeSet<String>)>> = BTreeMap::new();
        for activity in &stats.daily_activity {
            if activity.date.as_str() >= cutoff_date {
                days.entry(activity.date.clone()).or_insert(None);
            }
        }
        for day in &stats.daily_model_tokens {
            if day.date.as_str() >= cutoff_date {
                let tokens = day.tokens_by_model.values().sum();
                let models = day.tokens_by_model.keys().cloned().collect();
                days.insert(day.date.clone(), Some((tokens, models)));
            }
        }

        let today = now.format("%Y-%m-%d").to_string();
        let mut total_tokens: Option<i64> = None;
        let mut today_tokens: Option<i64> = None;
        let daily_entries: Vec<CostDailyEntry> = days
            .into_iter()
            .rev()
            .map(|(date, usage)| {
                let (tokens, models) = usage.map_or((None, None), |(tokens, models)| {
                    (Some(tokens), Some(models.into_iter().collect()))
                });
                if let Some(tokens) = tokens {
                    *total_tokens.get_or_insert(0) += tokens;
                    if date == today {
                        today_tokens = Some(tokens);
                    }
                }
                CostDailyEntry {
                    date,
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    total_tokens: tokens,
                    total_cost: None,
                    models_used: models,
                }
            })
            .collect();

        Ok(CostPayload {
            provider: "claude".to_string(),
            source: "local".to_string(),
            updated_at: now,
            session_tokens: today_tokens.or_else(|| total_tokens.map(|_| 0)),
            session_cost_usd: None,
            last_30_days_tokens: total_tokens,
            last_30_days_cost_usd: None,
            daily: daily_entries,
            totals: Some(CostTotals {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens,
                total_cost: None,
            }),
        })
    }

    /// Scan Codex's local session files.
    async fn scan_codex(&self) -> Result<CostPayload> {
        let codex_dir = dirs::home_dir()
            .ok_or_else(|| CautError::Config("Cannot determine home directory".to_string()))?
            .join(".codex");

        let sessions_dir = codex_dir.join("sessions");

        if !sessions_dir.exists() {
            tracing::debug!(?sessions_dir, "Codex sessions directory not found");
            return Ok(self.empty_cost_payload("codex"));
        }

        tracing::debug!(?sessions_dir, "Scanning Codex sessions directory");

        // Collect all JSONL files from the sessions directory
        let cutoff = Utc::now() - Duration::days(30);
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();

        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();
        let today = Utc::now().format("%Y-%m-%d").to_string();

        // Walk through year/month/day directories
        if let Ok(years) = std::fs::read_dir(&sessions_dir) {
            for year_entry in years.flatten() {
                if !year_entry.path().is_dir() {
                    continue;
                }
                if let Ok(months) = std::fs::read_dir(year_entry.path()) {
                    for month_entry in months.flatten() {
                        if !month_entry.path().is_dir() {
                            continue;
                        }
                        if let Ok(days) = std::fs::read_dir(month_entry.path()) {
                            for day_entry in days.flatten() {
                                let day_path = day_entry.path();
                                if !day_path.is_dir() {
                                    continue;
                                }

                                // Read JSONL files in this day's directory
                                if let Ok(files) = std::fs::read_dir(&day_path) {
                                    for file_entry in files.flatten() {
                                        let file_path = file_entry.path();
                                        if file_path.extension().is_some_and(|ext| ext == "jsonl") {
                                            self.scan_codex_jsonl(
                                                &file_path,
                                                &cutoff_date,
                                                &mut daily_counts,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Also check history.jsonl in codex root
        let history_path = codex_dir.join("history.jsonl");
        if history_path.exists() {
            self.scan_codex_history(&history_path, &cutoff_date, &mut daily_counts);
        }

        // Convert to daily entries
        let mut daily_entries: Vec<CostDailyEntry> = daily_counts
            .into_iter()
            .map(|(date, count)| CostDailyEntry {
                date,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(count.events),
                total_cost: None,
                models_used: None,
            })
            .collect();

        daily_entries.sort_by(|a, b| b.date.cmp(&a.date));

        let total_events: i64 = daily_entries.iter().filter_map(|e| e.total_tokens).sum();

        let today_events = daily_entries
            .iter()
            .find(|e| e.date == today)
            .and_then(|e| e.total_tokens)
            .unwrap_or(0);

        Ok(CostPayload {
            provider: "codex".to_string(),
            source: "local".to_string(),
            updated_at: Utc::now(),
            session_tokens: Some(today_events),
            session_cost_usd: None,
            last_30_days_tokens: Some(total_events),
            last_30_days_cost_usd: None,
            daily: daily_entries,
            totals: Some(CostTotals {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                total_tokens: Some(total_events),
                total_cost: None,
            }),
        })
    }

    /// Scan a Codex JSONL session file.
    #[allow(clippy::unused_self)]
    fn scan_codex_jsonl(
        &self,
        path: &PathBuf,
        cutoff_date: &str,
        daily_counts: &mut HashMap<String, DailyCount>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(?path, error = %e, "Failed to open Codex session file");
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut recent_lines: VecDeque<String> = VecDeque::with_capacity(100);

        for line in reader.lines() {
            let Ok(line) = line else { continue };

            if line.is_empty() {
                continue;
            }

            if recent_lines.len() == 100 {
                recent_lines.pop_front();
            }
            recent_lines.push_back(line);
        }

        for line in recent_lines {
            let event: CodexEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Extract date from timestamp
            if let Some(date) = event.timestamp.get(..10)
                && date >= cutoff_date
            {
                daily_counts.entry(date.to_string()).or_default().events += 1;
            }
        }
    }

    /// Scan Codex history.jsonl for session counts.
    #[allow(clippy::unused_self)]
    fn scan_codex_history(
        &self,
        path: &PathBuf,
        cutoff_date: &str,
        daily_counts: &mut HashMap<String, DailyCount>,
    ) {
        #[derive(Deserialize)]
        struct HistoryEntry {
            ts: i64,
        }

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(?path, error = %e, "Failed to open Codex history file");
                return;
            }
        };

        let reader = BufReader::new(file);

        for line in reader.lines() {
            let Ok(line) = line else { continue };

            if line.is_empty() {
                continue;
            }

            // History format: {"session_id": "...", "ts": 1234567890, "text": "..."}
            let entry: HistoryEntry = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Convert timestamp to date
            if let Some(dt) = DateTime::from_timestamp(entry.ts, 0) {
                let date = dt.format("%Y-%m-%d").to_string();
                if date.as_str() >= cutoff_date {
                    daily_counts.entry(date).or_default().events += 1;
                }
            }
        }
    }

    /// Create an empty cost payload for a provider.
    #[allow(clippy::unused_self)]
    fn empty_cost_payload(&self, provider: &str) -> CostPayload {
        CostPayload {
            provider: provider.to_string(),
            source: "local".to_string(),
            updated_at: Utc::now(),
            session_tokens: None,
            session_cost_usd: None,
            last_30_days_tokens: None,
            last_30_days_cost_usd: None,
            daily: Vec::new(),
            totals: None,
        }
    }
}

impl Default for CostScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper for counting daily events.
#[derive(Default)]
struct DailyCount {
    events: i64,
}

/// Claude Code config directories to read: `$CLAUDE_CONFIG_DIR` when set, otherwise
/// `~/.config/claude` and `~/.claude` (session logs from both are merged and deduplicated).
fn claude_config_dirs() -> Result<Vec<PathBuf>> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(vec![PathBuf::from(dir)]);
    }
    let home = dirs::home_dir()
        .ok_or_else(|| CautError::Config("Cannot determine home directory".to_string()))?;
    Ok(vec![
        home.join(".config").join("claude"),
        home.join(".claude"),
    ])
}

/// Every `*.jsonl` file under `root`, at most [`CLAUDE_LOG_MAX_DEPTH`] levels down.
/// Symlinks are not followed.
fn collect_jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if depth < CLAUDE_LOG_MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Read Claude usage per UTC day, on or after `cutoff_date`, from session logs.
fn scan_claude_session_logs(
    projects_dirs: &[PathBuf],
    cutoff_date: &str,
    pricing: &PricingTable,
) -> ClaudeLogScan {
    let mut scan = ClaudeLogScan::default();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    // Logs are append-only, so a file last written before the window holds nothing in it.
    let window_start = chrono::NaiveDate::parse_from_str(cutoff_date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|start| SystemTime::from(start.and_utc()));

    for projects_dir in projects_dirs {
        for path in collect_jsonl_files(projects_dir) {
            scan.log_files += 1;
            if let Some(window_start) = window_start
                && let Ok(modified) = std::fs::metadata(&path).and_then(|meta| meta.modified())
                && modified < window_start
            {
                continue;
            }
            scan_claude_log_file(&path, cutoff_date, pricing, &mut seen, &mut scan);
        }
    }
    scan
}

/// Add the assistant responses in one session log to `scan`.
fn scan_claude_log_file(
    path: &Path,
    cutoff_date: &str,
    pricing: &PricingTable,
    seen: &mut HashSet<(String, String)>,
    scan: &mut ClaudeLogScan,
) {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            tracing::debug!(?path, error = %e, "Failed to open Claude session log");
            return;
        }
    };

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        // Cheap filter: only assistant entries carry usage.
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<ClaudeLogEntry>(&line) else {
            continue;
        };
        if entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(message) = entry.message else {
            continue;
        };
        let (Some(usage), Some(model)) = (message.usage, message.model) else {
            continue;
        };
        // Claude Code's locally generated messages (errors, interrupts) are not API calls.
        if model.starts_with('<') {
            continue;
        }
        let Some(timestamp) = entry
            .timestamp
            .as_deref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
        else {
            continue;
        };
        let date = timestamp.with_timezone(&Utc).format("%Y-%m-%d").to_string();
        if date.as_str() < cutoff_date {
            continue;
        }
        if let Some(message_id) = message.id
            && !seen.insert((message_id, entry.request_id.unwrap_or_default()))
        {
            continue;
        }

        let tokens = usage.response_tokens();
        let day = scan.days.entry(date).or_default();
        day.input += tokens.input;
        day.output += tokens.output;
        day.cache_read += tokens.cache_read;
        day.cache_creation += tokens.cache_write_5m + tokens.cache_write_1h;
        match pricing.response_cost(&model, &tokens) {
            Some(cost) => {
                day.cost_usd += cost;
                day.priced_responses += 1;
            }
            None => {
                scan.unpriced_models.insert(model.clone());
            }
        }
        day.models.insert(canonical_model_id(&model));
    }
}

/// Build the cost payload from session-log usage.
fn claude_payload_from_logs(scan: &ClaudeLogScan, now: DateTime<Utc>) -> CostPayload {
    let today = now.format("%Y-%m-%d").to_string();
    let mut totals = ClaudeDayUsage::default();
    for day in scan.days.values() {
        totals.input += day.input;
        totals.output += day.output;
        totals.cache_read += day.cache_read;
        totals.cache_creation += day.cache_creation;
        totals.cost_usd += day.cost_usd;
        totals.priced_responses += day.priced_responses;
    }

    let daily: Vec<CostDailyEntry> = scan
        .days
        .iter()
        .rev()
        .map(|(date, day)| CostDailyEntry {
            date: date.clone(),
            input_tokens: Some(day.input),
            output_tokens: Some(day.output),
            cache_read_tokens: Some(day.cache_read),
            cache_creation_tokens: Some(day.cache_creation),
            total_tokens: Some(day.total_tokens()),
            total_cost: day.cost(),
            models_used: Some(day.models.iter().cloned().collect()),
        })
        .collect();

    // No usage at all is a real zero; usage with no known price has no cost.
    let cost_or_zero = |day: &ClaudeDayUsage| {
        if day.total_tokens() == 0 {
            Some(0.0)
        } else {
            day.cost()
        }
    };
    let today_usage = scan.days.get(&today);

    CostPayload {
        provider: "claude".to_string(),
        source: "local".to_string(),
        updated_at: now,
        session_tokens: Some(today_usage.map_or(0, ClaudeDayUsage::total_tokens)),
        session_cost_usd: today_usage.map_or(Some(0.0), cost_or_zero),
        last_30_days_tokens: Some(totals.total_tokens()),
        last_30_days_cost_usd: cost_or_zero(&totals),
        daily,
        totals: Some(CostTotals {
            input_tokens: Some(totals.input),
            output_tokens: Some(totals.output),
            cache_read_tokens: Some(totals.cache_read),
            cache_creation_tokens: Some(totals.cache_creation),
            total_tokens: Some(totals.total_tokens()),
            total_cost: cost_or_zero(&totals),
        }),
    }
}

/// Module-level access to dirs crate.
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;
    use tempfile::TempDir;

    // =========================================================================
    // CostScanner Creation Tests
    // =========================================================================

    #[test]
    fn test_cost_scanner_new() {
        let scanner = CostScanner::new();
        // Just verify it creates without panic
        // Using scanner to avoid warning
        let _ = scanner.paths;
    }

    #[test]
    fn test_cost_scanner_default() {
        let scanner = CostScanner::default();
        // Default implementation works
        let _ = scanner.paths;
    }

    // =========================================================================
    // Empty Cost Payload Tests
    // =========================================================================

    #[test]
    fn test_empty_cost_payload() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("test");
        assert_eq!(payload.provider, "test");
        assert_eq!(payload.source, "local");
        assert!(payload.daily.is_empty());
    }

    #[test]
    fn test_empty_cost_payload_claude() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("claude");
        assert_eq!(payload.provider, "claude");
        assert_eq!(payload.source, "local");
        assert!(payload.session_tokens.is_none());
        assert!(payload.session_cost_usd.is_none());
        assert!(payload.last_30_days_tokens.is_none());
        assert!(payload.last_30_days_cost_usd.is_none());
        assert!(payload.daily.is_empty());
        assert!(payload.totals.is_none());
    }

    #[test]
    fn test_empty_cost_payload_codex() {
        let scanner = CostScanner::new();
        let payload = scanner.empty_cost_payload("codex");
        assert_eq!(payload.provider, "codex");
        assert_eq!(payload.source, "local");
    }

    // =========================================================================
    // ClaudeStatsCache Parsing Tests
    // =========================================================================

    #[test]
    fn test_claude_stats_cache_parse_full() {
        let json = r#"{
            "version": 1,
            "lastComputedDate": "2026-01-18",
            "dailyActivity": [
                {"date": "2026-01-18", "messageCount": 42, "sessionCount": 3, "toolCallCount": 15},
                {"date": "2026-01-17", "messageCount": 35, "sessionCount": 2, "toolCallCount": 12}
            ]
        }"#;

        let stats: ClaudeStatsCache = serde_json::from_str(json).unwrap();
        assert_eq!(stats.version, 1);
        assert_eq!(stats.last_computed_date, "2026-01-18");
        assert_eq!(stats.daily_activity.len(), 2);

        let first = &stats.daily_activity[0];
        assert_eq!(first.date, "2026-01-18");
        assert_eq!(first.message_count, 42);
        assert_eq!(first.session_count, 3);
        assert_eq!(first.tool_call_count, 15);
    }

    #[test]
    fn test_claude_stats_cache_parse_empty_activity() {
        let json = r#"{
            "version": 1,
            "lastComputedDate": "2026-01-18",
            "dailyActivity": []
        }"#;

        let stats: ClaudeStatsCache = serde_json::from_str(json).unwrap();
        assert!(stats.daily_activity.is_empty());
    }

    #[test]
    fn test_claude_stats_cache_parse_malformed() {
        let json = r#"{ "version": 1, "lastComputedDate": "2026-01-18" "dailyActivity": [] }"#;
        let result: std::result::Result<ClaudeStatsCache, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // =========================================================================
    // CodexEvent Parsing Tests
    // =========================================================================

    #[test]
    fn test_codex_event_parse_full() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message", "payload": {"text": "Hello"}}"#;

        let event: CodexEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.timestamp, "2026-01-18T10:30:00Z");
        assert_eq!(event.event_type, "message");
        assert!(event.payload.is_some());
    }

    #[test]
    fn test_codex_event_parse_minimal() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "response"}"#;

        let event: CodexEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.timestamp, "2026-01-18T10:30:00Z");
        assert_eq!(event.event_type, "response");
        assert!(event.payload.is_none());
    }

    #[test]
    fn test_codex_event_date_extraction() {
        let json = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}"#;
        let event: CodexEvent = serde_json::from_str(json).unwrap();

        // Extract date from first 10 chars
        let date = event.timestamp.get(..10);
        assert_eq!(date, Some("2026-01-18"));
    }

    // =========================================================================
    // scan_codex_jsonl Tests
    // =========================================================================

    #[test]
    fn test_scan_codex_jsonl_valid_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message", "payload": {}}
{"timestamp": "2026-01-18T10:31:00Z", "type": "response", "payload": {}}
{"timestamp": "2026-01-17T14:00:00Z", "type": "message", "payload": {}}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();
        let cutoff_date = "2026-01-01";

        scanner.scan_codex_jsonl(&file_path, cutoff_date, &mut daily_counts);

        // Should have 2 dates
        assert_eq!(daily_counts.len(), 2);
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
        assert_eq!(daily_counts.get("2026-01-17").unwrap().events, 1);
    }

    #[test]
    fn test_scan_codex_jsonl_empty_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}

{"timestamp": "2026-01-18T10:31:00Z", "type": "response"}

"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip empty lines
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
    }

    #[test]
    fn test_scan_codex_jsonl_malformed_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}
not valid json at all
{"timestamp": "2026-01-18T10:31:00Z", "type": "response"}
{incomplete json"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip malformed lines, count only valid ones
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 2);
    }

    #[test]
    fn test_scan_codex_jsonl_100_line_limit() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        // Create 150 lines
        let mut content = String::new();
        for i in 0..150 {
            let _ = write!(
                content,
                r#"{{"timestamp": "2026-01-18T10:{:02}:00Z", "type": "message"}}"#,
                i % 60
            );
            content.push('\n');
        }

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_jsonl(&file_path, "2026-01-01", &mut daily_counts);

        // Should only read first 100 lines
        assert_eq!(daily_counts.get("2026-01-18").unwrap().events, 100);
    }

    #[test]
    fn test_scan_codex_jsonl_date_cutoff() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("session.jsonl");

        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}
{"timestamp": "2025-12-20T10:30:00Z", "type": "message"}
{"timestamp": "2025-06-01T10:30:00Z", "type": "message"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Cutoff is 2025-12-15, so 2026-01-18 and 2025-12-20 should pass, 2025-06-01 filtered out
        scanner.scan_codex_jsonl(&file_path, "2025-12-15", &mut daily_counts);

        assert_eq!(daily_counts.len(), 2); // 2026-01-18 and 2025-12-20
        assert!(daily_counts.contains_key("2026-01-18"));
        assert!(daily_counts.contains_key("2025-12-20"));
        assert!(!daily_counts.contains_key("2025-06-01")); // Before cutoff
    }

    #[test]
    fn test_scan_codex_jsonl_nonexistent_file() {
        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic on nonexistent file
        scanner.scan_codex_jsonl(
            &PathBuf::from("/nonexistent/path/file.jsonl"),
            "2026-01-01",
            &mut daily_counts,
        );

        assert!(daily_counts.is_empty());
    }

    // =========================================================================
    // scan_codex_history Tests
    // =========================================================================

    #[test]
    fn test_scan_codex_history_valid() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Unix timestamps for 2026-01-18 (~1768780800) and 2026-01-17 (~1768694400)
        // Using timestamps that are definitely in 2026
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "First message"}
{"session_id": "sess_001", "ts": 1768780860, "text": "Second message"}
{"session_id": "sess_002", "ts": 1768694400, "text": "Yesterday message"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should have entries (exact dates depend on timezone, but should have some)
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_malformed() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Using 2026 timestamps (~1768780800)
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "Valid"}
not valid json
{"session_id": "sess_002", "ts": 1768780900, "text": "Also valid"}"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic, should skip malformed lines
        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should have parsed the valid lines
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_empty_lines() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("history.jsonl");

        // Using 2026 timestamps (~1768780800)
        let content = r#"{"session_id": "sess_001", "ts": 1768780800, "text": "Valid"}

{"session_id": "sess_002", "ts": 1768780900, "text": "Also valid"}
"#;

        std::fs::write(&file_path, content).unwrap();

        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        scanner.scan_codex_history(&file_path, "2026-01-01", &mut daily_counts);

        // Should skip empty lines
        assert!(!daily_counts.is_empty());
    }

    #[test]
    fn test_scan_codex_history_nonexistent() {
        let scanner = CostScanner::new();
        let mut daily_counts: HashMap<String, DailyCount> = HashMap::new();

        // Should not panic on nonexistent file
        scanner.scan_codex_history(
            &PathBuf::from("/nonexistent/history.jsonl"),
            "2026-01-01",
            &mut daily_counts,
        );

        assert!(daily_counts.is_empty());
    }

    // =========================================================================
    // DailyCount Tests
    // =========================================================================

    #[test]
    fn test_daily_count_default() {
        let count = DailyCount::default();
        assert_eq!(count.events, 0);
    }

    #[test]
    fn test_daily_count_increment() {
        let mut count = DailyCount::default();
        count.events += 1;
        count.events += 1;
        assert_eq!(count.events, 2);
    }

    // =========================================================================
    // Date Filtering Tests
    // =========================================================================

    #[test]
    fn test_date_comparison() {
        // Test that string date comparison works correctly
        let cutoff = "2026-01-01";
        let recent_date = "2026-01-18";
        let old_date = "2025-12-01";

        assert!(recent_date >= cutoff);
        assert!((old_date < cutoff));
    }

    #[test]
    fn test_date_sorting() {
        let mut dates = vec!["2026-01-15", "2026-01-18", "2026-01-10", "2026-01-17"];
        dates.sort_by(|a, b| b.cmp(a)); // Descending

        assert_eq!(
            dates,
            vec!["2026-01-18", "2026-01-17", "2026-01-15", "2026-01-10"]
        );
    }

    // =========================================================================
    // Integration-style Tests (with temp directories)
    // =========================================================================

    #[test]
    fn test_codex_directory_structure() {
        let dir = TempDir::new().unwrap();
        let sessions_dir = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("01")
            .join("18");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let session_file = sessions_dir.join("session_abc.jsonl");
        let content = r#"{"timestamp": "2026-01-18T10:30:00Z", "type": "message"}"#;
        std::fs::write(&session_file, content).unwrap();

        // Verify structure was created
        assert!(session_file.exists());
    }

    #[test]
    fn test_cost_daily_entry_creation() {
        let entry = CostDailyEntry {
            date: "2026-01-18".to_string(),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            total_tokens: Some(42),
            total_cost: None,
            models_used: None,
        };

        assert_eq!(entry.date, "2026-01-18");
        assert_eq!(entry.total_tokens, Some(42));
    }

    // =========================================================================
    // Error Handling Tests
    // =========================================================================

    #[tokio::test]
    async fn test_scan_unsupported_provider() {
        let scanner = CostScanner::new();

        // Gemini doesn't support cost scanning
        let result = scanner.scan(Provider::Gemini, false).await;
        assert!(result.is_err());

        if let Err(CautError::Config(msg)) = result {
            assert!(msg.contains("does not support local cost scanning"));
        } else {
            panic!("Expected Config error");
        }
    }

    // =========================================================================
    // Claude Session Log Tests
    // =========================================================================

    /// 2026-11-20 12:00 UTC: the scan window starts on 2026-10-21.
    fn claude_scan_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[allow(clippy::too_many_arguments)]
    fn assistant_line(
        message_id: &str,
        request_id: &str,
        timestamp: &str,
        model: &str,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: (i64, i64),
    ) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": timestamp,
            "requestId": request_id,
            "sessionId": "s1",
            "message": {
                "id": message_id,
                "model": model,
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}],
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": cache_read,
                    "cache_creation_input_tokens": cache_creation.0 + cache_creation.1,
                    "cache_creation": {
                        "ephemeral_5m_input_tokens": cache_creation.0,
                        "ephemeral_1h_input_tokens": cache_creation.1
                    },
                    "service_tier": "standard",
                    "inference_geo": "global",
                    "speed": "standard"
                }
            }
        })
        .to_string()
    }

    fn write_lines(path: &Path, lines: &[String]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(path, body).unwrap();
    }

    fn assert_cost(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("cost should be known");
        assert!(
            (actual - expected).abs() < 1e-9,
            "cost {actual} != expected {expected}"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // one fixture tree, checked end to end
    fn claude_session_logs_give_daily_tokens_and_cost() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("claude");
        let project = config.join("projects").join("-work-app");

        // Today: one Opus 5.5 response written once per content block (the same message
        // and request id), plus noise that must not count.
        let opus = assistant_line(
            "msg_a",
            "req_a",
            "2026-11-20T08:00:00.000Z",
            "claude-opus-5-5",
            1_000,
            2_000,
            1_000_000,
            (0, 10_000),
        );
        let user_mentioning_usage = serde_json::json!({
            "type": "user",
            "timestamp": "2026-11-20T08:01:00.000Z",
            "message": {"role": "user", "content": "what does \"usage\" mean for an \"assistant\"?"}
        })
        .to_string();
        let synthetic = serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-11-20T08:02:00.000Z",
            "message": {
                "id": "synthetic-1",
                "model": "<synthetic>",
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        })
        .to_string();
        let outside_window = assistant_line(
            "msg_old",
            "req_old",
            "2026-10-01T00:00:00.000Z",
            "claude-opus-5-5",
            9_999,
            9_999,
            0,
            (0, 0),
        );
        let unpriced = assistant_line(
            "msg_u",
            "req_u",
            "2026-11-20T09:00:00.000Z",
            "claude-imaginary-9",
            100,
            100,
            0,
            (0, 0),
        );
        write_lines(
            &project.join("s1.jsonl"),
            &[
                opus.clone(),
                opus.clone(),
                user_mentioning_usage,
                "{not json \"usage\" \"assistant\"".to_string(),
                synthetic,
                outside_window,
                unpriced,
            ],
        );

        // Yesterday: a subagent transcript (5-minute cache writes, Sonnet 4.6 by dated id).
        write_lines(
            &project.join("s1").join("subagents").join("agent-1.jsonl"),
            &[assistant_line(
                "msg_b",
                "req_b",
                "2026-11-19T23:59:59.000Z",
                "claude-sonnet-4-6-20260217",
                500_000,
                100_000,
                0,
                (40_000, 0),
            )],
        );

        // A resumed session that repeats today's response in another file.
        write_lines(
            &config.join("projects").join("-work-other").join("s2.jsonl"),
            &[opus],
        );

        let scanner = CostScanner::new();
        let payload = scanner
            .scan_claude_dirs(&[config], claude_scan_now())
            .expect("session logs should be scanned");

        assert_eq!(payload.provider, "claude");
        let dates: Vec<&str> = payload.daily.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-11-20", "2026-11-19"]);

        // Today: msg_a once (the duplicates and the copy are dropped) plus the unpriced model.
        let today = &payload.daily[0];
        assert_eq!(today.input_tokens, Some(1_100));
        assert_eq!(today.output_tokens, Some(2_100));
        assert_eq!(today.cache_read_tokens, Some(1_000_000));
        assert_eq!(today.cache_creation_tokens, Some(10_000));
        assert_eq!(today.total_tokens, Some(1_013_200));
        // Opus 5.5: 1,000 x $4 + 2,000 x $20 + 1M cache reads x $0.20 + 10,000 1-hour
        // cache writes x $8 (per million) = $0.324. The unpriced model adds tokens only.
        assert_cost(today.total_cost, 0.324);
        assert_eq!(
            today.models_used.as_deref(),
            Some(
                &[
                    "claude-imaginary-9".to_string(),
                    "claude-opus-5-5".to_string()
                ][..]
            )
        );

        // Yesterday: Sonnet 4.6 500,000 x $3 + 100,000 x $15 + 40,000 5-minute cache
        // writes x $3.75 (per million) = $3.15.
        let yesterday = &payload.daily[1];
        assert_eq!(yesterday.total_tokens, Some(640_000));
        assert_cost(yesterday.total_cost, 3.15);
        assert_eq!(
            yesterday.models_used.as_deref(),
            Some(&["claude-sonnet-4-6".to_string()][..])
        );

        assert_eq!(payload.session_tokens, Some(1_013_200));
        assert_cost(payload.session_cost_usd, 0.324);
        assert_eq!(payload.last_30_days_tokens, Some(1_653_200));
        assert_cost(payload.last_30_days_cost_usd, 3.474);
        let totals = payload.totals.expect("totals");
        assert_eq!(totals.input_tokens, Some(501_100));
        assert_eq!(totals.output_tokens, Some(102_100));
        assert_eq!(totals.cache_creation_tokens, Some(50_000));
    }

    #[test]
    fn claude_session_logs_with_no_recent_usage_report_zero() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("claude");
        write_lines(
            &config.join("projects").join("-work-app").join("s1.jsonl"),
            &[assistant_line(
                "msg_old",
                "req_old",
                "2026-09-01T00:00:00.000Z",
                "claude-opus-4-7",
                10,
                10,
                0,
                (0, 0),
            )],
        );

        let payload = CostScanner::new()
            .scan_claude_dirs(&[config], claude_scan_now())
            .expect("an existing but quiet log directory is a real zero");
        assert!(payload.daily.is_empty());
        assert_eq!(payload.session_tokens, Some(0));
        assert_eq!(payload.last_30_days_tokens, Some(0));
        assert_cost(payload.last_30_days_cost_usd, 0.0);
    }

    #[test]
    fn claude_without_any_source_is_an_error_not_zero_usage() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("claude");
        std::fs::create_dir_all(config.join("projects")).unwrap();

        let err = CostScanner::new()
            .scan_claude_dirs(&[config], claude_scan_now())
            .expect_err("no session logs and no stats cache must be reported");
        let message = err.to_string();
        assert!(
            message.contains("no Claude usage data found"),
            "unexpected error: {message}"
        );
        assert!(message.contains("stats-cache.json"), "{message}");
    }

    #[test]
    fn claude_stats_cache_fallback_reports_tokens_not_message_counts() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("claude");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("stats-cache.json"),
            r#"{
                "version": 5,
                "lastComputedDate": "2026-11-20",
                "dailyActivity": [
                    {"date": "2026-11-20", "messageCount": 104, "sessionCount": 1, "toolCallCount": 30},
                    {"date": "2026-11-18", "messageCount": 7, "sessionCount": 1, "toolCallCount": 2},
                    {"date": "2026-09-01", "messageCount": 5, "sessionCount": 1, "toolCallCount": 1}
                ],
                "dailyModelTokens": [
                    {"date": "2026-11-20", "tokensByModel": {"claude-opus-4-7": 1549497, "claude-haiku-4-5": 3}}
                ]
            }"#,
        )
        .unwrap();

        let payload = CostScanner::new()
            .scan_claude_dirs(&[config], claude_scan_now())
            .expect("stats cache fallback");

        let dates: Vec<&str> = payload.daily.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(dates, vec!["2026-11-20", "2026-11-18"]);
        assert_eq!(payload.daily[0].total_tokens, Some(1_549_500));
        // A day with only a message count has no token count.
        assert_eq!(payload.daily[1].total_tokens, None);
        assert_eq!(payload.session_tokens, Some(1_549_500));
        assert_eq!(payload.last_30_days_tokens, Some(1_549_500));
        assert_eq!(payload.last_30_days_cost_usd, None);
    }
}
