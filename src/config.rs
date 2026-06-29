//! Config from the environment.

/// The agent hook command (`$MATOU_AGENT_CMD`), if set — used by the `a` (ask) panel.
pub fn agent_hook() -> Option<String> {
    std::env::var("MATOU_AGENT_CMD").ok().filter(|s| !s.is_empty())
}
