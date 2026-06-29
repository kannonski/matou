//! The layout engine: parse `palette.layouts`, normalize pane sizes, lex commands, build a layout
//! in kitty, and sketch a preview. Parser/lexer/normalizer are ported here; `layout_build` and
//! `sketch` are stubbed and ported faithfully from the Go next (the split-bias math + the ASCII
//! diagram).

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Layout {
    pub name: String,
    pub shape: String,
    pub panes: Vec<String>,
    pub ratio: Vec<f64>,
    pub caption: String,
    pub order: i64,
}

pub fn layouts_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("MATOU_LAYOUTS") {
        return p.into();
    }
    if let Ok(p) = std::env::var("KITTY_PALETTE_LAYOUTS") {
        return p.into();
    }
    let h = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(h).join(".config/kitty/palette.layouts")
}

pub fn load_layouts() -> Vec<Layout> {
    let data = std::fs::read_to_string(layouts_path()).unwrap_or_default();
    parse_layouts(&data)
}

fn toml_str(v: &str) -> String {
    v.trim().trim_matches('"').to_string()
}
fn toml_int(v: &str) -> i64 {
    v.trim().parse().unwrap_or(0)
}
fn toml_float_array(v: &str) -> Vec<f64> {
    v.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect()
}
/// Each double-quoted string in order (ignores brackets/commas/whitespace outside quotes), so
/// `["nvim {dir}", "npm run dev"]` → ["nvim {dir}", "npm run dev"].
fn toml_str_array(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    for c in v.chars() {
        match c {
            '"' if in_str => {
                out.push(std::mem::take(&mut cur));
                in_str = false;
            }
            '"' => in_str = true,
            _ if in_str => cur.push(c),
            _ => {}
        }
    }
    out
}

/// Parse the layouts file body (a tiny TOML subset). Single-pane layouts are forced to "single";
/// result is sorted by (order, name).
pub fn parse_layouts(data: &str) -> Vec<Layout> {
    let mut out: Vec<Layout> = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            out.push(Layout { name: name.trim().to_string(), ..Default::default() });
        } else if let Some((k, val)) = line.split_once('=') {
            if let Some(l) = out.last_mut() {
                match k.trim() {
                    "shape" => l.shape = toml_str(val),
                    "panes" => l.panes = toml_str_array(val),
                    "ratio" => l.ratio = toml_float_array(val),
                    "caption" => l.caption = toml_str(val),
                    "order" => l.order = toml_int(val),
                    _ => {}
                }
            }
        }
    }
    for l in &mut out {
        if l.panes.len() == 1 {
            l.shape = "single".to_string();
        }
    }
    out.sort_by(|a, b| a.order.cmp(&b.order).then(a.name.cmp(&b.name)));
    out
}

/// Bash-like lexing honoring single and double quotes. `npm "run dev"` → ["npm", "run dev"].
pub fn shlex_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                for c2 in chars.by_ref() {
                    if c2 == '\'' {
                        break;
                    }
                    cur.push(c2);
                }
            }
            '"' => {
                started = true;
                for c2 in chars.by_ref() {
                    if c2 == '"' {
                        break;
                    }
                    cur.push(c2);
                }
            }
            _ => {
                started = true;
                cur.push(c);
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Normalize `sizes` to `n` percentages: leading values are kept, remaining panes split what's
/// left equally; with no sizes, all split equally. `norm([60], 3)` → `[60, 20, 20]`.
pub fn norm(sizes: &[f64], n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![];
    }
    let mut out = vec![0.0; n];
    let k = sizes.len().min(n);
    let mut used = 0.0;
    for i in 0..k {
        out[i] = sizes[i];
        used += sizes[i];
    }
    let rem = n - k;
    if rem > 0 {
        let each = (100.0 - used).max(0.0) / rem as f64;
        for v in out.iter_mut().skip(k) {
            *v = each;
        }
    }
    out
}

