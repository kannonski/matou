//! Rendering — ported from view.go (lipgloss strings) to ratatui widgets: a rounded frame with a
//! header + rule, a two-pane body (List of rows | preview / live sketch), a context footer, and a
//! centered agent overlay. Catppuccin colors via `theme`.

use crate::model::{Item, Model};
use crate::{config, palette, theme};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap};

// ── small helpers ───────────────────────────────────────────────────────────────────────────

/// Truncate to `n` chars, replacing the last with `…` when over.
pub fn trunc(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    if n == 0 {
        return String::new();
    }
    let mut out: String = chars[..n - 1].iter().collect();
    out.push('…');
    out
}

fn glyph_rune(it: &Item) -> &'static str {
    if it.kind != "open" {
        return "+";
    }
    match it.status.as_str() {
        "focused" => "●",
        "running" => "⏵",
        "failed" => "✗",
        _ => "○",
    }
}

fn glyph_color(it: &Item) -> Color {
    if it.kind != "open" {
        return theme::OVERLAY0;
    }
    match it.status.as_str() {
        "focused" => theme::GREEN,
        "running" => theme::PEACH,
        "failed" => theme::RED,
        _ => theme::BLUE,
    }
}

fn name_of(it: &Item) -> String {
    let n = crate::model::basename(&it.dir);
    if it.kind == "open" && (n.is_empty() || n == "/" || n == ".") {
        it.title.clone()
    } else {
        n.to_string()
    }
}

fn name_color(it: &Item) -> Color {
    if it.kind == "open" { theme::BLUE } else { theme::SUBTEXT0 }
}

fn item_meta(it: &Item) -> String {
    if it.kind != "open" {
        return String::new();
    }
    let mut s = it.proc.clone();
    if it.changes > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("*{}", it.changes));
    }
    s
}

/// Context-aware footer hints.
fn nav_actions(m: &Model) -> String {
    let mut a: Vec<&str> = Vec::new();
    match m.sel() {
        Some(it) if it.kind == "open" => a.extend(["↵ jump", "x close", "r rename"]),
        Some(_) => a.push("↵ open"),
        None => {}
    }
    a.push("m move");
    a.push("s share");
    if config::agent_hook().is_some() {
        a.push("a ask");
    }
    if !m.cwd.is_empty() {
        a.push(". relayout");
    }
    a.extend(["/ search", "q quit"]);
    a.join(" · ")
}

fn section_head(title: &str) -> Line<'static> {
    Line::from(Span::styled(format!("▌ {title}"), Style::default().fg(theme::LAVENDER).bold()))
}

// ── list rows ───────────────────────────────────────────────────────────────────────────────

fn row_items(m: &Model, width: u16) -> Vec<ListItem<'static>> {
    let zone = (width as usize).saturating_sub(4); // after "  " + glyph + " "
    m.view
        .iter()
        .map(|&i| {
            let it = &m.all[i];
            let mut meta = item_meta(it);
            let mut mlen = meta.chars().count();
            if mlen > 0 && zone.saturating_sub(mlen + 1) < 6 {
                meta.clear();
                mlen = 0;
            }
            let name_max = if mlen > 0 { zone.saturating_sub(mlen + 1) } else { zone };
            let name = trunc(&name_of(it), name_max);
            let pad = zone.saturating_sub(name.chars().count() + mlen);
            let mut spans = vec![
                Span::raw("  "),
                Span::styled(glyph_rune(it), Style::default().fg(glyph_color(it))),
                Span::raw(" "),
                Span::styled(name, Style::default().fg(name_color(it))),
                Span::raw(" ".repeat(pad)),
            ];
            if mlen > 0 {
                spans.push(Span::styled(meta, Style::default().fg(theme::OVERLAY1)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
}

fn layout_items(m: &Model, width: u16) -> Vec<ListItem<'static>> {
    const NAMEW: usize = 7;
    m.layouts
        .iter()
        .map(|l| {
            let nm = format!("{:<NAMEW$}", trunc(&l.name, NAMEW));
            let desc = trunc(&l.caption, (width as usize).saturating_sub(4 + NAMEW).max(1));
            ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(nm, Style::default().fg(theme::MAUVE)),
                Span::raw(" "),
                Span::styled(desc, Style::default().fg(theme::OVERLAY0)),
            ]))
        })
        .collect()
}

fn render_list(m: &Model, f: &mut Frame, area: Rect) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    let (items, sel) = if m.mode == "layout" {
        (layout_items(m, area.width), m.lay_cur)
    } else {
        (row_items(m, area.width), m.cur)
    };
    let n = items.len();
    let list = List::new(items).highlight_style(Style::default().bg(theme::SURFACE0).fg(theme::TEXT));
    let mut st = ListState::default();
    if n > 0 {
        st.select(Some(sel.min(n - 1)));
    }
    f.render_stateful_widget(list, area, &mut st);
}

