//! On-disk persistence of agent replies + last instructions, at $XDG_CACHE_HOME/matou/agent.json.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Default, Serialize, Deserialize)]
pub struct AgentStore {
    /// key = `dir\0instr` → reply
    #[serde(default)]
    pub replies: HashMap<String, String>,
    /// dir → last instruction asked
    #[serde(default)]
    pub last_instr: HashMap<String, String>,
}

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .unwrap_or_else(|_| format!("{}/.cache", std::env::var("HOME").unwrap_or_default()));
    std::path::PathBuf::from(base).join("matou/agent.json")
}

pub fn load() -> AgentStore {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Atomic write (pid-tagged temp + rename).
pub fn save(store: &AgentStore) {
    let p = cache_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(store) {
        let tmp = p.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}
