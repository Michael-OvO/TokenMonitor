use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single SSH host entry parsed from ~/.ssh/config.
#[derive(Debug, Clone)]
pub struct SshHostEntry {
    /// The alias used on the `Host` line (e.g., "myserver").
    pub alias: String,
    /// Resolved hostname (HostName directive, falls back to alias).
    pub hostname: String,
    /// Username (User directive).
    pub user: Option<String>,
    /// Port number (Port directive, default 22).
    pub port: u16,
    /// Path to the identity file (IdentityFile directive).
    #[allow(dead_code)]
    pub identity_file: Option<PathBuf>,
}

/// Serializable version sent to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SshHostInfo {
    pub alias: String,
    pub hostname: String,
    pub user: Option<String>,
    pub port: u16,
}

impl From<&SshHostEntry> for SshHostInfo {
    fn from(entry: &SshHostEntry) -> Self {
        Self {
            alias: entry.alias.clone(),
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port,
        }
    }
}

/// Discover all concrete SSH hosts from the user's ~/.ssh/config.
///
/// Returns hosts that have specific aliases (excludes wildcard patterns
/// like `*`, `*.example.com`, `!negated`).
pub fn discover_ssh_hosts() -> Vec<SshHostEntry> {
    let config_path = ssh_config_path();
    if !config_path.exists() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut hosts = Vec::new();
    parse_ssh_config_content(&content, &config_path, &mut hosts, 0);
    hosts
}

/// Returns the default SSH config path for the current platform.
fn ssh_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".ssh")
        .join("config")
}

/// Recursively parse SSH config content, handling Include directives.
///
/// `depth` limits recursion to prevent infinite Include loops.
fn parse_ssh_config_content(
    content: &str,
    config_path: &Path,
    hosts: &mut Vec<SshHostEntry>,
    depth: u8,
) {
    let home = dirs::home_dir().unwrap_or_default();
    parse_ssh_config_inner(content, config_path, &home, hosts, depth);
}

fn parse_ssh_config_inner(
    content: &str,
    config_path: &Path,
    home: &Path,
    hosts: &mut Vec<SshHostEntry>,
    depth: u8,
) {
    if depth > 5 {
        return; // Guard against Include loops
    }

    let config_dir = config_path.parent().unwrap_or(Path::new("."));

    // Accumulate directives for the current Host block.
    let mut current_aliases: Vec<String> = Vec::new();
    let mut directives: HashMap<String, String> = HashMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (keyword, value) = match split_directive(line) {
            Some(pair) => pair,
            None => continue,
        };

        let keyword_lower = keyword.to_ascii_lowercase();

        // Handle Include before anything else.
        if keyword_lower == "include" {
            // Flush any in-progress host block.
            flush_host_block(&current_aliases, &directives, home, hosts);
            current_aliases.clear();
            directives.clear();

            // Resolve Include path (expand ~ and globs).
            let patterns = resolve_include_path(value, config_dir, home);
            for pattern in patterns {
                if let Ok(entries) = glob_paths(&pattern) {
                    for path in entries {
                        if let Ok(inc_content) = std::fs::read_to_string(&path) {
                            parse_ssh_config_inner(&inc_content, &path, home, hosts, depth + 1);
                        }
                    }
                }
            }
            continue;
        }

        if keyword_lower == "host" {
            // Flush previous host block.
            flush_host_block(&current_aliases, &directives, home, hosts);
            directives.clear();

            // Parse all aliases on this Host line.
            current_aliases = value.split_whitespace().map(String::from).collect();
            continue;
        }

        if keyword_lower == "match" {
            // Flush previous host block; skip Match blocks entirely.
            flush_host_block(&current_aliases, &directives, home, hosts);
            current_aliases.clear();
            directives.clear();
            continue;
        }

        // Store directives for the current host block.
        // SSH config uses first-match semantics, so don't overwrite.
        directives
            .entry(keyword_lower)
            .or_insert_with(|| value.to_string());
    }

    // Flush the last host block.
    flush_host_block(&current_aliases, &directives, home, hosts);
}

