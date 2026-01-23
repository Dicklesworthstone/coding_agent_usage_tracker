//! Session log discovery and parsing.
//!
//! Provides discovery of provider session logs and parsers for extracting
//! per-session usage totals from JSONL files.

use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Discovered session log file with metadata.
#[derive(Debug, Clone)]
pub struct SessionLogPath {
    pub provider: Provider,
    pub path: PathBuf,
    pub project_path: Option<PathBuf>,
    pub session_id: String,
    pub modified_at: Option<DateTime<Utc>>,
}

impl SessionLogPath {
    fn new(provider: Provider, path: PathBuf, project_path: Option<PathBuf>) -> Self {
        let session_id = session_id_from_path(&path);
        let modified_at = file_modified_at(&path);
        Self {
            provider,
            path,
            project_path,
            session_id,
            modified_at,
        }
    }
}

/// Session log discovery for supported providers.
pub struct SessionLogFinder {
    claude_base: PathBuf,
    codex_base: PathBuf,
}

impl SessionLogFinder {
    /// Create a new finder using default home-based paths.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| CautError::Config("Cannot determine home directory".to_string()))?;
        Ok(Self {
            claude_base: home.join(".claude"),
            codex_base: home.join(".codex"),
        })
    }

    /// Create a finder with explicit base paths (useful for tests).
    #[must_use]
    pub fn with_paths(claude_base: PathBuf, codex_base: PathBuf) -> Self {
        Self {
            claude_base,
            codex_base,
        }
    }

    /// Find session logs for a provider, optionally filtering by modified time.
    #[must_use]
    pub fn find_sessions(
        &self,
        provider: Provider,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Vec<SessionLogPath> {
        match provider {
            Provider::Claude => self.find_claude_sessions(since, until),
            Provider::Codex => self.find_codex_sessions(since, until),
            _ => Vec::new(),
        }
    }

    fn find_claude_sessions(
        &self,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Vec<SessionLogPath> {
        let mut results = Vec::new();
        let projects_dir = self.claude_base.join("projects");
        if !projects_dir.exists() {
            return results;
        }

        if let Ok(projects) = fs::read_dir(&projects_dir) {
            for project_entry in projects.flatten() {
                let project_path = project_entry.path();
                if !project_path.is_dir() {
                    continue;
                }

                let conversations_dir = project_path.join("conversations");
                if !conversations_dir.exists() {
                    continue;
                }

                if let Ok(files) = fs::read_dir(&conversations_dir) {
                    for file_entry in files.flatten() {
                        let path = file_entry.path();
                        if !is_jsonl_file(&path) {
                            continue;
                        }

                        let log =
                            SessionLogPath::new(Provider::Claude, path, Some(project_path.clone()));
                        if within_range(log.modified_at, since, until) {
                            results.push(log);
                        }
                    }
                }
            }
        }

        sort_logs_by_mtime(&mut results);
        results
    }

    fn find_codex_sessions(
        &self,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Vec<SessionLogPath> {
        let mut results = Vec::new();
        let sessions_dir = self.codex_base.join("sessions");
        if !sessions_dir.exists() {
            return results;
        }

        let files = collect_jsonl_files(&sessions_dir);
        for path in files {
            let log = SessionLogPath::new(Provider::Codex, path, None);
            if within_range(log.modified_at, since, until) {
                results.push(log);
            }
        }

        sort_logs_by_mtime(&mut results);
        results
    }
}

/// Usage totals parsed from a single session log.
#[derive(Debug, Default)]
pub struct SessionUsage {
    pub session_id: String,
    pub project_path: Option<PathBuf>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub models_used: HashSet<String>,
    pub message_count: i64,
}

impl SessionUsage {
    fn record_timestamp(&mut self, ts: DateTime<Utc>) {
        self.started_at = Some(self.started_at.map_or(ts, |cur| cur.min(ts)));
        self.ended_at = Some(self.ended_at.map_or(ts, |cur| cur.max(ts)));
    }
}

/// Parser for Claude Code session logs.
pub struct ClaudeSessionParser;

impl ClaudeSessionParser {
    /// Parse a Claude session log JSONL file.
    pub fn parse(&self, path: &Path) -> Result<SessionUsage> {
        let mut usage = parse_session_log(path)?;
        if usage.project_path.is_none() {
            usage.project_path = extract_claude_project_path(path);
        }
        Ok(usage)
    }
}

/// Parser for Codex CLI session logs.
pub struct CodexSessionParser;

impl CodexSessionParser {
    /// Parse a Codex session log JSONL file.
    pub fn parse(&self, path: &Path) -> Result<SessionUsage> {
        parse_session_log(path)
    }
}

fn parse_session_log(path: &Path) -> Result<SessionUsage> {
    let file = File::open(path)
        .map_err(|e| CautError::Config(format!("Failed to open session log: {}", e)))?;
    let reader = BufReader::new(file);

    let mut usage = SessionUsage {
        session_id: session_id_from_path(path),
        project_path: None,
        started_at: None,
        ended_at: None,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        models_used: HashSet::new(),
        message_count: 0,
    };

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        usage.message_count += 1;

        if let Some(ts) = extract_timestamp(&value) {
            usage.record_timestamp(ts);
        }

        if let Some(model) = extract_model(&value) {
            if !model.is_empty() {
                usage.models_used.insert(model.to_string());
            }
        }

        for usage_value in extract_usage_candidates(&value) {
            if let Some(counts) = extract_token_counts(usage_value) {
                usage.input_tokens += counts.input;
                usage.output_tokens += counts.output;
                usage.cache_read_tokens += counts.cache_read;
                usage.cache_creation_tokens += counts.cache_creation;
            }
        }
    }

    Ok(usage)
}

#[derive(Default)]
struct TokenCounts {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
}

fn extract_token_counts(value: &Value) -> Option<TokenCounts> {
    if !value.is_object() {
        return None;
    }

    let input = first_i64(value, &["input_tokens", "prompt_tokens"]).unwrap_or(0);
    let output = first_i64(value, &["output_tokens", "completion_tokens"]).unwrap_or(0);
    let cache_read =
        first_i64(value, &["cache_read_input_tokens", "cache_read_tokens"]).unwrap_or(0);
    let cache_creation = first_i64(
        value,
        &["cache_creation_input_tokens", "cache_creation_tokens"],
    )
    .unwrap_or(0);

    Some(TokenCounts {
        input,
        output,
        cache_read,
        cache_creation,
    })
}

fn extract_usage_candidates<'a>(value: &'a Value) -> Vec<&'a Value> {
    let mut candidates = Vec::new();

    let usage_keys = ["usage", "token_usage", "usage_stats", "tokens"];

    for key in usage_keys {
        if let Some(usage) = value.get(key) {
            candidates.push(usage);
        }
    }

    for container_key in ["message", "response", "payload"] {
        if let Some(container) = value.get(container_key) {
            for key in usage_keys {
                if let Some(usage) = container.get(key) {
                    candidates.push(usage);
                }
            }
        }
    }

    candidates
}

