use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

pub const ALL_USAGE_INTEGRATIONS_ID: &str = "all";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsageIntegrationId {
    Claude,
    Codex,
    Cursor,
    Kimi,
}

const ALL_USAGE_INTEGRATIONS: [UsageIntegrationId; 4] = [
    UsageIntegrationId::Claude,
    UsageIntegrationId::Codex,
    UsageIntegrationId::Cursor,
    UsageIntegrationId::Kimi,
];

impl UsageIntegrationId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Kimi => "kimi",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Cursor => "Cursor IDE",
            Self::Kimi => "Kimi Code",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            "kimi" => Some(Self::Kimi),
            _ => None,
        }
    }

    pub fn detect_roots(self) -> Vec<PathBuf> {
        match self {
            Self::Claude => detect_claude_project_dirs(),
            Self::Codex => vec![detect_codex_sessions_dir()],
            Self::Cursor => detect_cursor_workspace_storage_dirs(),
            Self::Kimi => detect_kimi_sessions_dirs(),
        }
    }
}

/// Separator between integration ids in a multi-integration scope string,
/// e.g. `claude+kimi`. Mirrored by `USAGE_SCOPE_SEPARATOR` in
/// `src/lib/providerMetadata.ts`.
pub const USAGE_SELECTION_SEPARATOR: char = '+';

/// Which integrations a usage request covers. Parsed from the `provider`
/// string the frontend sends: `all`, one id, or ids joined by `+`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageIntegrationSelection {
    Single(UsageIntegrationId),
    /// Two or more, but not all, integrations in canonical order with no
    /// duplicates. Built only through `parse`, which normalises the input.
    Subset(Vec<UsageIntegrationId>),
    All,
}

impl UsageIntegrationSelection {
    pub fn parse(value: &str) -> Option<Self> {
        if value == ALL_USAGE_INTEGRATIONS_ID {
            return Some(Self::All);
        }
        let mut ids: Vec<UsageIntegrationId> = Vec::new();
        for part in value.split(USAGE_SELECTION_SEPARATOR) {
            let id = UsageIntegrationId::parse(part)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            None
        } else {
            Some(Self::from_ids(&ids))
        }
    }

    /// Canonical selection for a set of ids: sorted into integration order,
    /// de-duplicated, and collapsed to `Single`/`All` exactly like `parse`.
    /// An empty set yields `All`, mirroring the frontend's fallback when no
    /// integration tab is enabled (the All view never shows nothing).
    pub fn from_ids(ids: &[UsageIntegrationId]) -> Self {
        let sorted: Vec<UsageIntegrationId> = ALL_USAGE_INTEGRATIONS
            .into_iter()
            .filter(|id| ids.contains(id))
            .collect();
        match sorted.len() {
            0 => Self::All,
            1 => Self::Single(sorted[0]),
            n if n == ALL_USAGE_INTEGRATIONS.len() => Self::All,
            _ => Self::Subset(sorted),
        }
    }

    pub fn integration_ids(&self) -> Vec<UsageIntegrationId> {
        match self {
            Self::Single(id) => vec![*id],
            Self::Subset(ids) => ids.clone(),
            Self::All => ALL_USAGE_INTEGRATIONS.to_vec(),
        }
    }

    pub fn contains(&self, id: UsageIntegrationId) -> bool {
        match self {
            Self::Single(single) => *single == id,
            Self::Subset(ids) => ids.contains(&id),
            Self::All => true,
        }
    }

    pub fn includes_cursor(&self) -> bool {
        self.contains(UsageIntegrationId::Cursor)
    }
}

impl std::fmt::Display for UsageIntegrationSelection {
    /// The canonical scope string: `all`, a single id, or sorted ids joined
    /// by `+`. Every cache key and debug report uses this spelling.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str(ALL_USAGE_INTEGRATIONS_ID),
            Self::Single(id) => f.write_str(id.as_str()),
            Self::Subset(ids) => {
                let parts: Vec<&str> = ids.iter().map(|id| id.as_str()).collect();
                f.write_str(&parts.join(&USAGE_SELECTION_SEPARATOR.to_string()))
            }
        }
    }
}

pub fn all_usage_integrations() -> &'static [UsageIntegrationId] {
    &ALL_USAGE_INTEGRATIONS
}

