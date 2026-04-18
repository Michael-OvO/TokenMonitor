use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterState {
    pub available: Option<UpdateInfo>,
    pub last_check: Option<DateTime<Utc>>,
    pub last_check_error: Option<String>,
    pub skipped_versions: HashSet<String>,
    pub last_notified_version: Option<String>,
    pub auto_check_enabled: bool,
    pub progress: Option<DownloadProgress>,
    pub dismissed_for_session: bool,
}

#[allow(dead_code)]
impl UpdaterState {
    pub fn new() -> Self {
        Self {
            auto_check_enabled: true,
            ..Default::default()
        }
    }

    /// Whether the banner should be shown for the currently available update.
    pub fn should_show_banner(&self) -> bool {
        match &self.available {
            Some(info) => {
                !self.dismissed_for_session && !self.skipped_versions.contains(&info.version)
            }
            None => false,
        }
    }

    /// Whether an OS notification should fire for this version.
    pub fn should_notify(&self, version: &str) -> bool {
        self.auto_check_enabled
            && !self.skipped_versions.contains(version)
            && self.last_notified_version.as_deref() != Some(version)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_show_banner_hides_when_no_update() {
        let state = UpdaterState::new();
        assert!(!state.should_show_banner());
    }

    #[test]
    fn should_show_banner_hides_when_dismissed() {
        let mut state = UpdaterState::new();
        state.available = Some(UpdateInfo {
            version: "0.8.0".into(),
            current_version: "0.7.2".into(),
            notes: None,
            pub_date: None,
        });
        state.dismissed_for_session = true;
        assert!(!state.should_show_banner());
    }

    #[test]
    fn should_show_banner_hides_when_skipped() {
        let mut state = UpdaterState::new();
        state.available = Some(UpdateInfo {
            version: "0.8.0".into(),
            current_version: "0.7.2".into(),
            notes: None,
            pub_date: None,
        });
        state.skipped_versions.insert("0.8.0".into());
        assert!(!state.should_show_banner());
    }

    #[test]
    fn should_show_banner_true_when_available_and_not_suppressed() {
        let mut state = UpdaterState::new();
        state.available = Some(UpdateInfo {
            version: "0.8.0".into(),
            current_version: "0.7.2".into(),
            notes: None,
            pub_date: None,
        });
        assert!(state.should_show_banner());
    }

    #[test]
    fn should_notify_fires_on_new_version_only() {
        let mut state = UpdaterState::new();
        assert!(state.should_notify("0.8.0"));
        state.last_notified_version = Some("0.8.0".into());
        assert!(!state.should_notify("0.8.0"));
        assert!(state.should_notify("0.9.0"));
    }

    #[test]
    fn should_notify_suppressed_when_auto_check_off() {
        let mut state = UpdaterState::new();
        state.auto_check_enabled = false;
        assert!(!state.should_notify("0.8.0"));
    }

    #[test]
    fn should_notify_suppressed_for_skipped() {
        let mut state = UpdaterState::new();
        state.skipped_versions.insert("0.8.0".into());
        assert!(!state.should_notify("0.8.0"));
    }
}
