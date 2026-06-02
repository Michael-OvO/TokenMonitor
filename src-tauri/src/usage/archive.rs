// ─────────────────────────────────────────────────────────────────────────────
// Usage archive — persistent hourly aggregate storage
//
// Prevents data loss when source JSONL files (local .claude/ or remote servers)
// are deleted or become inaccessible. Completed hours are archived into compact
// per-month JSONL files under {app_data_dir}/usage-archive/.
//
// Storage layout:
//   {app_data_dir}/usage-archive/
//   ├── local/
//   │   ├── claude/2026-04.jsonl
//   │   └── codex/2026-04.jsonl
//   ├── devices/
//   │   └── {alias}/2026-04.jsonl
//   └── .archive-state.json
//
// Dedup strategy: time-boundary partitioning.
// Archive covers hours [0..frontier]. Live source covers (frontier..now].
// Zero overlap = zero double counting.
// ─────────────────────────────────────────────────────────────────────────────

use super::parser::ParsedEntry;
use chrono::{Datelike, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// Archived hourly aggregate record
// ─────────────────────────────────────────────────────────────────────────────

/// Compact per-hour per-model aggregate. ~100 bytes per JSON line.
/// Cost is NOT stored — recalculated at query time with current pricing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArchivedHourly {
    /// Date: "2026-04-11"
    pub d: String,
    /// Hour: 0-23
    pub h: u8,
    /// Model key (normalized, e.g. "sonnet-4-6")
    pub mk: String,
    /// Model display name (e.g. "Sonnet 4.6")
    pub mn: String,
    /// Input tokens
    #[serde(rename = "in")]
    pub input_tokens: u64,
    /// Output tokens
    pub out: u64,
    /// Cache creation 5m tokens
    pub c5: u64,
    /// Cache creation 1h tokens
    pub c1: u64,
    /// Cache read tokens
    pub cr: u64,
    /// Web search requests
    pub ws: u64,
    /// Provider: "claude" or "codex"
    pub p: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Archive state — tracks last archived hour per source
// ─────────────────────────────────────────────────────────────────────────────

const STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ArchiveState {
    version: u32,
    /// Maps source key → last archived hour as "YYYY-MM-DDTHH"
    /// e.g. "local:claude" → "2026-04-11T14"
    sources: HashMap<String, String>,
}

impl Default for ArchiveState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            sources: HashMap::new(),
        }
    }
}

/// Parsed archive frontier: (date, hour) inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveFrontier {
    pub date: NaiveDate,
    pub hour: u8,
}

impl ArchiveFrontier {
    /// Returns true if (entry_date, entry_hour) is within the archived range,
    /// i.e. at or before this frontier.
    pub fn covers(&self, entry_date: NaiveDate, entry_hour: u8) -> bool {
        entry_date < self.date || (entry_date == self.date && entry_hour <= self.hour)
    }

    /// Returns true if nothing new can be archived — frontier is already
    /// at the hour before (current_date, current_hour), or somehow ahead.
    pub fn is_up_to_date(&self, current_date: NaiveDate, current_hour: u8) -> bool {
        let (prev_date, prev_hour) = if current_hour == 0 {
            (current_date.pred_opt().unwrap_or(current_date), 23)
        } else {
            (current_date, current_hour - 1)
        };
        // Already at prev_hour, or ahead of current.
        (self.date == prev_date && self.hour == prev_hour)
            || self.date > current_date
            || (self.date == current_date && self.hour >= current_hour)
    }
}

impl ArchiveState {
    fn get_frontier(&self, source_key: &str) -> Option<ArchiveFrontier> {
        let s = self.sources.get(source_key)?;
        // Parse "2026-04-11T14"
        let (date_str, hour_str) = s.split_once('T')?;
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let hour: u8 = hour_str.parse().ok()?;
        Some(ArchiveFrontier { date, hour })
    }

