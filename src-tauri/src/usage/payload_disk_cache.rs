use std::fs;
use std::path::{Path, PathBuf};

use crate::models::UsagePayload;

const CACHE_DIR_NAME: &str = "payload_cache";

pub struct PayloadDiskCache {
    dir: PathBuf,
}

impl PayloadDiskCache {
    pub fn new(app_data_dir: &Path) -> Self {
        let dir = app_data_dir.join(CACHE_DIR_NAME);
        fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    pub fn save(&self, key: &str, payload: &UsagePayload) -> bool {
        let path = self.path_for(key);
        let json = match serde_json::to_vec(payload) {
            Ok(j) => j,
            Err(_) => return false,
        };
        // Skip write if content is identical (avoids unnecessary disk IO during warmup).
        if let Ok(existing) = fs::read(&path) {
            if existing == json {
                return false;
            }
        }
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, &json).is_ok() {
            fs::rename(&tmp, &path).ok();
            return true;
        }
        false
    }

    pub fn load(&self, key: &str) -> Option<UsagePayload> {
        let path = self.path_for(key);
        let data = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn clear_all(&self) {
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                    fs::remove_file(entry.path()).ok();
                }
            }
        }
    }

    pub fn clear_prefix(&self, prefix: &str) {
        let safe_prefix: String = prefix
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name.starts_with(&safe_prefix) {
                    fs::remove_file(&path).ok();
                }
            }
        }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let safe_name: String = key
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe_name}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let cache = PayloadDiskCache::new(tmp.path());
        let payload = UsagePayload::default();
        cache.save("claude_day_0", &payload);
        let loaded = cache.load("claude_day_0").unwrap();
        assert_eq!(loaded.total_cost, payload.total_cost);
        assert_eq!(loaded.total_tokens, payload.total_tokens);
    }

    #[test]
    fn load_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = PayloadDiskCache::new(tmp.path());
        assert!(cache.load("nonexistent").is_none());
    }

    #[test]
    fn key_sanitization() {
        let tmp = TempDir::new().unwrap();
        let cache = PayloadDiskCache::new(tmp.path());
        let payload = UsagePayload::default();
        cache.save("usage-view:claude:day:0", &payload);
        let loaded = cache.load("usage-view:claude:day:0");
        assert!(loaded.is_some());
    }
}