// ── right pane ──────────────────────────────────────────────────────────────────────────────

fn agent_teaser(question: &str, reply: &str, w: usize) -> Vec<Line<'static>> {
    const MAX: usize = 10;
    let mut out = vec![
        section_head("AGENT"),
        Line::from(Span::styled(format!("🤖 {}", trunc(question, w)), Style::default().fg(theme::MAUVE))),
    ];
    let lines: Vec<&str> = reply.trim_end_matches('\n').lines().collect();
    let clipped = lines.len() > MAX;
    for l in lines.iter().take(MAX) {
        out.push(Line::from(trunc(l, w)));
    }
    if clipped {
        out.push(Line::from(Span::styled("… press a for the full answer", Style::default().fg(theme::OVERLAY0))));
    }
    out
}

fn render_right(m: &Model, f: &mut Frame, area: Rect) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    if m.mode == "layout" {
        if let Some(l) = m.layouts.get(m.lay_cur) {
            let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
            palette::sketch(l, parts[0], f.buffer_mut());
            let cap = trunc(&format!("{} · {}", l.name, l.caption), area.width as usize);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(cap, Style::default().fg(theme::OVERLAY0)))),
                parts[1],
            );
        }
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    if let Some(it) = m.sel() {
        if !it.dir.is_empty() {
            if m.working_dirs.contains(&it.dir) {
                lines.push(section_head("AGENT"));
                lines.push(Line::from(Span::styled("🤖 working…", Style::default().fg(theme::PEACH))));
                lines.push(Line::from(""));
            } else if let Some(li) = m.last_instr.get(&it.dir) {
                if let Some(r) = m.reply_cache.get(&format!("{}\0{}", it.dir, li)) {
                    lines.extend(agent_teaser(li, r, area.width as usize));
                    lines.push(Line::from(""));
                }
            }
        }
    }
    for raw in m.preview.lines() {
        if raw.starts_with("▌ ") {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(theme::LAVENDER).bold(),
            )));
        } else {
            lines.push(Line::from(trunc(raw, area.width as usize)));
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

// ── header + footer ─────────────────────────────────────────────────────────────────────────

fn prompt(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(theme::MAUVE).bold())
}
fn dim(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(theme::OVERLAY0))
}
fn cursor() -> Span<'static> {
    Span::styled("▌", Style::default().fg(theme::PINK).bold())
}

fn header_line(m: &Model) -> Line<'static> {
    match m.mode.as_str() {
        "layout" => Line::from(vec![
            prompt(&format!(
                "{} {}",
                if m.lay_share { "share — layout for" } else { "layout for" },
                crate::model::basename(&m.lay_dir)
            )),
            dim(if m.lay_share { "   ↵ build + share · esc back" } else { "   ↵ build · esc back" }),
        ]),
        "rename" => Line::from(vec![prompt("rename tab ❯ "), Span::raw(m.rinput.clone()), cursor()]),
        "move" => {
            if m.move_src == 0 {
                Line::from(prompt("move which pane?"))
            } else {
                Line::from(prompt(&format!("move {} → which tab?", m.move_src_name)))
            }
        }
        "filter" => Line::from(vec![
            prompt("❯ "),
            Span::raw(m.query.clone()),
            cursor(),
            dim(&format!("   {} match", m.view.len())),
        ]),
        _ => {
            let mut spans = vec![prompt("matou"), dim("   j/k nav · / search")];
            if !m.status.is_empty() {
                spans.push(dim("   "));
                spans.push(Span::styled(m.status.clone(), Style::default().fg(theme::PINK)));
            }
            Line::from(spans)
        }
    }
}

fn footer_line(m: &Model, w: usize) -> Line<'static> {
    let f = match m.mode.as_str() {
        "layout" if m.lay_share => "j/k pick · l/↵ build + share · h back".to_string(),
        "layout" => "j/k pick · l/↵ build · h back".to_string(),
        "rename" => "enter save · esc cancel".to_string(),
        "move" if m.move_src == 0 => "j/k pick · ↵ choose this pane · esc cancel".to_string(),
        "move" => "↵ move into this tab · esc back".to_string(),
        "filter" => "↵ go · esc back to nav".to_string(),
        _ => trunc(&nav_actions(m), w),
    };
    Line::from(dim(&f))
}

// ── frame ────────────────────────────────────────────────────────────────────────────────────

fn left_width(m: &Model, inner_w: u16) -> u16 {
    let iw = inner_w as i32;
    let mut left = (((iw - 1) * 2 / 5).min(52)).max(24);
    if m.mode == "layout" {
        left = left.min(34).max(20);
    }
    if left > iw - 14 {
        left = (iw - 14).max(10);
    }
    left.clamp(8, iw.max(8)) as u16
}