fn extract_model(value: &Value) -> Option<&str> {
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        return Some(model);
    }

    for container_key in ["message", "response", "payload"] {
        if let Some(model) = value
            .get(container_key)
            .and_then(|v| v.get("model"))
            .and_then(Value::as_str)
        {
            return Some(model);
        }
    }

    None
}

fn extract_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let ts_keys = ["timestamp", "created_at", "createdAt", "ts", "time"];

    for key in ts_keys {
        if let Some(ts) = value.get(key).and_then(parse_timestamp) {
            return Some(ts);
        }
    }

    for container_key in ["message", "response", "payload"] {
        if let Some(container) = value.get(container_key) {
            for key in ts_keys {
                if let Some(ts) = container.get(key).and_then(parse_timestamp) {
                    return Some(ts);
                }
            }
        }
    }

    None
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc)),
        Value::Number(num) => {
            let raw = num.as_i64().or_else(|| num.as_u64().map(|u| u as i64))?;
            let (secs, nanos) = if raw > 1_000_000_000_000 {
                let secs = raw / 1000;
                let millis = (raw % 1000) as u32;
                (secs, millis * 1_000_000)
            } else {
                (raw, 0)
            };
            DateTime::from_timestamp(secs, nanos)
        }
        _ => None,
    }
}

fn first_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(num) = v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)) {
                return Some(num);
            }
        }
    }
    None
}

fn is_jsonl_file(path: &Path) -> bool {
    path.extension().map_or(false, |ext| ext == "jsonl")
}

fn collect_jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_jsonl_file(&path) {
                results.push(path);
            }
        }
    }

    results
}

fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "unknown".to_string())
}

fn extract_claude_project_path(path: &Path) -> Option<PathBuf> {
    let conversations_dir = path.parent()?;
    if conversations_dir.file_name()?.to_str()? != "conversations" {
        return None;
    }
    conversations_dir.parent().map(|p| p.to_path_buf())
}

fn file_modified_at(path: &Path) -> Option<DateTime<Utc>> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    system_time_to_utc(modified)
}

