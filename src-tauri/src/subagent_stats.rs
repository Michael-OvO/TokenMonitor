// src-tauri/src/subagent_stats.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentScope {
    #[default]
    Main,
    Subagent,
}