    fn set_frontier(&mut self, source_key: &str, frontier: ArchiveFrontier) {
        self.sources.insert(
            source_key.to_string(),
            format!("{}T{:02}", frontier.date.format("%Y-%m-%d"), frontier.hour),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ArchiveManager
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ArchiveManager {
    base_dir: PathBuf,
}

impl ArchiveManager {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            base_dir: app_data_dir.join("usage-archive"),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.base_dir.join(".archive-state.json")
    }

    fn load_state(&self) -> ArchiveState {
        match fs::read_to_string(self.state_path()) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => ArchiveState::default(),
        }
    }

    fn save_state(&self, state: &ArchiveState) {
        if let Err(e) = atomic_write_json(&self.state_path(), state) {
            tracing::warn!("Failed to save archive state: {e}");
        }
    }

    /// Get the archive frontier (inclusive) for a source.
    /// Returns None if no data has been archived for this source.
    pub fn frontier(&self, source_key: &str) -> Option<ArchiveFrontier> {
        self.load_state().get_frontier(source_key)
    }

    /// Archive completed hours from parsed entries.
    ///
    /// Only archives hours strictly before `current_hour` on `current_date`.
    /// Hours already covered by the frontier are skipped.
    /// Returns the number of aggregate records written.
    pub fn archive_completed_hours(
        &self,
        entries: &[ParsedEntry],
        source_key: &str,
        provider: &str,
        current_date: NaiveDate,
        current_hour: u8,
    ) -> usize {
        if entries.is_empty() {
            return 0;
        }

        let mut state = self.load_state();
        let frontier = state.get_frontier(source_key);

        // Aggregate entries into (date, hour, model_key) buckets,
        // skipping already-archived and not-yet-completed hours.
        let mut aggregates: HashMap<(String, u8, String), ArchivedHourly> =
            HashMap::with_capacity(entries.len().min(256));

        for entry in entries {
            let entry_date = entry.timestamp.date_naive();
            let entry_hour = entry.timestamp.hour() as u8;

            // Skip if this hour is not yet completed (at or after current hour today).
            if entry_date > current_date
                || (entry_date == current_date && entry_hour >= current_hour)
            {
                continue;
            }

            // Skip if already covered by the frontier.
            if let Some(ref f) = frontier {
                if f.covers(entry_date, entry_hour) {
                    continue;
                }
            }

            let known = crate::models::known_model_from_raw(&entry.model);
            let date_str = entry_date.format("%Y-%m-%d").to_string();

            let key = (date_str.clone(), entry_hour, known.model_key.clone());
            let agg = aggregates.entry(key).or_insert(ArchivedHourly {
                d: date_str,
                h: entry_hour,
                mk: known.model_key,
                mn: known.display_name,
                input_tokens: 0,
                out: 0,
                c5: 0,
                c1: 0,
                cr: 0,
                ws: 0,
                p: provider.to_string(),
            });

            agg.input_tokens += entry.input_tokens;
            agg.out += entry.output_tokens;
            agg.c5 += entry.cache_creation_5m_tokens;
            agg.c1 += entry.cache_creation_1h_tokens;
            agg.cr += entry.cache_read_tokens;
            agg.ws += entry.web_search_requests;
        }

        if aggregates.is_empty() {
            return 0;
        }

        // Group by YYYY-MM for file output.
        let mut by_month: HashMap<String, Vec<&ArchivedHourly>> = HashMap::new();
        for agg in aggregates.values() {
            let month_key = agg.d.get(..7).unwrap_or(&agg.d); // "2026-04"
            by_month.entry(month_key.to_string()).or_default().push(agg);
        }

        // Ensure source directory exists.
        let source_dir = self.source_dir(source_key);
        if let Err(e) = fs::create_dir_all(&source_dir) {
            tracing::warn!("Failed to create archive dir {source_dir:?}: {e}");
            return 0;
        }

        // Append to monthly files.
        let mut count = 0;
        for (month_key, records) in &by_month {
            let file_path = source_dir.join(format!("{month_key}.jsonl"));
            let mut lines = String::new();
            for record in records {
                match serde_json::to_string(record) {
                    Ok(line) => {
                        lines.push_str(&line);
                        lines.push('\n');
                        count += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to serialize archive record: {e}");
                    }
                }
            }
            if let Err(e) = append_to_file(&file_path, lines.as_bytes()) {
                tracing::warn!("Failed to append to archive {file_path:?}: {e}");
            }
        }

        // Update frontier to the max (date, hour) we just archived.
        let new_frontier = aggregates
            .keys()
            .filter_map(|(d, h, _)| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .ok()
                    .map(|date| ArchiveFrontier { date, hour: *h })
            })
            .max_by_key(|f| (f.date, f.hour));

        if let Some(new_f) = new_frontier {
            let should_update = frontier
                .map(|old_f| {
                    new_f.date > old_f.date || (new_f.date == old_f.date && new_f.hour > old_f.hour)
                })
                .unwrap_or(true);
            if should_update {
                state.set_frontier(source_key, new_f);
                self.save_state(&state);
            }
        }

        count
    }