pub fn render(m: &Model, f: &mut Frame) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(theme::BASE)), area);

    if !m.err.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("  {}", m.err), Style::default().fg(theme::RED)))),
            area,
        );
        return;
    }
    if m.mode == "agent" {
        render_agent_panel(m, f, area);
        return;
    }

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .padding(Padding::horizontal(1));
    let inner = outer.inner(area);
    f.render_widget(outer, area);
    if inner.height < 4 {
        return;
    }

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).split(inner);
    f.render_widget(Paragraph::new(header_line(m)), rows[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled("─".repeat(rows[1].width as usize), Style::default().fg(theme::BORDER)))),
        rows[1],
    );

    let left = left_width(m, inner.width);
    let cols = Layout::horizontal([Constraint::Length(left + 1), Constraint::Min(1)]).split(rows[2]);
    let lblock = Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(theme::BORDER));
    let larea = lblock.inner(cols[0]);
    f.render_widget(lblock, cols[0]);
    render_list(m, f, larea);
    let right = Rect { x: cols[1].x + 1, width: cols[1].width.saturating_sub(1), ..cols[1] };
    render_right(m, f, right);

    f.render_widget(Paragraph::new(footer_line(m, inner.width as usize)), rows[3]);
}

/// Approximate wrapped-line count (for clamping the agent scroll, since Paragraph::line_count
/// is private in this ratatui).
fn wrapped_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return text.lines().count().max(1);
    }
    text.lines()
        .map(|l| {
            let w = l.chars().count();
            if w == 0 { 1 } else { w.div_ceil(width) }
        })
        .sum()
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn render_agent_panel(m: &Model, f: &mut Frame, area: Rect) {
    let pw = ((area.width as i32 - 8).clamp(40, 90)) as u16;
    let ph = ((area.height as i32 - 6).clamp(8, 24)) as u16;
    let rect = centered(area, pw, ph);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.height < 4 {
        return;
    }
    let reading = m.agent_focus == "read";
    let parts = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // input
        Constraint::Length(1), // rule
        Constraint::Min(1),    // body
        Constraint::Length(1), // footer
    ])
    .split(inner);

    f.render_widget(Paragraph::new(Line::from(prompt(&format!("🤖 {}", trunc(&m.agent_name, inner.width as usize))))), parts[0]);

    let input = if reading {
        Line::from(dim(&format!("❯ {}", m.agent_input)))
    } else {
        Line::from(vec![prompt("❯ "), Span::raw(m.agent_input.clone()), cursor()])
    };
    f.render_widget(Paragraph::new(input), parts[1]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled("─".repeat(parts[2].width as usize), Style::default().fg(theme::BORDER)))),
        parts[2],
    );

    let body = parts[3];
    if m.agent_working {
        f.render_widget(Paragraph::new(Line::from(Span::styled("🤖 working…", Style::default().fg(theme::PEACH)))), body);
    } else if m.agent_result.is_empty() {
        f.render_widget(Paragraph::new(Line::from(dim("type a question, then enter"))), body);
    } else {
        let total = wrapped_count(&m.agent_result, body.width as usize);
        let off = m.agent_off.min(total.saturating_sub(body.height as usize)) as u16;
        let para = Paragraph::new(m.agent_result.clone()).wrap(Wrap { trim: false }).scroll((off, 0));
        f.render_widget(para, body);
    }

    let foot = match (reading, m.agent_result.is_empty()) {
        (true, _) => "j/k scroll · ^d/^u half · g/G ends · i ask · esc",
        (false, false) => "enter ask · tab read · esc close",
        (false, true) => "enter ask · esc close",
    };
    f.render_widget(Paragraph::new(Line::from(dim(foot))), parts[4]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn trunc_adds_ellipsis() {
        assert_eq!(trunc("hello", 10), "hello");
        assert_eq!(trunc("hello world", 5), "hell…");
        assert_eq!(trunc("hi", 0), "");
    }

    #[test]
    fn agent_teaser_caps_at_ten() {
        let reply = (0..15).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let t = agent_teaser("q?", &reply, 40);
        // section head + question + 10 reply lines + clip hint = 13
        assert_eq!(t.len(), 13);
        let text: String = t.iter().flat_map(|l| l.spans.iter().map(|s| s.content.to_string())).collect();
        assert!(text.contains("line9"));
        assert!(!text.contains("line10"));
        assert!(text.contains("press a"));
    }

    #[test]
    fn renders_rows_and_footer() {
        let mut m = Model::default();
        m.w = 80;
        m.h = 24;
        m.all.push(Item { kind: "project".into(), dir: "/home/u/widget".into(), ..Default::default() });
        m.apply_filter();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(&m, f)).unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("widget"), "row name not rendered");
        assert!(text.contains("matou"), "header not rendered");
        assert!(text.contains("q quit"), "footer not rendered");
    }
}