/// Is this row's model relevant to the selected provider tab?
///
/// The "Claude / Codex / Cursor" tabs are ultimately per-model-vendor filters,
/// not per-CLI filters — a GLM-5 row logged through Claude Code CLI does not
/// belong in the Claude tab, it's a third-party model. This keeps the main
/// dashboard total and the Per-Device breakdown consistent regardless of
/// which integration directory produced the row.
///
/// `cursor` passes through (no model-family filter) because Cursor IDE
/// multiplexes composer/gpt/claude/etc. — all rows from the Cursor
/// integration conceptually belong to the Cursor tab.
pub fn provider_matches_model(provider: &str, model: &str) -> bool {
    if provider == ALL_USAGE_INTEGRATIONS_ID {
        return true;
    }
    // Fast path for the single-id strings the per-record device filters use.
    if let Some(id) = UsageIntegrationId::parse(provider) {
        return integration_matches_model(id, model);
    }
    // Subset (`claude+kimi`): walk the parts without allocating — this runs
    // once per entry in the parser's retain and the device filters.
    let mut saw_known = false;
    for part in provider.split(USAGE_SELECTION_SEPARATOR) {
        if let Some(id) = UsageIntegrationId::parse(part) {
            saw_known = true;
            if integration_matches_model(id, model) {
                return true;
            }
        }
    }
    // Unknown provider strings pass through, as before.
    !saw_known
}

/// Match a remote SSH or archived peer-device row against a scope.
///
/// Those rows only ever carry Claude and Codex usage and, unlike rows read
/// from the local Cursor integration, have no "belongs to the Cursor tab"
/// meaning. So Cursor's match-everything rule from `provider_matches_model`
/// must not apply here: with only the Codex tab disabled the scope is
/// `claude+cursor+kimi`, and a Codex row has to be excluded rather than
/// admitted through the Cursor wildcard. Allocation-free like its sibling.
pub fn remote_record_matches_provider(provider: &str, model: &str) -> bool {
    use crate::models::{detect_model_family, ModelFamily};
    if provider == ALL_USAGE_INTEGRATIONS_ID {
        return true;
    }
    let family = detect_model_family(model);
    let mut saw_known = false;
    for part in provider.split(USAGE_SELECTION_SEPARATOR) {
        let Some(id) = UsageIntegrationId::parse(part) else {
            continue;
        };
        saw_known = true;
        let matches = match id {
            UsageIntegrationId::Claude => family == ModelFamily::Anthropic,
            UsageIntegrationId::Codex => family == ModelFamily::OpenAI,
            UsageIntegrationId::Cursor | UsageIntegrationId::Kimi => false,
        };
        if matches {
            return true;
        }
    }
    // Unknown provider strings pass through, as `provider_matches_model` does.
    !saw_known
}

fn integration_matches_model(id: UsageIntegrationId, model: &str) -> bool {
    use crate::models::{detect_model_family, ModelFamily};
    match id {
        UsageIntegrationId::Claude => detect_model_family(model) == ModelFamily::Anthropic,
        UsageIntegrationId::Codex => detect_model_family(model) == ModelFamily::OpenAI,
        UsageIntegrationId::Kimi => detect_model_family(model) == ModelFamily::Moonshot,
        UsageIntegrationId::Cursor => true,
    }
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

/// Normalize a Kimi data-home path to its `sessions` subdirectory. Accepts a
/// data home (`~/.kimi`, `~/.kimi-code`) and appends `sessions`, or passes a
/// path already ending in `sessions` through unchanged — mirroring Codex.
fn normalize_kimi_sessions_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|name| name == "sessions") {
        path
    } else {
        path.join("sessions")
    }
}

fn normalize_cursor_workspace_storage_dir(path: PathBuf) -> PathBuf {
    if path
        .file_name()
        .is_some_and(|name| name == "workspaceStorage")
    {
        return path;
    }
    if path.file_name().is_some_and(|name| name == "User") {
        return path.join("workspaceStorage");
    }
    if path.file_name().is_some_and(|name| name == "Cursor") {
        return path.join("User").join("workspaceStorage");
    }
    path.join("workspaceStorage")
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
            for p in &explicit {
                tracing::debug!(path = %p.display(), "Claude root (from CLAUDE_CONFIG_DIR)");
            }
            return dedupe_paths(explicit);
        }
    }

    let roots = crate::paths::claude_project_roots_default();
    if roots.is_empty() {
        tracing::warn!("Could not determine home directory for Claude projects");
    }
    for p in &roots {
        tracing::debug!(path = %p.display(), "Claude root (default)");
    }
    dedupe_paths(roots)
}

