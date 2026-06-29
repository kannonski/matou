//! The `a` (ask) agent hook — shells out to `$MATOU_AGENT_CMD <dir> "<instr>"`.

use std::process::Command;

/// Run the hook with `dir` + `instr` as args (via `sh -c` so a multi-word hook works). Returns
/// stdout (trailing newline trimmed), or a `⚠ agent failed (…)` message.
pub fn run_agent(hook: &str, dir: &str, instr: &str) -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{hook} \"$@\""))
        .arg("sh") // $0
        .arg(dir)
        .arg(instr)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
        Ok(o) => format!("⚠ agent failed ({})", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => format!("⚠ agent failed ({e})"),
    }
}
