//! The palette model — rows (open tabs + projects), filter/cursor, modes, and the agent state.
//! Ported from the Go (a value-style Bubble Tea model) to an imperative `&mut self` struct.

use crate::{cache, git, kitty, palette, sources};
use std::collections::{HashMap, HashSet};

/// One palette row.
#[derive(Clone, Default)]
pub struct Item {
    pub kind: String, // "open" | "project"
    pub dir: String,
    pub win_id: i64,
    pub tab_id: i64,
    pub title: String,
    pub status: String, // open: focused | running | idle | failed
    pub proc: String,
    pub changes: usize,
}

impl Item {
    pub fn filter_str(&self) -> String {
        let mut s = format!("{} {}", self.title, self.dir);
        if !self.dir.is_empty() {
            s.push(' ');
            s.push_str(basename(&self.dir));
        }
        s.to_lowercase()
    }
}

/// With no query, cap the project long-tail to the most-recent handful (open tabs always show).
pub const TOP_PROJECTS: usize = 10;

#[derive(Default)]
pub struct Model {
    pub all: Vec<Item>,
    pub view: Vec<usize>, // indices into `all` matching the query
    pub query: String,
    pub cur: usize,

    pub mode: String, // "" nav | filter | layout | rename | move | agent
    pub layouts: Vec<palette::Layout>,
    pub lay_cur: usize,
    pub lay_dir: String,
    pub lay_share: bool, // layout picker was opened by `s` → mirror the new tab once built

    pub rtab: i64,
    pub rinput: String,
    pub cwd: String, // launch dir, for the relayout key (.)

    pub move_src: i64,
    pub move_src_tab: i64,
    pub move_src_name: String,

    pub preview: String,
    pub cache: HashMap<String, String>,

    pub agent_input: String,
    pub agent_dir: String,
    pub agent_name: String,
    pub agent_result: String,
    pub agent_working: bool,
    pub agent_off: usize,
    pub agent_focus: String, // "input" | "read"
    pub reply_cache: HashMap<String, String>,
    pub working_dirs: HashSet<String>,
    pub last_instr: HashMap<String, String>,

    pub w: u16,
    pub h: u16,
    pub status: String,
    pub err: String,
}

impl Model {
    /// Build a model: load layouts + the agent cache, then reload rows from kitty + sources.
    pub fn new() -> Self {
        let store = cache::load();
        let mut m = Model {
            layouts: palette::load_layouts(),
            agent_focus: "input".into(),
            reply_cache: store.replies,
            last_instr: store.last_instr,
            ..Default::default()
        };
        m.reload();
        m
    }

    /// Rebuild rows from `kitty @ ls` + project sources, preserving query/cursor.
    pub fn reload(&mut self) {
        let (tabs, open_cwds) = match kitty::open_tabs() {
            Ok(v) => v,
            Err(_) => {
                self.err = "kitty @ ls failed — is remote control on?".into();
                return;
            }
        };
        self.err.clear();
        if let Ok(cwd) = std::env::current_dir() {
            self.cwd = cwd.to_string_lossy().into_owned();
        }
        let mut all = Vec::with_capacity(tabs.len());
        for t in tabs {
            all.push(Item {
                kind: "open".into(),
                dir: t.cwd,
                win_id: t.win_id,
                tab_id: t.tab_id,
                title: t.title,
                status: t.status,
                proc: t.proc,
                changes: t.changes,
            });
        }
        for d in sources::project_dirs(&open_cwds) {
            all.push(Item { kind: "project".into(), dir: d, ..Default::default() });
        }
        self.all = all;
        self.apply_filter();
        self.refresh_preview();
    }