fn detect_cursor_workspace_storage_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("CURSOR_USER_DIR") {
        let explicit = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(normalize_cursor_workspace_storage_dir)
            .collect::<Vec<_>>();

        if !explicit.is_empty() {
            for p in &explicit {
                tracing::debug!(path = %p.display(), "Cursor root (from CURSOR_USER_DIR)");
            }
            return dedupe_paths(explicit);
        }
    }

    let mut roots = Vec::new();
    if let Some(default_root) = crate::paths::cursor_workspace_storage_default() {
        roots.push(default_root);
    }
    if roots.is_empty() {
        tracing::warn!("Could not determine Cursor workspace storage directory");
    }
    for p in &roots {
        tracing::debug!(path = %p.display(), "Cursor root (default)");
    }
    dedupe_paths(roots)
}

fn detect_codex_sessions_dir() -> PathBuf {
    if let Ok(raw) = env::var("CODEX_HOME") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let p = normalize_codex_sessions_dir(PathBuf::from(trimmed));
            tracing::debug!(path = %p.display(), "Codex root (from CODEX_HOME)");
            return p;
        }
    }

    let p = crate::paths::codex_sessions_default().unwrap_or_else(|| {
        tracing::warn!("Could not determine home directory for Codex sessions");
        PathBuf::new()
    });
    tracing::debug!(path = %p.display(), "Codex root (default)");
    p
}

