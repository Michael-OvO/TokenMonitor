use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const UPSTREAM_REPO: &str = "Michael-OvO/TokenMonitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub id: String,
    pub label: String,
    pub owner: String,
    pub repo: String,
    pub has_releases: bool,
}

#[derive(Debug, Deserialize)]
struct GithubFork {
    full_name: String,
    owner: GithubOwner,
}

#[derive(Debug, Deserialize)]
struct GithubOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    #[allow(dead_code)]
    tag_name: String,
    #[allow(dead_code)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    #[allow(dead_code)]
    name: String,
}

pub async fn discover_channels() -> Vec<ChannelInfo> {
    let mut channels = vec![ChannelInfo {
        id: "main".into(),
        label: format!("{} (official)", UPSTREAM_REPO),
        owner: "Michael-OvO".into(),
        repo: "TokenMonitor".into(),
        has_releases: true,
    }];

    let forks = fetch_forks().await.unwrap_or_default();
    for fork in forks {
        let parts: Vec<&str> = fork.full_name.split('/').collect();
        if parts.len() != 2 {
            continue;
        }
        let has_releases = check_has_releases(&fork.full_name).await;
        if has_releases {
            channels.push(ChannelInfo {
                id: fork.full_name.clone(),
                label: fork.full_name.clone(),
                owner: fork.owner.login,
                repo: parts[1].to_string(),
                has_releases,
            });
        }
    }

    channels
}

async fn fetch_forks() -> Result<Vec<GithubFork>, String> {
    let url = format!(
        "https://api.github.com/repos/{}/forks?per_page=100&sort=pushed",
        UPSTREAM_REPO
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "TokenMonitor")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    resp.json::<Vec<GithubFork>>()
        .await
        .map_err(|e| e.to_string())
}

async fn check_has_releases(full_name: &str) -> bool {
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=1",
        full_name
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "TokenMonitor")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => r
            .json::<Vec<GithubRelease>>()
            .await
            .map(|releases| !releases.is_empty())
            .unwrap_or(false),
        _ => false,
    }
}

pub async fn fetch_fork_pubkey(channel: &str) -> Result<String, String> {
    let url = format!(
        "https://github.com/{}/releases/latest/download/updater-pubkey.txt",
        channel
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "TokenMonitor")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch pubkey: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "No updater-pubkey.txt found in {channel} releases (HTTP {})",
            resp.status()
        ));
    }

    let key = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read pubkey: {e}"))?
        .trim()
        .to_string();

    if key.is_empty() {
        return Err("Empty pubkey file".into());
    }

    Ok(key)
}

fn pubkey_cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("channel-pubkeys")
}

pub fn load_cached_pubkey(app_data_dir: &Path, channel: &str) -> Option<String> {
    let safe_name = channel.replace('/', "__");
    let path = pubkey_cache_dir(app_data_dir).join(format!("{safe_name}.pub"));
    std::fs::read_to_string(path).ok()
}

pub fn save_cached_pubkey(app_data_dir: &Path, channel: &str, pubkey: &str) -> Result<(), String> {
    let dir = pubkey_cache_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe_name = channel.replace('/', "__");
    let path = dir.join(format!("{safe_name}.pub"));
    std::fs::write(path, pubkey).map_err(|e| e.to_string())
}