    /// Recompute the visible rows for the query (case-insensitive substring). Empty query shows
    /// everything except the project long-tail; move mode lists open tabs only.
    pub fn apply_filter(&mut self) {
        if self.mode == "move" {
            self.view = self
                .all
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    it.kind == "open" && !(self.move_src != 0 && it.tab_id == self.move_src_tab)
                })
                .map(|(i, _)| i)
                .collect();
            self.cur = clamp(self.cur, self.view.len());
            return;
        }
        let q = self.query.trim().to_lowercase();
        let mut view = Vec::with_capacity(self.all.len());
        let mut projects = 0usize;
        for (i, it) in self.all.iter().enumerate() {
            if !q.is_empty() {
                if it.filter_str().contains(&q) {
                    view.push(i);
                }
                continue;
            }
            if it.kind == "project" {
                if projects >= TOP_PROJECTS {
                    continue;
                }
                projects += 1;
            }
            view.push(i);
        }
        self.view = view;
        self.cur = clamp(self.cur, self.view.len());
    }

    pub fn sel(&self) -> Option<&Item> {
        self.view.get(self.cur).map(|&i| &self.all[i])
    }

    /// Compute (and cache) the right-pane preview for the current selection.
    pub fn refresh_preview(&mut self) {
        if self.mode == "layout" {
            self.preview.clear(); // the sketch is rendered live, sized to the pane
            return;
        }
        let dir = self.sel().map(|it| it.dir.clone()).filter(|d| !d.is_empty());
        match dir {
            Some(d) => {
                let key = format!("dir:{d}");
                if let Some(c) = self.cache.get(&key) {
                    self.preview = c.clone();
                } else {
                    let v = dir_preview(&d);
                    self.cache.insert(key, v.clone());
                    self.preview = v;
                }
            }
            None => self.preview.clear(),
        }
    }

    /// Persist agent replies + last instructions.
    pub fn save_cache(&self) {
        cache::save(&cache::AgentStore {
            replies: self.reply_cache.clone(),
            last_instr: self.last_instr.clone(),
        });
    }
}

pub fn clamp(v: usize, n: usize) -> usize {
    if n == 0 {
        0
    } else if v >= n {
        n - 1
    } else {
        v
    }
}

pub fn basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

/// Right-pane preview for a directory: a REPO section (git) + a FILES listing. Section heads are
/// marked with "▌ " for the view to style; bodies are plain text.
pub fn dir_preview(dir: &str) -> String {
    let base = basename(dir);
    let mut s = String::from("▌ REPO\n");
    let (branch, changes, repo) = git::git_status(dir);
    if repo {
        s.push_str(&format!("{base} · {branch} · {changes} changes\n"));
    } else {
        s.push_str(&format!("{base} · not a git repo\n"));
    }
    s.push_str("\n▌ FILES\n");
    if let Ok(o) = std::process::Command::new("ls").args(["-1A", dir]).output() {
        s.push_str(&String::from_utf8_lossy(&o.stdout));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_bounds() {
        assert_eq!(clamp(5, 3), 2);
        assert_eq!(clamp(0, 0), 0);
        assert_eq!(clamp(2, 5), 2);
    }

    #[test]
    fn filter_caps_projects_then_searches_all() {
        let mut m = Model::default();
        m.all.push(Item { kind: "open".into(), title: "work".into(), dir: "/a".into(), ..Default::default() });
        for i in 0..15 {
            m.all.push(Item { kind: "project".into(), dir: format!("/p/proj{i}"), ..Default::default() });
        }
        m.apply_filter();
        assert_eq!(m.view.len(), 1 + TOP_PROJECTS); // open + capped projects

        m.query = "proj7".into();
        m.apply_filter();
        assert_eq!(m.view.len(), 1); // searches the full set
    }

    #[test]
    fn move_mode_lists_open_only() {
        let mut m = Model { mode: "move".into(), ..Default::default() };
        m.all.push(Item { kind: "open".into(), tab_id: 1, ..Default::default() });
        m.all.push(Item { kind: "open".into(), tab_id: 2, ..Default::default() });
        m.all.push(Item { kind: "project".into(), dir: "/p".into(), ..Default::default() });
        m.move_src = 9;
        m.move_src_tab = 1; // exclude tab 1
        m.apply_filter();
        assert_eq!(m.view.len(), 1); // only the open tab on tab 2
    }
}
