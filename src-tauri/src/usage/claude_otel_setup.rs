//! Turn headless Claude Code capture on/off by adding or removing the OTel
//! env keys in Claude Code's `settings.json` (see `usage::claude_otel`).
//! Runs that pass `--setting-sources ""` ignore this file; their launcher has
//! to pass the same variables itself.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use super::claude_otel::OTEL_PORT;
use crate::statusline::claude_settings;

const TELEMETRY_KEY: &str = "CLAUDE_CODE_ENABLE_TELEMETRY";

fn wanted() -> [(&'static str, String); 4] {
    [
        (TELEMETRY_KEY, "1".to_string()),
        ("OTEL_LOGS_EXPORTER", "otlp".to_string()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json".to_string()),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            format!("http://127.0.0.1:{OTEL_PORT}"),
        ),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum OtelSetupState {
    On,
    Off,
    /// The user set this key to something else (e.g. their own collector).
    Conflict {
        key: String,
        value: String,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SetupRecord {
    /// Keys TokenMonitor added (absent before install).
    added: Vec<String>,
}

fn env_str<'a>(env: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    env?.get(key).and_then(Value::as_str)
}

fn conflict(env: Option<&Map<String, Value>>) -> Option<OtelSetupState> {
    wanted().into_iter().find_map(|(key, want)| {
        let have = env?.get(key)?;
        let same = have.as_str() == Some(want.as_str())
            || (key == TELEMETRY_KEY && matches!(have.as_str(), Some("true") | Some("TRUE")));
        (!same).then(|| OtelSetupState::Conflict {
            key: key.to_string(),
            value: have
                .as_str()
                .map_or_else(|| have.to_string(), str::to_string),
        })
    })
}

pub(crate) fn check_at(settings_path: &Path) -> OtelSetupState {
    let Ok(settings) = claude_settings::load(settings_path) else {
        return OtelSetupState::Off;
    };
    let env = settings.get("env").and_then(Value::as_object);
    if let Some(c) = conflict(env) {
        return c;
    }
    let all = wanted().iter().all(|(k, _)| env_str(env, k).is_some());
    if all {
        OtelSetupState::On
    } else {
        OtelSetupState::Off
    }
}

pub(crate) fn install_at(
    settings_path: &Path,
    record_path: &Path,
) -> Result<OtelSetupState, String> {
    let mut settings = claude_settings::load(settings_path)
        .map_err(|e| format!("read {}: {e}", settings_path.display()))?;
    if let Some(c) = conflict(settings.get("env").and_then(Value::as_object)) {
        return Ok(c);
    }
    let mut record = read_record(record_path);
    let env = settings
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("settings.json \"env\" is not an object")?;
    let mut added = Vec::new();
    for (key, want) in wanted() {
        if !env.contains_key(key) {
            env.insert(key.to_string(), Value::String(want));
            added.push(key.to_string());
        }
    }
    if added.is_empty() {
        return Ok(OtelSetupState::On);
    }
    for key in added {
        if !record.added.contains(&key) {
            record.added.push(key);
        }
    }
    // Record first: a failed settings write then leaves a harmless record,
    // never keys we cannot later attribute to ourselves.
    write_record(record_path, &record)?;
    claude_settings::backup_once(settings_path);
    claude_settings::save(settings_path, &settings)
        .map_err(|e| format!("write {}: {e}", settings_path.display()))?;
    Ok(OtelSetupState::On)
}

pub(crate) fn uninstall_at(
    settings_path: &Path,
    record_path: &Path,
) -> Result<OtelSetupState, String> {
    let parsed = fs::read_to_string(record_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<SetupRecord>(&raw).ok());
    let record_known = parsed.is_some();
    let record = parsed.unwrap_or_default();
    let mut settings = claude_settings::load(settings_path)
        .map_err(|e| format!("read {}: {e}", settings_path.display()))?;
    let mut changed = false;
    let mut stuck: Vec<&str> = Vec::new();
    if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
        for (key, want) in wanted() {
            let ours = record.added.iter().any(|k| k == key);
            let exact = env.get(key).and_then(Value::as_str) == Some(want.as_str());
            if ours && exact {
                env.remove(key);
                changed = true;
            } else if exact && !record_known {
                stuck.push(key);
            }
        }
        // Without a readable record we cannot tell our keys from the user's:
        // leave settings.json untouched and say what remains.
        if !stuck.is_empty() {
            return Err(format!(
                "Could not tell which settings.json entries TokenMonitor added (its setup record is missing). \
                 These keys are still set in {}: {}. Remove them by hand to turn capture off.",
                settings_path.display(),
                stuck.join(", ")
            ));
        }
        if env.is_empty() {
            settings.remove("env");
        }
    }
    if changed {
        claude_settings::save(settings_path, &settings)
            .map_err(|e| format!("write {}: {e}", settings_path.display()))?;
    }
    let _ = fs::remove_file(record_path);
    Ok(check_at(settings_path))
}

fn read_record(path: &Path) -> SetupRecord {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_record(path: &Path, record: &SetupRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string(record).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write {}: {e}", path.display()))
}

fn default_paths() -> Result<(PathBuf, PathBuf), String> {
    let settings =
        claude_settings::settings_path().ok_or("Could not resolve ~/.claude/settings.json")?;
    let record = crate::paths::claude_otel_setup_file().ok_or("no app data directory")?;
    Ok((settings, record))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn enable_claude_otel() -> Result<OtelSetupState, String> {
    blocking(|| {
        let (s, r) = default_paths()?;
        install_at(&s, &r)
    })
    .await
}

#[tauri::command]
pub async fn disable_claude_otel() -> Result<OtelSetupState, String> {
    blocking(|| {
        let (s, r) = default_paths()?;
        uninstall_at(&s, &r)
    })
    .await
}

#[tauri::command]
pub async fn check_claude_otel() -> Result<OtelSetupState, String> {
    blocking(|| Ok(check_at(&default_paths()?.0))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn files() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let record = dir.path().join("setup.json");
        (dir, settings, record)
    }

    fn env_of(settings: &Path) -> Value {
        let map = claude_settings::load(settings).unwrap();
        map.get("env").cloned().unwrap_or(Value::Null)
    }

    #[test]
    fn install_adds_all_keys_keeps_others_and_backs_up() {
        let (_d, s, r) = files();
        std::fs::write(&s, r#"{"model":"opus","env":{"FOO":"bar"}}"#).unwrap();
        assert_eq!(install_at(&s, &r).unwrap(), OtelSetupState::On);
        let env = env_of(&s);
        assert_eq!(env["FOO"], "bar");
        assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:53218");
        assert_eq!(env["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");
        assert_eq!(claude_settings::load(&s).unwrap()["model"], "opus");
        assert!(s.with_extension("json.tokenmonitor.bak").exists());
        assert_eq!(check_at(&s), OtelSetupState::On);
    }

    #[test]
    fn install_on_missing_file_creates_it_and_is_idempotent() {
        let (_d, s, r) = files();
        assert_eq!(install_at(&s, &r).unwrap(), OtelSetupState::On);
        assert_eq!(install_at(&s, &r).unwrap(), OtelSetupState::On);
        assert_eq!(env_of(&s).as_object().unwrap().len(), 4);
    }

    #[test]
    fn differing_exporter_value_is_a_conflict_and_nothing_is_written() {
        let (_d, s, r) = files();
        let before = r#"{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://collector:4318"}}"#;
        std::fs::write(&s, before).unwrap();
        let want = OtelSetupState::Conflict {
            key: "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
            value: "http://collector:4318".into(),
        };
        assert_eq!(install_at(&s, &r).unwrap(), want);
        assert_eq!(std::fs::read_to_string(&s).unwrap(), before);
        assert_eq!(check_at(&s), want);
    }

    #[test]
    fn telemetry_explicitly_off_is_a_conflict() {
        let (_d, s, r) = files();
        std::fs::write(&s, r#"{"env":{"CLAUDE_CODE_ENABLE_TELEMETRY":"0"}}"#).unwrap();
        assert!(matches!(
            install_at(&s, &r).unwrap(),
            OtelSetupState::Conflict { .. }
        ));
    }

    #[test]
    fn uninstall_keeps_keys_the_user_had_before() {
        let (_d, s, r) = files();
        std::fs::write(
            &s,
            r#"{"env":{"CLAUDE_CODE_ENABLE_TELEMETRY":"1","FOO":"bar"}}"#,
        )
        .unwrap();
        install_at(&s, &r).unwrap();
        assert_eq!(uninstall_at(&s, &r).unwrap(), OtelSetupState::Off);
        assert_eq!(
            env_of(&s),
            json!({"CLAUDE_CODE_ENABLE_TELEMETRY": "1", "FOO": "bar"})
        );
        assert!(!r.exists());
    }

    #[test]
    fn uninstall_drops_empty_env_and_leaves_changed_values() {
        let (_d, s, r) = files();
        install_at(&s, &r).unwrap();
        // The user repointed the endpoint afterwards: that key is theirs now.
        let mut map = claude_settings::load(&s).unwrap();
        map["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"] = json!("http://elsewhere");
        claude_settings::save(&s, &map).unwrap();
        uninstall_at(&s, &r).unwrap();
        assert_eq!(
            env_of(&s),
            json!({"OTEL_EXPORTER_OTLP_ENDPOINT": "http://elsewhere"})
        );

        let (_d2, s2, r2) = files();
        std::fs::write(&s2, r#"{"model":"opus"}"#).unwrap();
        install_at(&s2, &r2).unwrap();
        uninstall_at(&s2, &r2).unwrap();
        assert!(claude_settings::load(&s2).unwrap().get("env").is_none());
        assert_eq!(check_at(&s2), OtelSetupState::Off);
    }

    #[test]
    fn install_records_added_keys() {
        let (_d, s, r) = files();
        install_at(&s, &r).unwrap();
        let record = read_record(&r);
        assert_eq!(record.added.len(), 4);
        assert!(record.added.contains(&"OTEL_LOGS_EXPORTER".to_string()));
    }

    #[test]
    fn uninstall_without_record_errors_naming_keys_and_changes_nothing() {
        let (_d, s, r) = files();
        install_at(&s, &r).unwrap();
        std::fs::remove_file(&r).unwrap();
        let before = std::fs::read_to_string(&s).unwrap();
        let err = uninstall_at(&s, &r).unwrap_err();
        assert!(err.contains("OTEL_LOGS_EXPORTER"), "{err}");
        assert!(err.contains("by hand"), "{err}");
        assert_eq!(std::fs::read_to_string(&s).unwrap(), before);
    }

    #[test]
    fn invalid_settings_json_is_refused() {
        let (_d, s, r) = files();
        std::fs::write(&s, "{not json").unwrap();
        assert!(install_at(&s, &r).is_err());
        assert_eq!(std::fs::read_to_string(&s).unwrap(), "{not json");
    }
}
