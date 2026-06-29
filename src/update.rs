//! The state machine: route events to the active mode and mutate the model. Ported from update.go
//! (Bubble Tea's Update) to an imperative `update(&mut Model, Msg) -> keep_running` over a channel.

use crate::model::{self, Model, clamp};
use crate::{config, hooks, kitty, palette};
use crossterm::event::{KeyCode::*, KeyEvent, KeyEventKind, KeyModifiers};
use std::sync::mpsc::Sender;

/// Events the app loop delivers to `update`.
pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    /// An async agent reply landed.
    Agent { dir: String, instr: String, reply: String },
}

/// Returns `false` when the app should quit.
pub fn update(m: &mut Model, msg: Msg, tx: &Sender<Msg>) -> bool {
    match msg {
        Msg::Resize(w, h) => {
            m.w = w;
            m.h = h;
            true
        }
        Msg::Agent { dir, instr, reply } => {
            handle_agent_reply(m, dir, instr, reply);
            true
        }
        Msg::Key(k) => {
            if k.kind == KeyEventKind::Release {
                return true;
            }
            m.status.clear();
            match m.mode.as_str() {
                "filter" => update_filter(m, k),
                "layout" => update_layout(m, k),
                "move" => update_move(m, k),
                "agent" => update_agent(m, k, tx),
                "rename" => update_rename(m, k),
                _ => update_nav(m, k),
            }
        }
    }
}

fn ctrl(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL)
}

/// Act on the current selection: jump to an open tab (and quit), or open the layout picker for a
/// project. Returns true if the app should quit.
fn act_on(m: &mut Model) -> bool {
    let (kind, win_id, dir) = match m.sel() {
        Some(it) => (it.kind.clone(), it.win_id, it.dir.clone()),
        None => return false,
    };
    if kind == "open" {
        kitty::focus_window(win_id);
        return true;
    }
    m.lay_dir = dir;
    m.mode = "layout".into();
    m.lay_cur = 0;
    m.refresh_preview();
    false
}

/// Share the current selection into kittyweb. An already-open tab is mirrored in place (you keep
/// using it in the terminal too) — `owned = false`. A project (not open yet) opens in a **hidden
/// OS window** the daemon owns and tears down on exit — `owned = true` — so the workspace never
/// shows up as a kitty tab. Returns true if the app should quit.
fn share_on(m: &mut Model) -> bool {
    let (kind, win_id, dir) = match m.sel() {
        Some(it) => (it.kind.clone(), it.win_id, it.dir.clone()),
        None => return false,
    };
    let (window, owned) = if kind == "open" {
        (win_id, false)
    } else {
        match kitty::new_hidden_oswindow_in(&dir) {
            Some(w) => (w, true),
            None => {
                m.status = "couldn't open a workspace".into();
                return false;
            }
        }
    };
    crate::mirror::start_detached(window, 9123, owned);
    true // matou quits; the daemon keeps serving and the browser opens kittyweb
}

fn update_nav(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Char('q'), _) | (Esc, _) | (Char('c'), true) | (Char('h'), false) => return false,
        (Char('j'), false) | (Down, _) | (Char('n'), true) => {
            m.cur = clamp(m.cur + 1, m.view.len());
            m.refresh_preview();
        }
        (Char('k'), false) | (Up, _) | (Char('p'), true) => {
            m.cur = m.cur.saturating_sub(1);
            m.refresh_preview();
        }
        (Char('g'), false) | (Home, _) => {
            m.cur = 0;
            m.refresh_preview();
        }
        (Char('G'), _) | (End, _) => {
            m.cur = clamp(usize::MAX, m.view.len());
            m.refresh_preview();
        }
        (Char('l'), false) | (Enter, _) => return !act_on(m),
        (Char('/'), false) => {
            m.mode = "filter".into();
            m.query.clear();
            m.apply_filter();
            m.refresh_preview();
        }
        (Char('a'), false) => enter_agent(m),
        (Char('m'), false) => {
            m.mode = "move".into();
            m.move_src = 0;
            m.move_src_tab = 0;
            m.apply_filter();
            m.refresh_preview();
        }
        (Char('.'), false) => {
            if !m.cwd.is_empty() {
                m.lay_dir = m.cwd.clone();
                m.mode = "layout".into();
                m.lay_cur = 0;
                m.refresh_preview();
            }
        }
        (Char('x'), false) => close_selected(m),
        (Char('r'), false) => start_rename(m),
        (Char('s'), false) => return !share_on(m),
        _ => {}
    }
    true
}