// ── build: launch the real panes via kitty @ (ported 1:1 from the Go) ──────────────────────────

fn argv_for(cmd: &str, dir: &str) -> Vec<String> {
    shlex_split(&cmd.replace("{dir}", dir))
}

/// `kitty @ launch …` returning the new window id; retries if the id comes back empty (kitty can
/// briefly not have registered a just-created window — a race the fast Rust hits).
fn launch_capture(args: &[String]) -> String {
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    for attempt in 0..3 {
        let out = crate::kitty::capture(&argv);
        if !out.is_empty() {
            return out;
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
    }
    String::new()
}

/// `kitty @ launch --type=tab …` for the first pane; returns the new window id.
pub fn launch_tab(cmd: &str, title: &str, dir: &str) -> String {
    let mut args: Vec<String> =
        vec!["launch".into(), "--type=tab".into(), "--tab-title".into(), title.into(), "--cwd".into(), dir.into(), "--".into()];
    args.extend(argv_for(cmd, dir));
    launch_capture(&args)
}

/// `kitty @ launch --location=<loc> --bias=<n> --next-to id:<prev> …`; returns the new window id.
/// Requires a non-empty `next_to`: without it kitty would split the *active* window (the matou
/// overlay), leaving a stray blank window behind — so we refuse rather than guess.
pub fn launch_split(loc: &str, bias: f64, next_to: &str, cmd: &str, dir: &str) -> String {
    if next_to.is_empty() {
        return String::new();
    }
    let mut args: Vec<String> = vec![
        "launch".into(),
        format!("--location={loc}"),
        format!("--bias={}", bias.round() as i64),
        "--next-to".into(),
        format!("id:{next_to}"),
        "--cwd".into(),
        dir.into(),
        "--".into(),
    ];
    args.extend(argv_for(cmd, dir));
    launch_capture(&args)
}

/// Lay cmds[1..] along one axis next to cmds[0] (already launched as window `first`). bias for the
/// i-th split = (sum of sizes from i onward) / (sum from i-1 onward) — the fraction of the current
/// combined space the new subtree gets.
fn chain(first: &str, cmds: &[String], sizes: &[f64], loc: &str, dir: &str) {
    let ws = norm(sizes, cmds.len());
    let mut prev = first.to_string();
    for i in 1..cmds.len() {
        if prev.is_empty() {
            break; // a split failed → stop rather than blind-split the active window
        }
        let num: f64 = ws[i..].iter().sum();
        let den: f64 = ws[i - 1..].iter().sum();
        let bias = if den != 0.0 { num / den * 100.0 } else { 0.0 };
        prev = launch_split(loc, bias, &prev, &cmds[i], dir);
    }
}

fn main_ratio(l: &Layout) -> f64 {
    l.ratio.first().copied().unwrap_or(60.0)
}
fn rest_ratio(l: &Layout) -> Vec<f64> {
    if l.ratio.len() > 1 { l.ratio[1..].to_vec() } else { vec![] }
}

/// Launch the layout's panes in a new tab cwd'd to `dir`, then focus the editor + record frecency.
/// Returns the editor (main pane) window id — the window to mirror when sharing the new tab.
pub fn layout_build(l: &Layout, dir: &str) -> Option<i64> {
    if l.panes.is_empty() {
        return None;
    }
    let base = dir.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let title = if base.is_empty() { dir.to_string() } else { base.to_string() };
    let ed = launch_tab(&l.panes[0], &title, dir);
    if ed.is_empty() {
        return None; // editor tab failed — don't chain (a chain off "" would blind-split the overlay)
    }
    match l.shape.as_str() {
        "single" => {}
        "columns" | "main+right" => chain(&ed, &l.panes, &l.ratio, "vsplit", dir),
        "rows" | "main+bottom" => chain(&ed, &l.panes, &l.ratio, "hsplit", dir),
        "main+rightstack" if l.panes.len() > 1 => {
            let right = launch_split("vsplit", 100.0 - main_ratio(l), &ed, &l.panes[1], dir);
            chain(&right, &l.panes[1..], &rest_ratio(l), "hsplit", dir);
        }
        "main+bottomrow" if l.panes.len() > 1 => {
            let bot = launch_split("hsplit", 100.0 - main_ratio(l), &ed, &l.panes[1], dir);
            chain(&bot, &l.panes[1..], &rest_ratio(l), "vsplit", dir);
        }
        _ => {}
    }
    let _ = std::process::Command::new("kitty")
        .args(["@", "focus-window", "--match", &format!("id:{ed}")])
        .status();
    let _ = std::process::Command::new("zoxide").args(["add", dir]).status();
    ed.parse().ok()
}

// ── tool recognition (icon + accent per command) ──────────────────────────────────────────────

pub struct Tool {
    pub icon: &'static str,
    pub kind: &'static str,
    pub accent: ratatui::style::Color,
}

/// Basename of a command's first word (the tool name); "sh" if empty.
pub fn label_of(cmd: &str) -> String {
    let toks = shlex_split(cmd);
    let first = toks.first().map(String::as_str).unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or("");
    if base.is_empty() { "sh".to_string() } else { base.to_string() }
}

pub fn appear(cmd: &str) -> Tool {
    use crate::theme::*;
    let (icon, kind, accent): (&str, &str, ratatui::style::Color) = match label_of(cmd).as_str() {
        "claude" => ("\u{f1a90}", "sh", PEACH),
        "nvim" | "vim" | "vi" => ("\u{e6ae}", "nvim", GREEN),
        "lazygit" | "gitui" | "git" => ("\u{e702}", "git", MAUVE),
        "k9s" | "kubectl" => ("\u{f10fe}", "k9s", TEAL),
        "lazydocker" | "docker" => ("\u{f308}", "logs", SAPPHIRE),
        "stern" | "tail" | "btop" | "htop" => ("\u{f0f6}", "logs", PEACH),
        "go" => ("\u{e627}", "sh", TEAL),
        "npm" => ("\u{e71e}", "sh", RED),
        "zsh" | "bash" | "fish" | "sh" => ("\u{e795}", "sh", BLUE),
        _ => ("\u{e795}", "sh", BLUE),
    };
    Tool { icon, kind, accent }
}

// ── sketch: render the layout preview with ratatui (Layout split → bordered Block per pane) ────

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

fn split(area: Rect, dir: Direction, pcts: &[f64]) -> Vec<Rect> {
    let cons: Vec<Constraint> =
        pcts.iter().map(|p| Constraint::Percentage(p.round().clamp(0.0, 100.0) as u16)).collect();
    RLayout::default().direction(dir).constraints(cons).split(area).to_vec()
}

/// Sub-rectangles for the panes, matching layout_build's shape semantics.
fn pane_rects(l: &Layout, area: Rect) -> Vec<Rect> {
    let n = l.panes.len();
    match l.shape.as_str() {
        "columns" | "main+right" => split(area, Direction::Horizontal, &norm(&l.ratio, n)),
        "rows" | "main+bottom" => split(area, Direction::Vertical, &norm(&l.ratio, n)),
        "main+rightstack" if n > 1 => {
            let cols = split(area, Direction::Horizontal, &[main_ratio(l), 100.0 - main_ratio(l)]);
            let mut out = vec![cols[0]];
            out.extend(split(cols[1], Direction::Vertical, &norm(&rest_ratio(l), n - 1)));
            out
        }
        "main+bottomrow" if n > 1 => {
            let rows = split(area, Direction::Vertical, &[main_ratio(l), 100.0 - main_ratio(l)]);
            let mut out = vec![rows[0]];
            out.extend(split(rows[1], Direction::Horizontal, &norm(&rest_ratio(l), n - 1)));
            out
        }
        _ => vec![area], // single (or degenerate)
    }
}

fn draw_pane(rect: Rect, cmd: &str, buf: &mut Buffer) {
    if rect.width < 3 || rect.height < 2 {
        return;
    }
    let t = appear(cmd);
    let title = Line::from(Span::styled(
        format!(" {} {} ", t.icon, label_of(cmd)),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    ));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(crate::theme::BORDER))
        .title(title);
    let inner = block.inner(rect);
    block.render(rect, buf);
    let body: &[&str] = match t.kind {
        "nvim" => &["~", "~", "~"],
        "git" => &["● ●", "  ●"],
        "k9s" => &["▤ ▤ ▤"],
        "logs" => &["—", "—", "—"],
        _ => &["\u{276f} "], // shell prompt ❯
    };
    for (i, line) in body.iter().enumerate() {
        if (i as u16) < inner.height {
            buf.set_string(inner.x, inner.y + i as u16, line, Style::default().fg(crate::theme::SUBTEXT));
        }
    }
}