/// Kimi Code CLI session-log roots. The Kimi CLI writes `wire.jsonl` files under
/// `<data-home>/sessions/…`; the current CLI defaults to `~/.kimi-code` and the
/// legacy `kimi-cli` to `~/.kimi`, so both are scanned. `KIMI_DATA_DIR`
/// (comma-separated, matching ccusage) overrides the defaults.
fn detect_kimi_sessions_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("KIMI_DATA_DIR") {
        let explicit = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(normalize_kimi_sessions_dir)
            .collect::<Vec<_>>();

        if !explicit.is_empty() {
            for p in &explicit {
                tracing::debug!(path = %p.display(), "Kimi root (from KIMI_DATA_DIR)");
            }
            return dedupe_paths(explicit);
        }
    }

    let roots = crate::paths::kimi_sessions_defaults();
    if roots.is_empty() {
        tracing::warn!("Could not determine home directory for Kimi sessions");
    }
    for p in &roots {
        tracing::debug!(path = %p.display(), "Kimi root (default)");
    }
    dedupe_paths(roots)
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
        assert_eq!(
            UsageIntegrationSelection::parse("cursor"),
            Some(UsageIntegrationSelection::Single(
                UsageIntegrationId::Cursor
            ))
        );
        assert_eq!(
            UsageIntegrationSelection::parse("kimi"),
            Some(UsageIntegrationSelection::Single(UsageIntegrationId::Kimi))
        );
    }

    #[test]
    fn kimi_tab_matches_only_moonshot_models() {
        assert!(provider_matches_model("kimi", "kimi-for-coding"));
        assert!(provider_matches_model("kimi", "kimi-k2.5"));
        assert!(!provider_matches_model("kimi", "claude-sonnet-4-5"));
        assert!(!provider_matches_model("kimi", "gpt-5-codex"));
        // Kimi rows must not leak into the Claude/Codex tabs.
        assert!(!provider_matches_model("claude", "kimi-for-coding"));
        assert!(!provider_matches_model("codex", "kimi-for-coding"));
    }

    #[test]
    fn usage_integration_selection_rejects_unknown_values() {
        assert_eq!(UsageIntegrationSelection::parse("gemini"), None);
    }

    #[test]
    fn usage_integration_selection_parses_subsets_in_canonical_order() {
        assert_eq!(
            UsageIntegrationSelection::parse("kimi+claude"),
            Some(UsageIntegrationSelection::Subset(vec![
                UsageIntegrationId::Claude,
                UsageIntegrationId::Kimi,
            ]))
        );
        // Duplicates collapse; a single survivor is a Single, not a Subset.
        assert_eq!(
            UsageIntegrationSelection::parse("codex+codex"),
            Some(UsageIntegrationSelection::Single(UsageIntegrationId::Codex))
        );
        // Every integration spelled out is just `all`.
        assert_eq!(
            UsageIntegrationSelection::parse("kimi+cursor+codex+claude"),
            Some(UsageIntegrationSelection::All)
        );
    }

    #[test]
    fn usage_integration_selection_rejects_malformed_subsets() {
        assert_eq!(UsageIntegrationSelection::parse(""), None);
        assert_eq!(UsageIntegrationSelection::parse("claude+"), None);
        assert_eq!(UsageIntegrationSelection::parse("claude+gemini"), None);
        assert_eq!(UsageIntegrationSelection::parse("claude+all"), None);
    }

    #[test]
    fn usage_integration_selection_round_trips_through_display() {
        for spelling in ["all", "claude", "kimi+claude", "cursor+codex+kimi"] {
            let parsed = UsageIntegrationSelection::parse(spelling).unwrap();
            let canonical = parsed.to_string();
            assert_eq!(UsageIntegrationSelection::parse(&canonical), Some(parsed));
        }
        assert_eq!(
            UsageIntegrationSelection::parse("kimi+claude")
                .unwrap()
                .to_string(),
            "claude+kimi"
        );
        assert_eq!(
            UsageIntegrationSelection::parse("all").unwrap().to_string(),
            "all"
        );
    }

    #[test]
    fn usage_integration_selection_membership() {
        let subset = UsageIntegrationSelection::parse("claude+kimi").unwrap();
        assert_eq!(
            subset.integration_ids(),
            vec![UsageIntegrationId::Claude, UsageIntegrationId::Kimi]
        );
        assert!(subset.contains(UsageIntegrationId::Kimi));
        assert!(!subset.contains(UsageIntegrationId::Codex));
        assert!(!subset.includes_cursor());
        assert!(UsageIntegrationSelection::parse("cursor+kimi")
            .unwrap()
            .includes_cursor());
        assert!(UsageIntegrationSelection::All.includes_cursor());
        assert_eq!(
            UsageIntegrationSelection::All.integration_ids(),
            all_usage_integrations().to_vec()
        );
    }

    #[test]
    fn subset_matches_models_of_any_member() {
        assert!(provider_matches_model("claude+kimi", "kimi-for-coding"));
        assert!(provider_matches_model("claude+kimi", "claude-sonnet-4-5"));
        assert!(!provider_matches_model("claude+kimi", "gpt-5-codex"));
        // Cursor passes every model through, as it does for the single tab.
        assert!(provider_matches_model("cursor+kimi", "gpt-5-codex"));
        // Existing single-tab and all-tab answers are unchanged.
        assert!(provider_matches_model("all", "gpt-5-codex"));
        assert!(!provider_matches_model("claude", "gpt-5-codex"));
    }

    #[test]
    fn from_ids_canonicalises_and_collapses() {
        use UsageIntegrationId::*;
        assert_eq!(
            UsageIntegrationSelection::from_ids(&[Kimi, Claude, Kimi]),
            UsageIntegrationSelection::Subset(vec![Claude, Kimi])
        );
        assert_eq!(
            UsageIntegrationSelection::from_ids(&[Codex]),
            UsageIntegrationSelection::Single(Codex)
        );
        assert_eq!(
            UsageIntegrationSelection::from_ids(&[Kimi, Cursor, Codex, Claude]),
            UsageIntegrationSelection::All
        );
        assert_eq!(
            UsageIntegrationSelection::from_ids(&[]),
            UsageIntegrationSelection::All
        );
        assert_eq!(
            UsageIntegrationSelection::from_ids(&[Kimi, Claude]).to_string(),
            "claude+kimi"
        );
    }

    #[test]
    fn remote_rows_ignore_the_cursor_wildcard() {
        // Only the Codex tab disabled: the scope still contains Cursor, but a
        // remote Codex row must stay out.
        assert!(!remote_record_matches_provider(
            "claude+cursor+kimi",
            "gpt-5-codex"
        ));
        assert!(remote_record_matches_provider(
            "claude+cursor+kimi",
            "claude-sonnet-4-5"
        ));
        assert!(remote_record_matches_provider("codex+kimi", "gpt-5-codex"));
        assert!(!remote_record_matches_provider(
            "cursor+kimi",
            "gpt-5-codex"
        ));
        assert!(!remote_record_matches_provider(
            "cursor+kimi",
            "claude-sonnet-4-5"
        ));
        // Single ids and `all` behave as the tabs always did for remote rows.
        assert!(remote_record_matches_provider("all", "gpt-5-codex"));
        assert!(remote_record_matches_provider(
            "claude",
            "claude-sonnet-4-5"
        ));
        assert!(!remote_record_matches_provider("claude", "gpt-5-codex"));
        assert!(remote_record_matches_provider("codex", "gpt-5-codex"));
        // Unknown scopes pass through like provider_matches_model.
        assert!(remote_record_matches_provider("gemini", "gpt-5-codex"));
        // Local rows keep Cursor's pass-through (unchanged behaviour).
        assert!(provider_matches_model("claude+cursor+kimi", "gpt-5-codex"));
    }
}
