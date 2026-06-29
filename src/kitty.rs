//! kitty remote-control client — the `kitty @ ls` window tree + the commands matou drives it with.
#![allow(dead_code)] // the deserialized structs mirror kitty's JSON; not every field is read

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;

#[derive(Deserialize, Default)]
pub struct OsWindow {
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

#[derive(Deserialize, Default)]
pub struct Tab {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub windows: Vec<Win>,
}

#[derive(Deserialize, Default)]
pub struct Win {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pid: i64,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub cmdline: Vec<String>,
    #[serde(default)]
    pub foreground_processes: Vec<Proc>,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub at_prompt: bool,
    #[serde(default)]
    pub last_cmd_exit_status: i64,
    #[serde(default)]
    pub last_focused_at: f64,
    #[serde(default)]
    pub user_vars: HashMap<String, String>,
}

#[derive(Deserialize, Default)]
pub struct Proc {
    #[serde(default)]
    pub cmdline: Vec<String>,
}

/// `kitty @ ls` → the OS-window tree. Errors if remote control is unavailable.
pub fn kitty_ls() -> anyhow::Result<Vec<OsWindow>> {
    let out = Command::new("kitty").args(["@", "ls"]).output()?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn run(args: &[&str]) {
    let _ = Command::new("kitty").arg("@").args(args).status();
}

/// `kitty @ <cmd>` capturing stdout (trimmed) — for `launch`, which prints the new window id.
pub fn capture(args: &[&str]) -> String {
    Command::new("kitty")
        .arg("@")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn focus_window(id: i64) {
    run(&["focus-window", "--match", &format!("id:{id}")]);
}
pub fn focus_tab(id: i64) {
    run(&["focus-tab", "--match", &format!("id:{id}")]);
}
pub fn close_tab(id: i64) {
    run(&["close-tab", "--match", &format!("id:{id}")]);
}
pub fn close_window(id: i64) {
    run(&["close-window", "--match", &format!("id:{id}")]);
}
pub fn set_tab_title(id: i64, title: &str) {
    run(&["set-tab-title", "--match", &format!("id:{id}"), title]);
}
pub fn move_to_tab(src_win: i64, dest_tab: i64) {
    run(&["detach-window", "--match", &format!("id:{src_win}"), "--target-tab", &format!("id:{dest_tab}")]);
    focus_tab(dest_tab);
}

/// Our own window id (set by kitty on launch), used to exclude ourselves.
pub fn self_window_id() -> i64 {
    std::env::var("KITTY_WINDOW_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// The id of another matou overlay (`user_vars.matou == "1"`) in the focused OS window's active
/// tab, excluding self — drives the press-again-to-dismiss singleton toggle.
pub fn find_other_matou(tree: &[OsWindow], self_id: i64) -> Option<i64> {
    for ow in tree.iter().filter(|w| w.is_focused) {
        for tab in ow.tabs.iter().filter(|t| t.is_active) {
            for w in &tab.windows {
                if w.id != self_id && w.user_vars.get("matou").map(String::as_str) == Some("1") {
                    return Some(w.id);
                }
            }
        }
    }
    None
}

/// One open project tab (its active window) — a jump target.
pub struct OpenTab {
    pub win_id: i64,
    pub tab_id: i64,
    pub cwd: String,
    pub title: String,
    pub status: String, // focused | running | idle | failed
    pub focused_at: f64,
    pub proc: String,
    pub branch: String,
    pub changes: usize,
}

fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

/// Foreground command basename in a window (the shell when idle).
fn proc_name(w: &Win) -> String {
    if let Some(p) = w.foreground_processes.last() {
        if let Some(c) = p.cmdline.first() {
            return basename(c);
        }
    }
    w.cmdline.first().map(|c| basename(c)).unwrap_or_default()
}

/// Flatten `kitty @ ls` to one jump target per tab (the active window), skipping matou's own
/// window; also return the set of already-open cwds (to dedup the project list). Recent-first.
pub fn open_tabs() -> anyhow::Result<(Vec<OpenTab>, HashSet<String>)> {
    let tree = kitty_ls()?;
    let self_id = self_window_id();
    let mut tabs: Vec<OpenTab> = Vec::new();
    let mut cwds: HashSet<String> = HashSet::new();
    for ow in &tree {
        for t in &ow.tabs {
            let wins: Vec<&Win> = t.windows.iter().filter(|w| w.id != self_id).collect();
            if wins.is_empty() {
                continue;
            }
            let mut a: &Win = wins[0];
            for &w in &wins {
                if w.is_active || w.is_focused {
                    a = w;
                    break;
                }
            }
            let status = if a.is_focused && t.is_active && ow.is_focused {
                "focused"
            } else if a.last_cmd_exit_status != 0 {
                "failed"
            } else if !a.at_prompt {
                "running"
            } else {
                "idle"
            };
            let (branch, changes, _) = crate::git::git_status(&a.cwd);
            tabs.push(OpenTab {
                win_id: a.id,
                tab_id: t.id,
                cwd: a.cwd.clone(),
                title: t.title.clone(),
                status: status.into(),
                focused_at: a.last_focused_at,
                proc: proc_name(a),
                branch,
                changes,
            });
            if !a.cwd.is_empty() {
                cwds.insert(a.cwd.clone());
            }
        }
    }
    tabs.sort_by(|x, y| y.focused_at.partial_cmp(&x.focused_at).unwrap_or(std::cmp::Ordering::Equal));
    Ok((tabs, cwds))
}

/// The window behind the matou overlay — what "mirror this tab" targets.
///
/// The overlay always lives in the *same tab* as the window it covers, so we locate our own
/// tab (the one containing `self_id`) and pick its most-recently-focused non-matou window.
/// This is deliberately independent of the `is_focused`/`is_active` flags: a `kitty @ launch`
/// overlay doesn't always leave those set the way interactive focus does, and relying on them
/// resolved to an unrelated window (the black-screen-on-share bug).
pub fn source_window(tree: &[OsWindow], self_id: i64) -> Option<i64> {
    for ow in tree {
        for t in &ow.tabs {
            if !t.windows.iter().any(|w| w.id == self_id) {
                continue; // not our tab
            }
            let mut best: Option<(f64, i64)> = None;
            for w in &t.windows {
                if w.id == self_id || w.user_vars.get("matou").map(String::as_str) == Some("1") {
                    continue;
                }
                if best.is_none_or(|(b, _)| w.last_focused_at > b) {
                    best = Some((w.last_focused_at, w.id));
                }
            }
            if let Some((_, id)) = best {
                return Some(id);
            }
        }
    }
    None
}

/// Open a fresh background tab (a shell) and return its window id — for "new stream tab".
pub fn new_tab() -> Option<i64> {
    capture(&["launch", "--type=tab", "--cwd=current", "--keep-focus"]).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: i64, focused_at: f64, matou: bool) -> Win {
        let mut w = Win { id, last_focused_at: focused_at, ..Default::default() };
        if matou {
            w.user_vars.insert("matou".into(), "1".into());
        }
        w
    }
    fn tab(id: i64, active: bool, windows: Vec<Win>) -> Tab {
        Tab { id, is_active: active, windows, ..Default::default() }
    }

    // The overlay (self) and the source share a tab; we resolve the source by *that* membership,
    // not by focus flags. Here the active/focused flags all point at a different tab on purpose.
    #[test]
    fn source_window_resolves_by_overlay_tab_not_focus_flags() {
        let tree = vec![OsWindow {
            is_focused: false, // no OS window flagged focused — the flag-based logic would miss
            tabs: vec![
                tab(1, true, vec![win(3, 100.0, false)]), // "active" tab, but matou isn't here
                tab(2, false, vec![win(20, 50.0, true), win(19, 40.0, false)]), // matou(20) + source(19)
            ],
        }];
        assert_eq!(source_window(&tree, 20), Some(19));
    }

    // Among several siblings, pick the most-recently-focused non-matou window.
    #[test]
    fn source_window_picks_most_recent_sibling() {
        let tree = vec![OsWindow {
            is_focused: true,
            tabs: vec![tab(
                1,
                true,
                vec![win(20, 10.0, true), win(7, 30.0, false), win(8, 90.0, false)],
            )],
        }];
        assert_eq!(source_window(&tree, 20), Some(8));
    }

    // matou alone in its tab → nothing to mirror.
    #[test]
    fn source_window_none_when_overlay_is_alone() {
        let tree = vec![OsWindow {
            is_focused: true,
            tabs: vec![tab(1, true, vec![win(20, 10.0, true)])],
        }];
        assert_eq!(source_window(&tree, 20), None);
    }
}