/// Render the layout preview into `area`.
pub fn sketch(l: &Layout, area: Rect, buf: &mut Buffer) {
    for (rect, cmd) in pane_rects(l, area).iter().zip(&l.panes) {
        draw_pane(*rect, cmd, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# matou layouts
[zsh]
shape   = "single"
panes   = ["zsh"]
caption = "just a shell"
order   = 0

[dev]
shape   = "main+rightstack"
panes   = ["nvim {dir}", "zsh", "lazygit"]
ratio   = [62, 50]
caption = "editor · shell · lazygit"
order   = 1

[k8s]
shape   = "main+right"
panes   = ["k9s", "zsh"]
ratio   = [60]
caption = "k9s · shell"
order   = 3
"#;

    #[test]
    fn parses_and_sorts() {
        let ls = parse_layouts(SAMPLE);
        assert_eq!(ls.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["zsh", "dev", "k8s"]);
        let dev = &ls[1];
        assert_eq!(dev.shape, "main+rightstack");
        assert_eq!(dev.panes, ["nvim {dir}", "zsh", "lazygit"]);
        assert_eq!(dev.ratio, [62.0, 50.0]);
        assert_eq!(dev.caption, "editor · shell · lazygit");
    }

    #[test]
    fn single_pane_forced_single() {
        let ls = parse_layouts(SAMPLE);
        assert_eq!(ls[0].name, "zsh");
        assert_eq!(ls[0].shape, "single");
    }

    #[test]
    fn shlex_quotes() {
        assert_eq!(shlex_split("npm \"run dev\""), ["npm", "run dev"]);
        assert_eq!(shlex_split("nvim {dir}"), ["nvim", "{dir}"]);
        assert_eq!(shlex_split("git 'commit -m x'"), ["git", "commit -m x"]);
    }

    #[test]
    fn norm_splits_remainder() {
        assert_eq!(norm(&[60.0], 3), [60.0, 20.0, 20.0]);
        assert_eq!(norm(&[], 2), [50.0, 50.0]);
        assert_eq!(norm(&[62.0, 50.0], 3), [62.0, 50.0, 0.0]); // 100-112 clamped → 0 leftover
    }

    #[test]
    fn sketch_draws_pane_labels() {
        let l = Layout {
            name: "dev".into(),
            shape: "main+rightstack".into(),
            panes: vec!["nvim {dir}".into(), "zsh".into(), "lazygit".into()],
            ratio: vec![62.0, 50.0],
            caption: String::new(),
            order: 0,
        };
        let area = Rect::new(0, 0, 60, 16);
        let mut buf = Buffer::empty(area);
        sketch(&l, area, &mut buf);
        let mut s = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(s.contains("nvim"), "sketch missing nvim pane:\n{s}");
        assert!(s.contains("zsh"), "sketch missing zsh pane");
        assert!(s.contains("lazygit"), "sketch missing lazygit pane");
    }
}