fn update_filter(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Esc, _) | (Char('c'), true) => {
            m.mode = String::new();
            m.query.clear();
            m.apply_filter();
            m.refresh_preview();
        }
        (Enter, _) => return !act_on(m),
        (Up, _) | (Char('p'), true) => {
            m.cur = m.cur.saturating_sub(1);
            m.refresh_preview();
        }
        (Down, _) | (Char('n'), true) => {
            m.cur = clamp(m.cur + 1, m.view.len());
            m.refresh_preview();
        }
        (Backspace, _) => {
            m.query.pop();
            m.apply_filter();
            m.refresh_preview();
        }
        (Char('u'), true) => {
            m.query.clear();
            m.apply_filter();
            m.refresh_preview();
        }
        (Char(ch), false) => {
            m.query.push(ch);
            m.apply_filter();
            m.refresh_preview();
        }
        _ => {}
    }
    true
}

fn update_layout(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Char('q'), _) | (Char('c'), true) => return false,
        (Esc, _) | (Char('h'), false) => {
            m.mode = String::new();
            m.refresh_preview();
        }
        (Enter, _) | (Char('l'), false) => {
            if let Some(l) = m.layouts.get(m.lay_cur).cloned() {
                let dir = m.lay_dir.clone();
                palette::layout_build(&l, &dir);
                return false;
            }
        }
        (Char('j'), false) | (Down, _) | (Char('n'), true) => {
            m.lay_cur = clamp(m.lay_cur + 1, m.layouts.len());
        }
        (Char('k'), false) | (Up, _) | (Char('p'), true) => {
            m.lay_cur = m.lay_cur.saturating_sub(1);
        }
        _ => {}
    }
    true
}

fn update_move(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Esc, _) | (Char('c'), true) => {
            if m.move_src != 0 {
                m.move_src = 0; // stage B → A
                m.move_src_tab = 0;
            } else {
                m.mode = String::new(); // A → nav
            }
            m.apply_filter();
            m.refresh_preview();
        }
        (Char('j'), false) | (Down, _) | (Char('n'), true) => {
            m.cur = clamp(m.cur + 1, m.view.len());
            m.refresh_preview();
        }
        (Char('k'), false) | (Up, _) | (Char('p'), true) => {
            m.cur = m.cur.saturating_sub(1);
            m.refresh_preview();
        }
        (Enter, _) | (Char('l'), false) => {
            if m.move_src == 0 {
                let pick = m.sel().map(|it| {
                    let base = model::basename(&it.dir).to_string();
                    (it.win_id, it.tab_id, if base.is_empty() { it.title.clone() } else { base })
                });
                if let Some((w, t, name)) = pick {
                    m.move_src = w;
                    m.move_src_tab = t;
                    m.move_src_name = name;
                    m.apply_filter();
                    m.refresh_preview();
                }
            } else if let Some(dest) = m.sel().map(|it| it.tab_id) {
                kitty::move_to_tab(m.move_src, dest);
                return false;
            }
        }
        _ => {}
    }
    true
}

fn update_rename(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Esc, _) | (Char('c'), true) => m.mode = String::new(),
        (Enter, _) => {
            let title = m.rinput.trim().to_string();
            if !title.is_empty() {
                kitty::set_tab_title(m.rtab, &title);
            }
            m.mode = String::new();
            m.reload();
        }
        (Backspace, _) => {
            m.rinput.pop();
        }
        (Char('u'), true) => m.rinput.clear(),
        (Char(ch), false) => m.rinput.push(ch),
        _ => {}
    }
    true
}

