use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

pub const ALL_USAGE_INTEGRATIONS_ID: &str = "all";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsageIntegrationId {
    Claude,
    Codex,
}

const ALL_USAGE_INTEGRATIONS: [UsageIntegrationId; 2] =
    [UsageIntegrationId::Claude, UsageIntegrationId::Codex];

impl UsageIntegrationId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex CLI",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn detect_roots(self) -> Vec<PathBuf> {
        match self {
            Self::Claude => detect_claude_project_dirs(),
            Self::Codex => vec![detect_codex_sessions_dir()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageIntegrationSelection {
    Single(UsageIntegrationId),
    All,
}

impl UsageIntegrationSelection {
    pub fn parse(value: &str) -> Option<Self> {
        if value == ALL_USAGE_INTEGRATIONS_ID {
            Some(Self::All)
        } else {
            UsageIntegrationId::parse(value).map(Self::Single)
        }
    }

    pub fn integration_ids(self) -> &'static [UsageIntegrationId] {
        match self {
            Self::Single(id) => match id {
                UsageIntegrationId::Claude => &ALL_USAGE_INTEGRATIONS[..1],
                UsageIntegrationId::Codex => &ALL_USAGE_INTEGRATIONS[1..],
            },
            Self::All => &ALL_USAGE_INTEGRATIONS,
        }
    }
}

pub fn all_usage_integrations() -> &'static [UsageIntegrationId] {
    &ALL_USAGE_INTEGRATIONS
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for path in paths {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            out.push(path);
        }
    }

    out
}

fn normalize_claude_projects_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "projects") {
        path
    } else {
        path.join("projects")
    }
}

fn normalize_codex_sessions_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "sessions") {
        path
    } else {
        path.join("sessions")
    }
}

fn detect_claude_project_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("CLAUDE_CONFIG_DIR") {
        let explicit = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(normalize_claude_projects_dir)
            .collect::<Vec<_>>();

        if !explicit.is_empty() {
            return dedupe_paths(explicit);
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!("Could not determine home directory for Claude projects");
        PathBuf::new()
    });
    let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));

    dedupe_paths(vec![
        config_dir.join("claude").join("projects"),
        home.join(".claude").join("projects"),
    ])
}

fn detect_codex_sessions_dir() -> PathBuf {
    if let Ok(raw) = env::var("CODEX_HOME") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return normalize_codex_sessions_dir(PathBuf::from(trimmed));
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!("Could not determine home directory for Codex sessions");
        PathBuf::new()
    });
    home.join(".codex").join("sessions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_integration_selection_parses_all() {
        assert_eq!(
            UsageIntegrationSelection::parse("all"),
            Some(UsageIntegrationSelection::All)
        );
    }

    #[test]
    fn usage_integration_selection_parses_single_integration() {
        assert_eq!(
            UsageIntegrationSelection::parse("claude"),
            Some(UsageIntegrationSelection::Single(
                UsageIntegrationId::Claude
            ))
        );
        assert_eq!(
            UsageIntegrationSelection::parse("codex"),
            Some(UsageIntegrationSelection::Single(UsageIntegrationId::Codex))
        );
    }

    #[test]
    fn usage_integration_selection_rejects_unknown_values() {
        assert_eq!(UsageIntegrationSelection::parse("gemini"), None);
    }
}
