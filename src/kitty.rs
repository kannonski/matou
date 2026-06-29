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

/// Open a fresh tab (a shell) cwd'd to `dir` and return its window id — the seed pane when sharing
/// a not-yet-open project into kittyweb. `--keep-focus` so it doesn't yank focus from the overlay.
pub fn new_tab_in(dir: &str) -> Option<i64> {
    capture(&["launch", "--type=tab", "--cwd", dir, "--keep-focus"]).parse().ok()
}

/// Open a shell in a brand-new OS window cwd'd to `dir`, **hide that OS window**, and return its
/// window id. This is kittyweb's "new workspace": the panes live in a hidden OS window driven only
/// from the browser, so there's no tab in kitty to see — or accidentally mess with.
pub fn new_hidden_oswindow_in(dir: &str) -> Option<i64> {
    let id: i64 = capture(&["launch", "--type=os-window", "--cwd", dir, "--keep-focus"]).parse().ok()?;
    let _ = Command::new("kitty")
        .args(["@", "resize-os-window", "--action", "hide", "--match", &format!("id:{id}")])
        .status();
    Some(id)
}