    /// Load archived data for a source within a date range.
    ///
    /// Returns synthetic `ParsedEntry` objects that can be merged with live data.
    /// `since`: only load records on or after this date.
    pub fn load_archived(&self, source_key: &str, since: Option<NaiveDate>) -> Vec<ParsedEntry> {
        let source_dir = self.source_dir(source_key);
        if !source_dir.exists() {
            return Vec::new();
        }

        // Find relevant monthly JSONL files.
        let files: Vec<PathBuf> = match fs::read_dir(&source_dir) {
            Ok(dir_entries) => dir_entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
                .collect(),
            Err(_) => return Vec::new(),
        };

        // Optionally skip monthly files that are entirely before `since`.
        // File name format: "2026-04.jsonl" → month starts at "2026-04-01".
        let relevant_files: Vec<&PathBuf> = files
            .iter()
            .filter(|path| {
                let dominated_by_since = since.and_then(|s| {
                    let stem = path.file_stem()?.to_str()?;
                    // Parse "2026-04" → last day is at most 2026-04-30.
                    let month_start =
                        NaiveDate::parse_from_str(&format!("{stem}-01"), "%Y-%m-%d").ok()?;
                    // Month end: go forward one month and back one day.
                    let next_month = if month_start.month() == 12 {
                        NaiveDate::from_ymd_opt(month_start.year() + 1, 1, 1)
                    } else {
                        NaiveDate::from_ymd_opt(month_start.year(), month_start.month() + 1, 1)
                    };
                    let month_end = next_month?.pred_opt()?;
                    Some(month_end < s)
                });
                // If we couldn't parse or since is None, include the file.
                !dominated_by_since.unwrap_or(false)
            })
            .collect();

        let mut entries = Vec::with_capacity(128);
        for file_path in relevant_files {
            let file = match fs::File::open(file_path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            for line in BufReader::new(file).lines() {
                let line = match line {
                    Ok(l) if !l.trim().is_empty() => l,
                    _ => continue,
                };
                let record: ArchivedHourly = match serde_json::from_str(&line) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping malformed archive record in {}: {e}",
                            file_path.display()
                        );
                        continue;
                    }
                };

                let record_date = match NaiveDate::parse_from_str(&record.d, "%Y-%m-%d") {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                // Filter by since date.
                if since.is_some_and(|s| record_date < s) {
                    continue;
                }

                // Convert to synthetic ParsedEntry.
                // Use model_key as model field — normalize_model() recognizes it.
                let time = NaiveTime::from_hms_opt(record.h as u32, 0, 0).unwrap_or_default();
                let naive_dt = record_date.and_time(time);
                let timestamp = Local
                    .from_local_datetime(&naive_dt)
                    .single()
                    .unwrap_or_else(Local::now);

                entries.push(ParsedEntry {
                    timestamp,
                    model: record.mk,
                    input_tokens: record.input_tokens,
                    output_tokens: record.out,
                    cache_creation_5m_tokens: record.c5,
                    cache_creation_1h_tokens: record.c1,
                    cache_read_tokens: record.cr,
                    web_search_requests: record.ws,
                    unique_hash: None,
                    session_key: format!("archive:{source_key}"),
                    agent_scope: crate::stats::subagent::AgentScope::Main,
                });
            }
        }

