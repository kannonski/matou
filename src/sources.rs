//! Project directory sources: zoxide frecency + the usual project globs, minus already-open tabs.

use std::collections::HashSet;
use std::process::Command;

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// Project dirs, frecency-first (zoxide), then globbed project roots + a couple of fixed dirs,
/// de-duplicated and excluding directories already open as tabs.
pub fn project_dirs(open_cwds: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |d: String, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if !d.is_empty() && !open_cwds.contains(&d) && seen.insert(d.clone()) {
            out.push(d);
        }
    };

    // zoxide frecency
    if let Ok(o) = Command::new("zoxide").args(["query", "-l"]).output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let d = line.trim();
                if !d.is_empty() {
                    push(d.to_string(), &mut out, &mut seen);
                }
            }
        }
    }

    // project roots
    let h = home();
    for root in [format!("{h}/Project/gitlab"), format!("{h}/Project/github")] {
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut dirs: Vec<String> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().to_string_lossy().into_owned())
                .collect();
            dirs.sort();
            for d in dirs {
                push(d, &mut out, &mut seen);
            }
        }
    }

    // fixed dirs
    for d in [format!("{h}/.local/share/chezmoi"), format!("{h}/.config/nvim")] {
        if std::path::Path::new(&d).is_dir() {
            push(d, &mut out, &mut seen);
        }
    }

    out
}
