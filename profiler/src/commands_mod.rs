//! Tauri-free stub of `crate::commands` — just enough for device_aggregation.
//!
//! device_aggregation.rs references `crate::commands::AppState` (only the
//! parser / ssh_hosts / ssh_cache fields) and `crate::commands::period::*`
//! (pure date math). We provide both without the Tauri stack.

#[path = "../../src-tauri/src/commands/period.rs"]
pub mod period;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::usage::parser::UsageParser;
use crate::usage::ssh_remote::{SshCacheManager, SshHostConfig};

/// Minimal AppState carrying only the fields device_aggregation reads.
pub struct AppState {
    pub parser: Arc<UsageParser>,
    pub ssh_hosts: Arc<RwLock<Vec<SshHostConfig>>>,
    pub ssh_cache: Arc<RwLock<Option<SshCacheManager>>>,
}