fn update_agent(m: &mut Model, k: KeyEvent, tx: &Sender<Msg>) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Esc, _) | (Char('c'), true) => {
            m.mode = String::new();
            m.refresh_preview();
            return true;
        }
        (Tab, _) => {
            if !m.agent_result.is_empty() {
                m.agent_focus = if m.agent_focus == "input" { "read".into() } else { "input".into() };
            }
            return true;
        }
        _ => {}
    }
    if m.agent_focus == "read" {
        update_agent_read(m, k)
    } else {
        update_agent_input(m, k, tx)
    }
}

/// Panel body height (mirrors the view's agent dims) for page scrolling.
fn agent_body_h(m: &Model) -> usize {
    let panel_h = ((m.h as usize).saturating_sub(6)).clamp(8, 24);
    panel_h.saturating_sub(6).max(1)
}

fn update_agent_read(m: &mut Model, k: KeyEvent) -> bool {
    let c = ctrl(&k);
    let body = agent_body_h(m);
    match (k.code, c) {
        (Char('j'), false) | (Down, _) | (Char('n'), true) => m.agent_off += 1,
        (Char('k'), false) | (Up, _) | (Char('p'), true) => m.agent_off = m.agent_off.saturating_sub(1),
        (Char('d'), true) => m.agent_off += body / 2,
        (Char('u'), true) => m.agent_off = m.agent_off.saturating_sub(body / 2),
        (Char('f'), true) | (PageDown, _) | (Char(' '), false) => m.agent_off += body,
        (Char('b'), true) | (PageUp, _) => m.agent_off = m.agent_off.saturating_sub(body),
        (Char('g'), false) | (Home, _) => m.agent_off = 0,
        (Char('G'), _) | (End, _) => m.agent_off = 1 << 30,
        (Char('i'), false) | (Char('a'), false) | (Char('/'), false) | (Enter, _) => {
            m.agent_focus = "input".into()
        }
        _ => {}
    }
    true
}

fn update_agent_input(m: &mut Model, k: KeyEvent, tx: &Sender<Msg>) -> bool {
    let c = ctrl(&k);
    match (k.code, c) {
        (Enter, _) => run_agent(m, tx),
        (Up, _) | (Char('p'), true) => m.agent_off = m.agent_off.saturating_sub(1),
        (Down, _) | (Char('n'), true) => m.agent_off += 1,
        (Backspace, _) => {
            m.agent_input.pop();
        }
        (Char('u'), true) => m.agent_input.clear(),
        (Char(ch), false) => m.agent_input.push(ch),
        _ => {}
    }
    true
}

// ── helpers ───────────────────────────────────────────────────────────────────────────────────

fn enter_agent(m: &mut Model) {
    let dir = match m.sel() {
        Some(it) if !it.dir.is_empty() => it.dir.clone(),
        _ => return,
    };
    if config::agent_hook().is_none() {
        m.status = "set $MATOU_AGENT_CMD".into();
        return;
    }
    m.mode = "agent".into();
    m.agent_name = model::basename(&dir).to_string();
    m.agent_focus = "input".into();
    m.agent_off = 0;
    m.agent_working = false;
    m.agent_input = m.last_instr.get(&dir).cloned().unwrap_or_default();
    let key = format!("{dir}\0{}", m.agent_input);
    m.agent_result = if m.agent_input.is_empty() {
        String::new()
    } else {
        m.reply_cache.get(&key).cloned().unwrap_or_default()
    };
    if !m.agent_result.is_empty() {
        m.agent_focus = "read".into();
    }
    m.agent_dir = dir;
}

fn close_selected(m: &mut Model) {
    let tab = match m.sel() {
        Some(it) if it.kind == "open" => it.tab_id,
        _ => return,
    };
    kitty::close_tab(tab);
    m.reload();
}

fn start_rename(m: &mut Model) {
    let (tab, title) = match m.sel() {
        Some(it) if it.kind == "open" => (it.tab_id, it.title.clone()),
        _ => return,
    };
    m.mode = "rename".into();
    m.rtab = tab;
    m.rinput = title;
}

