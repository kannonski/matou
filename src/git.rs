//! git status for the preview pane / open-tab metadata.

use std::process::Command;

fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `(branch, changes, is_repo)` for `dir`. branch is "detached" when there's no current branch.
pub fn git_status(dir: &str) -> (String, usize, bool) {
    if git(dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return (String::new(), 0, false);
    }
    let branch = match git(dir, &["branch", "--show-current"]) {
        Some(b) if !b.is_empty() => b,
        _ => "detached".to_string(),
    };
    let changes = git(dir, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    (branch, changes, true)
}