/// Flush accumulated directives into concrete SshHostEntry items.
fn flush_host_block(
    aliases: &[String],
    directives: &HashMap<String, String>,
    home: &Path,
    hosts: &mut Vec<SshHostEntry>,
) {
    for alias in aliases {
        // Skip wildcard and negated patterns.
        if alias.contains('*') || alias.contains('?') || alias.starts_with('!') {
            continue;
        }

        let hostname = directives
            .get("hostname")
            .cloned()
            .unwrap_or_else(|| alias.clone());

        let user = directives.get("user").cloned();

        let port = directives
            .get("port")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(22);

        let identity_file = directives
            .get("identityfile")
            .map(|p| expand_tilde(p, home));

        // Skip if this alias was already discovered (first match wins).
        if hosts.iter().any(|h| h.alias == *alias) {
            continue;
        }

        hosts.push(SshHostEntry {
            alias: alias.clone(),
            hostname,
            user,
            port,
            identity_file,
        });
    }
}

/// Split a line into (keyword, value), handling both `Key Value` and `Key=Value`.
fn split_directive(line: &str) -> Option<(&str, &str)> {
    // Try `Key=Value` first.
    if let Some(eq_pos) = line.find('=') {
        let keyword = line[..eq_pos].trim();
        let value = line[eq_pos + 1..].trim();
        if !keyword.is_empty() && !value.is_empty() {
            return Some((keyword, value));
        }
    }

    // Fall back to whitespace split.
    let mut parts = line.splitn(2, char::is_whitespace);
    let keyword = parts.next()?.trim();
    let value = parts.next()?.trim();
    if keyword.is_empty() || value.is_empty() {
        return None;
    }
    Some((keyword, value))
}

/// Expand `~` in a path to the home directory.
fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        home.join(&path[2..])
    } else {
        PathBuf::from(path)
    }
}

/// Resolve an Include path, expanding ~ and returning glob patterns.
fn resolve_include_path(value: &str, config_dir: &Path, home: &Path) -> Vec<String> {
    value
        .split_whitespace()
        .map(|token| {
            let expanded = if token.starts_with("~/") || token == "~" {
                home.join(&token[2..]).to_string_lossy().to_string()
            } else if !Path::new(token).is_absolute() {
                config_dir.join(token).to_string_lossy().to_string()
            } else {
                token.to_string()
            };
            // Normalize path separators for cross-platform glob.
            expanded.replace('\\', "/")
        })
        .collect()
}

/// Simple glob expansion for Include directives.
/// Supports `*` wildcard in the last path segment only (e.g., `~/.ssh/config.d/*`).
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, std::io::Error> {
    let path = Path::new(pattern);

    // If no wildcard, return the path directly if it exists.
    if !pattern.contains('*') && !pattern.contains('?') {
        if path.exists() {
            return Ok(vec![path.to_path_buf()]);
        }
        return Ok(Vec::new());
    }

    // Simple glob: split at the last wildcard segment.
    let parent = match path.parent() {
        Some(p) if p.exists() => p,
        _ => return Ok(Vec::new()),
    };

    let file_pattern = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches_simple_glob(&name, &file_pattern) {
                let path = entry.path();
                if path.is_file() {
                    results.push(path);
                }
            }
        }
    }
    results.sort();
    Ok(results)
}