fn run_agent(m: &mut Model, tx: &Sender<Msg>) {
    let instr = m.agent_input.trim().to_string();
    if instr.is_empty() {
        return;
    }
    let dir = m.agent_dir.clone();
    m.last_instr.insert(dir.clone(), instr.clone());
    let key = format!("{dir}\0{instr}");
    if let Some(reply) = m.reply_cache.get(&key).cloned() {
        m.agent_result = reply;
        m.agent_focus = "read".into();
        m.agent_off = 0;
        m.save_cache();
        return;
    }
    m.agent_working = true;
    m.working_dirs.insert(dir.clone());
    m.save_cache();
    let Some(hook) = config::agent_hook() else { return };
    let txc = tx.clone();
    std::thread::spawn(move || {
        let reply = hooks::run_agent(&hook, &dir, &instr);
        let _ = txc.send(Msg::Agent { dir, instr, reply });
    });
}

fn handle_agent_reply(m: &mut Model, dir: String, instr: String, reply: String) {
    m.reply_cache.insert(format!("{dir}\0{instr}"), reply.clone());
    m.working_dirs.remove(&dir);
    m.save_cache();
    if m.mode == "agent" && m.agent_dir == dir {
        m.agent_result = reply;
        m.agent_working = false;
        m.agent_focus = "read".into();
        m.agent_off = 0;
    }
    m.refresh_preview();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(crossterm::event::KeyCode::Char(c), KeyModifiers::empty())
    }
    fn dummy_tx() -> Sender<Msg> {
        std::sync::mpsc::channel().0
    }

    #[test]
    fn nav_quit_keys() {
        let tx = dummy_tx();
        let mut m = Model::default();
        assert!(!update(&mut m, Msg::Key(key('q')), &tx));
        let mut m = Model::default();
        assert!(!update(&mut m, Msg::Key(key('h')), &tx));
    }

    #[test]
    fn layout_picker_nav() {
        let tx = dummy_tx();
        let mut m = Model::default();
        m.all.push(Item { kind: "project".into(), dir: "/p".into(), ..Default::default() });
        m.view = vec![0];
        m.layouts =
            vec![palette::Layout { name: "a".into(), ..Default::default() }, palette::Layout { name: "b".into(), ..Default::default() }];
        update(&mut m, Msg::Key(key('l')), &tx); // project → layout picker
        assert_eq!(m.mode, "layout");
        update(&mut m, Msg::Key(key('j')), &tx);
        assert_eq!(m.lay_cur, 1);
        update(&mut m, Msg::Key(key('k')), &tx);
        assert_eq!(m.lay_cur, 0);
        update(&mut m, Msg::Key(key('h')), &tx); // back to nav
        assert_eq!(m.mode, "");
    }

    #[test]
    fn move_two_stage() {
        let tx = dummy_tx();
        let mut m = Model::default();
        m.all.push(Item { kind: "open".into(), win_id: 5, tab_id: 1, ..Default::default() });
        m.all.push(Item { kind: "open".into(), win_id: 6, tab_id: 2, ..Default::default() });
        update(&mut m, Msg::Key(key('m')), &tx);
        assert_eq!(m.mode, "move");
        assert_eq!(m.move_src, 0); // stage A
        update(&mut m, Msg::Key(crossterm::event::KeyEvent::new(Enter, KeyModifiers::empty())), &tx);
        assert_ne!(m.move_src, 0); // stage B (a pane picked)
        update(&mut m, Msg::Key(crossterm::event::KeyEvent::new(Esc, KeyModifiers::empty())), &tx);
        assert_eq!(m.move_src, 0); // back to stage A
    }

    #[test]
    fn agent_read_scroll() {
        let tx = dummy_tx();
        let mut m = Model { mode: "agent".into(), agent_focus: "read".into(), agent_result: "x".into(), ..Default::default() };
        update(&mut m, Msg::Key(key('j')), &tx);
        assert_eq!(m.agent_off, 1);
        update(&mut m, Msg::Key(key('g')), &tx);
        assert_eq!(m.agent_off, 0);
        update(&mut m, Msg::Key(key('i')), &tx);
        assert_eq!(m.agent_focus, "input");
    }
}
