//! Tauri-free mirror of `src-tauri/src/usage/mod.rs`.
//!
//! Declares every usage submodule via #[path] to the real source, EXCEPT the
//! two that pull in Tauri (`cache_warmup` needs commands::AppState +
//! get_usage_data_inner; `device_aggregation` needs commands::AppState). Nothing
//! in the remaining files references those two, so excluding them compiles.

#[path = "../../src-tauri/src/usage/archive.rs"]
pub mod archive;
#[path = "../../src-tauri/src/usage/claude_parser.rs"]
pub mod claude_parser;
#[path = "../../src-tauri/src/usage/codex_parser.rs"]
pub mod codex_parser;
#[path = "../../src-tauri/src/usage/cursor_parser.rs"]
pub mod cursor_parser;
#[path = "../../src-tauri/src/usage/device_aggregation.rs"]
pub mod device_aggregation;
#[path = "../../src-tauri/src/usage/exchange_rates.rs"]
pub mod exchange_rates;
#[path = "../../src-tauri/src/usage/integrations.rs"]
pub mod integrations;
#[path = "../../src-tauri/src/usage/litellm.rs"]
pub mod litellm;
#[path = "../../src-tauri/src/usage/openrouter.rs"]
pub mod openrouter;
#[path = "../../src-tauri/src/usage/parser.rs"]
pub mod parser;
#[path = "../../src-tauri/src/usage/payload_disk_cache.rs"]
pub mod payload_disk_cache;
#[path = "../../src-tauri/src/usage/pricing.rs"]
pub mod pricing;
#[path = "../../src-tauri/src/usage/ssh_config.rs"]
pub mod ssh_config;
#[path = "../../src-tauri/src/usage/ssh_remote.rs"]
pub mod ssh_remote;
