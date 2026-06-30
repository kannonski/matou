//! matou — kitty project launcher (palette + layouts) with a browser/P2P tab mirror.
//! Rust/ratatui rewrite. The legacy Go is on the `go-legacy` branch.


mod cache;
mod config;
mod git;
mod hooks;
mod kitty;
mod mirror;
mod mirror_p2p;
mod mirror_wt;
mod model;
mod palette;
mod sources;
mod theme;
mod update;
mod view;

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use model::Model;
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use std::io::stdout;
use std::sync::mpsc;
use update::{Msg, update};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("mirror") => return mirror::run(&args[2..]),
        Some("mirror-open") => return mirror_p2p::open(args.get(2)),
        Some("web") => return launch_web(),
        Some("-once") | Some("--once") => return render_once(),
        _ => {}
    }
    // require kitty remote control, and self-toggle (press the key again to dismiss)
    match kitty::kitty_ls() {
        Ok(tree) => {
            if let Some(other) = kitty::find_other_matou(&tree, kitty::self_window_id()) {
                kitty::close_window(other);
                return Ok(());
            }
        }
        Err(e) => {
            eprintln!(
                "matou: `kitty @ ls` failed — run inside kitty with remote control enabled\n  \
                 (allow_remote_control + listen_on). detail: {e}"
            );
            std::process::exit(1);
        }
    }
    run_tui()
}

/// `matou web`: jump straight into matouweb (no picker overlay) — seed a shell in the current
/// directory and hand off to the browser daemon. Bound to a kitty key via an overlay launch, so it
/// runs with the kitty socket; the seed window is hidden and the canvas grows from there.
fn launch_web() -> Result<()> {
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    match kitty::new_hidden_oswindow_in(&cwd) {
        Some(w) => {
            mirror::start_detached(w, 9123, true);
            Ok(())
        }
        None => {
            eprintln!("matou web: couldn't open a workspace — run inside kitty with remote control on");
            std::process::exit(1);
        }
    }
}

fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, Hide)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut m = Model::new();
    let (w, h) = crossterm::terminal::size().unwrap_or((100, 30));
    m.w = w;
    m.h = h;

    let (tx, rx) = mpsc::channel::<Msg>();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                match event::read() {
                    Ok(Event::Key(k)) => {
                        if tx.send(Msg::Key(k)).is_err() {
                            break;
                        }
                    }
                    Ok(Event::Resize(w, h)) => {
                        if tx.send(Msg::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
    }

    let res = (|| -> Result<()> {
        loop {
            term.draw(|f| view::render(&m, f))?;
            let Ok(msg) = rx.recv() else { break };
            if !update(&mut m, msg, &tx) {
                break;
            }
        }
        Ok(())
    })();

    execute!(stdout(), Show, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    m.save_cache();
    res
}

/// `-once`: render the palette once into a fixed-size buffer and print it (for demos / previews).
fn render_once() -> Result<()> {
    let env_u16 = |k: &str, d: u16| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (cols, rows) = (env_u16("COLUMNS", 100), env_u16("LINES", 30));
    let m = Model::new();
    let mut term = Terminal::new(TestBackend::new(cols, rows))?;
    term.draw(|f| view::render(&m, f))?;
    print!("{}", buffer_to_ansi(term.backend().buffer()));
    Ok(())
}

fn buffer_to_ansi(buf: &Buffer) -> String {
    let area = buf.area();
    let mut s = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let c = &buf[(x, y)];
            if let Color::Rgb(r, g, b) = c.fg {
                s.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            if let Color::Rgb(r, g, b) = c.bg {
                s.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
            }
            s.push_str(c.symbol());
            s.push_str("\x1b[0m");
        }
        s.push('\n');
    }
    s
}