/// Match a filename against a simple glob pattern (only `*` and `?`).
fn matches_simple_glob(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return !name.starts_with('.');
    }

    let name_bytes = name.as_bytes();
    let pattern_bytes = pattern.as_bytes();
    let (n, m) = (name_bytes.len(), pattern_bytes.len());

    // DP-free approach: single-star match.
    let mut ni = 0;
    let mut pi = 0;
    let mut star_pi = usize::MAX;
    let mut star_ni = 0;

    while ni < n {
        if pi < m && (pattern_bytes[pi] == b'?' || pattern_bytes[pi] == name_bytes[ni]) {
            ni += 1;
            pi += 1;
        } else if pi < m && pattern_bytes[pi] == b'*' {
            star_pi = pi;
            star_ni = ni;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ni += 1;
            ni = star_ni;
        } else {
            return false;
        }
    }

    while pi < m && pattern_bytes[pi] == b'*' {
        pi += 1;
    }

    pi == m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_directive_whitespace() {
        assert_eq!(split_directive("Host myserver"), Some(("Host", "myserver")));
    }

    #[test]
    fn split_directive_equals() {
        assert_eq!(
            split_directive("IdentityFile=~/.ssh/id_rsa"),
            Some(("IdentityFile", "~/.ssh/id_rsa"))
        );
    }

    #[test]
    fn split_directive_whitespace_with_value_spaces() {
        assert_eq!(
            split_directive("Host foo bar baz"),
            Some(("Host", "foo bar baz"))
        );
    }

    #[test]
    fn expand_tilde_home() {
        let home = PathBuf::from("/home/user");
        assert_eq!(
            expand_tilde("~/.ssh/id_rsa", &home),
            PathBuf::from("/home/user/.ssh/id_rsa")
        );
    }

    #[test]
    fn expand_tilde_absolute() {
        let home = PathBuf::from("/home/user");
        assert_eq!(
            expand_tilde("/etc/ssh/key", &home),
            PathBuf::from("/etc/ssh/key")
        );
    }

    #[test]
    fn matches_simple_glob_star() {
        assert!(matches_simple_glob("config", "*"));
        assert!(!matches_simple_glob(".hidden", "*"));
    }

    #[test]
    fn matches_simple_glob_pattern() {
        assert!(matches_simple_glob("config.d", "config.*"));
        assert!(!matches_simple_glob("other.d", "config.*"));
    }

    #[test]
    fn parse_basic_config() {
        let content = "\
Host myserver
    HostName 192.168.1.100
    User john
    Port 2222
    IdentityFile ~/.ssh/id_ed25519

Host devbox
    HostName dev.example.com

Host *
    ServerAliveInterval 60
";
        let home = PathBuf::from("/home/test");
        let config_path = home.join(".ssh/config");
        let mut hosts = Vec::new();
        parse_ssh_config_inner(content, &config_path, &home, &mut hosts, 0);

        assert_eq!(hosts.len(), 2);

        assert_eq!(hosts[0].alias, "myserver");
        assert_eq!(hosts[0].hostname, "192.168.1.100");
        assert_eq!(hosts[0].user.as_deref(), Some("john"));
        assert_eq!(hosts[0].port, 2222);
        assert_eq!(
            hosts[0].identity_file,
            Some(PathBuf::from("/home/test/.ssh/id_ed25519"))
        );

        assert_eq!(hosts[1].alias, "devbox");
        assert_eq!(hosts[1].hostname, "dev.example.com");
        assert_eq!(hosts[1].user, None);
        assert_eq!(hosts[1].port, 22);
    }

    #[test]
    fn parse_multi_alias_host_line() {
        let content = "Host foo bar baz\n    HostName shared.example.com\n";
        let home = PathBuf::from("/tmp");
        let config_path = PathBuf::from("/tmp/.ssh/config");
        let mut hosts = Vec::new();
        parse_ssh_config_inner(content, &config_path, &home, &mut hosts, 0);

        assert_eq!(hosts.len(), 3);
        assert_eq!(hosts[0].alias, "foo");
        assert_eq!(hosts[1].alias, "bar");
        assert_eq!(hosts[2].alias, "baz");
        for h in &hosts {
            assert_eq!(h.hostname, "shared.example.com");
        }
    }

    #[test]
    fn skips_wildcard_and_negated_hosts() {
        let content = "\
Host *.example.com
    User wildcard

Host !excluded
    User negated

Host real-host
    HostName 10.0.0.1
";
        let home = PathBuf::from("/tmp");
        let config_path = PathBuf::from("/tmp/.ssh/config");
        let mut hosts = Vec::new();
        parse_ssh_config_inner(content, &config_path, &home, &mut hosts, 0);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "real-host");
    }

    #[test]
    fn first_match_wins_for_duplicate_aliases() {
        let content = "\
Host myhost
    HostName first.example.com
    Port 2222

Host myhost
    HostName second.example.com
    Port 3333
";
        let home = PathBuf::from("/tmp");
        let config_path = PathBuf::from("/tmp/.ssh/config");
        let mut hosts = Vec::new();
        parse_ssh_config_inner(content, &config_path, &home, &mut hosts, 0);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "first.example.com");
        assert_eq!(hosts[0].port, 2222);
    }
}