        entries
    }

    /// Resolve the directory for a given source key.
    ///
    /// Source key format:
    /// - `"local:claude"` → `usage-archive/local/claude/`
    /// - `"local:codex"` → `usage-archive/local/codex/`
    /// - `"device:{alias}"` → `usage-archive/devices/{alias}/`
    pub fn reset(&self) {
        if self.base_dir.exists() {
            if let Err(e) = fs::remove_dir_all(&self.base_dir) {
                tracing::warn!("Failed to remove archive dir {:?}: {e}", self.base_dir);
            }
        }
    }

    fn source_dir(&self, source_key: &str) -> PathBuf {
        match source_key.split_once(':') {
            Some(("local", provider)) => self.base_dir.join("local").join(provider),
            Some(("device", alias)) => self.base_dir.join("devices").join(alias),
            _ => self.base_dir.join("other").join(source_key),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// File I/O helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Atomic write via temp file + rename.
fn atomic_write_json<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("serialize: {e}"))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, json.as_bytes()).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp_path, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Append data to a file, creating it if it doesn't exist.
fn append_to_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open: {e}"))?;
    file.write_all(data).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_entry(date: &str, hour: u32, model: &str, input: u64, output: u64) -> ParsedEntry {
        let naive_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        let time = NaiveTime::from_hms_opt(hour, 30, 0).unwrap();
        let naive_dt = naive_date.and_time(time);
        let timestamp = Local.from_local_datetime(&naive_dt).single().unwrap();
        ParsedEntry {
            timestamp,
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cache_read_tokens: 0,
            web_search_requests: 0,
            unique_hash: None,
            session_key: "test-session".to_string(),
            agent_scope: crate::stats::subagent::AgentScope::Main,
        }
    }

    #[test]
    fn archives_completed_hours_and_skips_current() {
        let tmp = TempDir::new().unwrap();
        let mgr = ArchiveManager::new(tmp.path());

        let entries = vec![
            make_entry("2026-04-11", 10, "claude-sonnet-4-6-20260301", 1000, 500),
            make_entry("2026-04-11", 10, "claude-sonnet-4-6-20260301", 2000, 800),
            make_entry("2026-04-11", 11, "claude-opus-4-6-20260301", 500, 200),
            make_entry("2026-04-11", 14, "claude-sonnet-4-6-20260301", 100, 50), // current hour
        ];

        let current_date = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let count =
            mgr.archive_completed_hours(&entries, "local:claude", "claude", current_date, 14);

        // Should archive hours 10 and 11, but not 14 (current hour).
        assert_eq!(count, 2); // 2 aggregate records: (h10, sonnet-4-6) + (h11, opus-4-6)

        // Verify frontier.
        let frontier = mgr.frontier("local:claude").unwrap();
        assert_eq!(frontier.date, current_date);
        assert_eq!(frontier.hour, 11);

        // Verify archived data is loadable.
        let loaded = mgr.load_archived("local:claude", None);
        assert_eq!(loaded.len(), 2);

        // Hour 10 sonnet should have aggregated tokens.
        let h10 = loaded.iter().find(|e| e.timestamp.hour() == 10).unwrap();
        assert_eq!(h10.input_tokens, 3000); // 1000 + 2000
        assert_eq!(h10.output_tokens, 1300); // 500 + 800
    }

    #[test]
    fn does_not_re_archive_already_archived_hours() {
        let tmp = TempDir::new().unwrap();
        let mgr = ArchiveManager::new(tmp.path());

        let entries = vec![make_entry(
            "2026-04-11",
            10,
            "claude-sonnet-4-6-20260301",
            1000,
            500,
        )];

        let current_date = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let count =
            mgr.archive_completed_hours(&entries, "local:claude", "claude", current_date, 14);
        assert_eq!(count, 1);

        // Archive again — should write 0 because hour 10 is already archived.
        let count =
            mgr.archive_completed_hours(&entries, "local:claude", "claude", current_date, 14);
        assert_eq!(count, 0);
    }

    #[test]
    fn load_archived_filters_by_since_date() {
        let tmp = TempDir::new().unwrap();
        let mgr = ArchiveManager::new(tmp.path());

        let entries = vec![
            make_entry("2026-04-10", 10, "claude-sonnet-4-6-20260301", 1000, 500),
            make_entry("2026-04-11", 10, "claude-sonnet-4-6-20260301", 2000, 800),
        ];

        let current_date = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        mgr.archive_completed_hours(&entries, "local:claude", "claude", current_date, 14);

        let since = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let loaded = mgr.load_archived("local:claude", Some(since));

        // Should only return the April 11 entry.
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].input_tokens, 2000);
    }

    #[test]
    fn frontier_covers_correctly() {
        let f = ArchiveFrontier {
            date: NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(),
            hour: 14,
        };

        // Same date, earlier hour → covered.
        assert!(f.covers(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 10));
        // Same date, same hour → covered.
        assert!(f.covers(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 14));
        // Same date, later hour → NOT covered.
        assert!(!f.covers(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 15));
        // Earlier date → covered.
        assert!(f.covers(NaiveDate::from_ymd_opt(2026, 4, 10).unwrap(), 23));
        // Later date → NOT covered.
        assert!(!f.covers(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap(), 0));
    }

    #[test]
    fn is_up_to_date_at_prev_hour() {
        let f = ArchiveFrontier {
            date: NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(),
            hour: 13,
        };
        // Frontier at hour 13, current hour 14 → up to date (13 == 14-1).
        assert!(f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 14));
        // Frontier at hour 13, current hour 15 → NOT up to date (hour 14 missing).
        assert!(!f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 15));
        // Frontier at hour 13 today, current hour 0 tomorrow → NOT up to date.
        assert!(!f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap(), 0));
    }

    #[test]
    fn is_up_to_date_midnight_boundary() {
        let f = ArchiveFrontier {
            date: NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(),
            hour: 23,
        };
        // Frontier at hour 23, current hour 0 next day → up to date.
        assert!(f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap(), 0));
        // Frontier at hour 23, current hour 1 next day → NOT up to date.
        assert!(!f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap(), 1));
    }

    #[test]
    fn is_up_to_date_ahead_of_current() {
        let f = ArchiveFrontier {
            date: NaiveDate::from_ymd_opt(2026, 4, 12).unwrap(),
            hour: 10,
        };
        // Frontier is a day ahead → up to date.
        assert!(f.is_up_to_date(NaiveDate::from_ymd_opt(2026, 4, 11).unwrap(), 14));
    }

    #[test]
    fn empty_entries_returns_zero() {
        let tmp = TempDir::new().unwrap();
        let mgr = ArchiveManager::new(tmp.path());
        let current_date = NaiveDate::from_ymd_opt(2026, 4, 11).unwrap();
        let count = mgr.archive_completed_hours(&[], "local:claude", "claude", current_date, 14);
        assert_eq!(count, 0);
    }

    #[test]
    fn load_from_nonexistent_source_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let mgr = ArchiveManager::new(tmp.path());
        let loaded = mgr.load_archived("local:claude", None);
        assert!(loaded.is_empty());
    }
}