fn system_time_to_utc(time: SystemTime) -> Option<DateTime<Utc>> {
    Some(DateTime::<Utc>::from(time))
}

fn within_range(
    modified_at: Option<DateTime<Utc>>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> bool {
    if let Some(modified) = modified_at {
        if let Some(since) = since {
            if modified < since {
                return false;
            }
        }
        if let Some(until) = until {
            if modified > until {
                return false;
            }
        }
    }
    true
}

fn sort_logs_by_mtime(entries: &mut [SessionLogPath]) {
    entries.sort_by(|a, b| {
        let by_time = b.modified_at.cmp(&a.modified_at);
        if by_time == std::cmp::Ordering::Equal {
            a.path.cmp(&b.path)
        } else {
            by_time
        }
    });
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
    use tempfile::TempDir;

    #[test]
    fn find_claude_sessions_discovers_conversations() {
        let temp = TempDir::new().unwrap();
        let claude_base = temp.path().join(".claude");
        let project = claude_base.join("projects").join("proj1");
        let conversations = project.join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        let log_path = conversations.join("session_abc.jsonl");
        std::fs::write(&log_path, "{}\n").unwrap();

        let finder = SessionLogFinder::with_paths(claude_base, temp.path().join(".codex"));
        let logs = finder.find_sessions(Provider::Claude, None, None);

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].session_id, "session_abc");
        assert_eq!(
            logs[0].project_path.as_ref().unwrap().file_name().unwrap(),
            "proj1"
        );
    }

    #[test]
    fn find_codex_sessions_recurses_directories() {
        let temp = TempDir::new().unwrap();
        let codex_base = temp.path().join(".codex");
        let sessions = codex_base
            .join("sessions")
            .join("2026")
            .join("01")
            .join("18");
        std::fs::create_dir_all(&sessions).unwrap();
        let log_path = sessions.join("session_xyz.jsonl");
        std::fs::write(&log_path, "{}\n").unwrap();

        let finder = SessionLogFinder::with_paths(temp.path().join(".claude"), codex_base);
        let logs = finder.find_sessions(Provider::Codex, None, None);

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].session_id, "session_xyz");
    }

    #[test]
    fn parse_claude_session_log_extracts_tokens_and_models() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("claude.jsonl");
        let content = r#"{"model":"claude-3-opus","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":5,"cache_creation_input_tokens":2},"created_at":"2026-01-18T12:00:00Z"}
{"message":{"model":"claude-3-sonnet","usage":{"prompt_tokens":25,"completion_tokens":10}},"timestamp":"2026-01-18T12:05:00Z"}
malformed-line
"#;
        std::fs::write(&log_path, content).unwrap();

        let parser = ClaudeSessionParser;
        let usage = parser.parse(&log_path).unwrap();

        assert_eq!(usage.input_tokens, 125);
        assert_eq!(usage.output_tokens, 60);
        assert_eq!(usage.cache_read_tokens, 5);
        assert_eq!(usage.cache_creation_tokens, 2);
        assert!(usage.models_used.contains("claude-3-opus"));
        assert!(usage.models_used.contains("claude-3-sonnet"));
        assert_eq!(usage.message_count, 2);
        assert!(usage.started_at.is_some());
        assert!(usage.ended_at.is_some());
    }

    #[test]
    fn parse_codex_session_log_extracts_tokens() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("codex.jsonl");
        let content = r#"{"timestamp":"2026-01-18T10:30:00Z","type":"message","payload":{"usage":{"input_tokens":120,"output_tokens":30},"model":"gpt-4.1"}}
{"timestamp":1768782000,"type":"response","usage":{"prompt_tokens":10,"completion_tokens":5,"cache_read_tokens":2}}
"#;
        std::fs::write(&log_path, content).unwrap();

        let parser = CodexSessionParser;
        let usage = parser.parse(&log_path).unwrap();

        assert_eq!(usage.input_tokens, 130);
        assert_eq!(usage.output_tokens, 35);
        assert_eq!(usage.cache_read_tokens, 2);
        assert!(usage.models_used.contains("gpt-4.1"));
        assert_eq!(usage.message_count, 2);
    }

    #[test]
    fn parse_session_log_handles_empty_file() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("empty.jsonl");
        std::fs::write(&log_path, "\n\n").unwrap();

        let parser = CodexSessionParser;
        let usage = parser.parse(&log_path).unwrap();

        assert_eq!(usage.message_count, 0);
        assert_eq!(usage.input_tokens, 0);
    }
}
