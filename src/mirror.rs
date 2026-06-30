//! `chatons mirror --window <id> [--port <p>] [--bind <addr>]` — serve a live, controllable
//! view of a kitty window in the browser, rendered with **xterm.js**. Default bind is
//! `127.0.0.1` (the trust boundary); other binds warn (no auth/TLS yet — that's the remote TODO).
//!
//!   GET  /           a self-contained page hosting an xterm.js terminal
//!   GET  /xterm.js   vendored xterm.js (MIT) — see vendor/xterm.LICENSE
//!   GET  /xterm.css  vendored xterm.css
//!   GET  /size       the source window's {cols,rows}
//!   GET  /stream     SSE; base64 frames of raw ANSI written verbatim into xterm.js
//!   POST /key        bytes from xterm's onData, replayed into the window
//!
//! Speed: we talk to kitty's remote-control **unix socket directly** (one persistent connection,
//! ~0.5ms/query) instead of spawning `kitty @` per frame/keystroke (~10ms fork+exec). The poll is
//! adaptive (≈30fps while the screen changes, backing off when idle) and frames are **row-diffed**
//! (only changed rows are sent). Still a poll — kitty has no screen-change event over `@`.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

// (cert hash as dotted-hex, quic port) — set once at startup if WebTransport comes up, then read
// by page() to bootstrap the browser's serverCertificateHashes. Empty hash ⇒ SSE-only.
static WT_INFO: OnceLock<(String, u16)> = OnceLock::new();

// Set when the session is ending, so connected SSE clients get a clean `event: end` (→ the page
// closes its own tab) before the process exits a moment later.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

// Browser-presence tracking: STREAMS counts live SSE panes and SEEN_CLIENT flips once a browser has
// ever connected — together they let the daemon exit once the browser is gone (no lingering daemon).
static STREAMS: AtomicUsize = AtomicUsize::new(0);
static SEEN_CLIENT: AtomicBool = AtomicBool::new(false);

// Windows the canvas created (drawn cards, + the seed when it's a project's hidden OS window). The
// daemon owns these — it resizes them and closes them on teardown. An open-tab seed isn't in here,
// so it's left untouched.
static OWNED_WINS: Mutex<Vec<i64>> = Mutex::new(Vec::new());
fn own_window(id: i64) {
    let Ok(mut v) = OWNED_WINS.lock() else { return };
    if !v.contains(&id) {
        v.push(id);
    }
}
fn disown_window(id: i64) {
    if let Ok(mut v) = OWNED_WINS.lock() {
        v.retain(|&x| x != id);
    }
}
fn is_owned(id: i64) -> bool {
    OWNED_WINS.lock().map(|v| v.contains(&id)).unwrap_or(false)
}

/// matou's config dir (where the mirror keeps its pidfile + ticket).
pub(crate) fn home() -> PathBuf {
    if let Ok(h) = std::env::var("MATOU_HOME") {
        return PathBuf::from(h);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("matou")
}

fn pidfile() -> PathBuf {
    home().join("mirror.pid")
}

/// Kill whatever is listening on TCP `port` (one daemon owns both its TCP and udp/QUIC sockets, so
/// this clears the whole process). Robust against a clobbered/stale pidfile — returns how many.
pub(crate) fn kill_port(port: u16) -> usize {
    let Ok(out) = Command::new("ss").args(["-ltnpH", &format!("sport = :{port}")]).output() else {
        return 0;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut killed = 0;
    for seg in text.split("pid=").skip(1) {
        let pid: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !pid.is_empty() {
            let _ = Command::new("kill").arg(&pid).status();
            killed += 1;
        }
    }
    killed
}

fn stop(port: u16) -> Result<()> {
    // best-effort pidfile, then anything still bound to the port (the reliable path)
    if let Ok(pid) = std::fs::read_to_string(pidfile()) {
        let _ = Command::new("kill").arg(pid.trim()).status();
    }
    let _ = std::fs::remove_file(pidfile());
    let n = kill_port(port);
    println!("mirror stopped ({n} listener(s) on :{port})");
    Ok(())
}

/// End this mirror session and exit: signal connected pages (`event: end`), close every window the
/// canvas created (the user's own open-tab seed is never owned, so it's left alone), then go.
fn shutdown_now() -> ! {
    SHUTDOWN.store(true, Ordering::Relaxed);
    let owned: Vec<i64> = OWNED_WINS.lock().map(|v| v.clone()).unwrap_or_default();
    for id in owned {
        let _ = Command::new("kitty")
            .args(["@", "close-window", "--match", &format!("id:{id}")])
            .status();
    }
    let _ = std::fs::remove_file(pidfile());
    std::thread::sleep(Duration::from_millis(300));
    std::process::exit(0);
}

/// Create a shell in a fresh hidden OS window, sized to `cols`×`rows` cells, and return its id.
/// The canvas's "draw a rectangle → terminal". Owned, so it's torn down with the session.
fn spawn_window(cols: u16, rows: u16, cwd: &str) -> Option<i64> {
    let mut a = vec!["@".to_string(), "launch".into(), "--type=os-window".into(), "--keep-focus".into()];
    if !cwd.is_empty() {
        a.push("--cwd".into());
        a.push(cwd.to_string());
    }
    let out = Command::new("kitty").args(&a).output().ok()?;
    let id: i64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    let _ = Command::new("kitty")
        .args(["@", "resize-os-window", "--action", "hide", "--match", &format!("id:{id}")])
        .status();
    resize_window(id, cols, rows);
    own_window(id);
    Some(id)
}

/// Resize an owned window's OS window to `cols`×`rows` cells (clamped to a sane minimum), so its
/// grid follows the card and text stays crisp instead of being scaled.
fn resize_window(id: i64, cols: u16, rows: u16) {
    let c = cols.clamp(8, 500).to_string();
    let r = rows.clamp(3, 300).to_string();
    let _ = Command::new("kitty")
        .args(["@", "resize-os-window", "--action", "resize", "--unit", "cells", "--width", &c, "--height", &r, "--match", &format!("id:{id}")])
        .status();
}

/// A short human name for window `id` — the running command if it isn't the shell, else the cwd
/// basename. Shown on the card's titlebar and its collapsed chip.
fn window_name(id: &str) -> String {
    let Ok(out) = Command::new("kitty").args(["@", "ls"]).output() else { return "term".into() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return "term".into() };
    let shellish = |s: &str| matches!(s, "zsh" | "bash" | "fish" | "sh" | "-zsh" | "-bash" | "-fish");
    for ow in v.as_array().into_iter().flatten() {
        for tab in ow["tabs"].as_array().into_iter().flatten() {
            for w in tab["windows"].as_array().into_iter().flatten() {
                if w["id"].as_u64().map(|x| x.to_string()).as_deref() != Some(id) {
                    continue;
                }
                let fg = w["foreground_processes"]
                    .as_array()
                    .and_then(|a| a.last())
                    .and_then(|p| p["cmdline"].as_array())
                    .and_then(|c| c.first())
                    .and_then(|s| s.as_str())
                    .map(|s| s.rsplit('/').next().unwrap_or(s).to_string());
                if let Some(c) = fg.filter(|c| !c.is_empty() && !shellish(c)) {
                    return c;
                }
                let base = w["cwd"].as_str().unwrap_or("").rsplit('/').next().unwrap_or("");
                return if base.is_empty() { "term".into() } else { base.to_string() };
            }
        }
    }
    "term".into()
}

pub fn run(args: &[String]) -> Result<()> {
    let mut window: Option<String> = None;
    let mut port: u16 = 9123;
    let mut bind = "127.0.0.1".to_string();
    let mut do_stop = false;
    let mut p2p = false;
    let mut owned_seed = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stop" => do_stop = true,
            "--p2p" => p2p = true,
            "--owned-seed" => owned_seed = true,
            "--window" | "-w" => {
                window = args.get(i + 1).cloned();
                i += 1;
            }
            "--port" | "-p" => {
                if let Some(p) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    port = p;
                }
                i += 1;
            }
            "--bind" | "-b" => {
                if let Some(b) = args.get(i + 1) {
                    bind = b.clone();
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if do_stop {
        return stop(port);
    }
    let window = window.context("usage: chatons mirror --window <id> [--port <p>] [--bind <addr>]")?;
    let matchspec = format!("id:{window}");
    // a project seed is a hidden OS window we made → own it so teardown closes it; an open-tab seed
    // is the user's window → leave it out so it survives.
    if let (true, Ok(id)) = (owned_seed, window.parse::<i64>()) {
        own_window(id);
    }

    // bind, retrying briefly so a just-killed predecessor has time to release the port
    let mut listener = None;
    for _ in 0..15 {
        match TcpListener::bind((bind.as_str(), port)) {
            Ok(l) => {
                listener = Some(l);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let listener = listener.with_context(|| format!("binding {bind}:{port}"))?;
    if bind != "127.0.0.1" && bind != "localhost" && bind != "::1" {
        eprintln!(
            "WARNING: bound to {bind} with no auth — anyone who can reach this port gets a live \
             shell. localhost-only is the supported mode until auth/TLS lands."
        );
    }
    let _ = std::fs::create_dir_all(home());
    let _ = std::fs::write(pidfile(), std::process::id().to_string());
    println!("matou mirror → http://{bind}:{port}/  (window {window})");

    // WebTransport (HTTP/3 over QUIC) fast path on udp/<port+1> — additive; the browser pins the
    // self-signed cert via the hash injected into the page, and falls back to SSE if it can't.
    let quic_port = port.wrapping_add(1);
    match wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1", bind.as_str()]) {
        Ok(identity) => {
            let hash = identity.certificate_chain().as_slice()[0]
                .hash()
                .fmt(wtransport::tls::Sha256DigestFmt::DottedHex);
            let _ = WT_INFO.set((hash, quic_port));
            let (b, ms) = (bind.clone(), matchspec.clone());
            std::thread::spawn(move || crate::mirror_wt::serve(quic_port, b, identity, ms));
            println!("  WebTransport on udp/{quic_port}");
        }
        Err(e) => eprintln!("  WebTransport disabled (cert: {e}); SSE only"),
    }

    // P2P (iroh) — opt-in via --p2p; dial-from-anywhere over QUIC + NAT traversal + relay
    if p2p {
        let ms = matchspec.clone();
        std::thread::spawn(move || crate::mirror_p2p::serve(ms));
    }

    // watchdog: the daemon is detached, so it must exit on its own once the browser is gone — the
    // canvas's windows span many OS windows, so the lifeline is the browser connection, not any one
    // window. Exit when every pane's SSE has dropped (after a browser ever connected), or if no
    // browser shows up at all (e.g. the open failed). `shutdown_now` then closes the owned windows.
    {
        std::thread::spawn(move || {
            let mut gone = 0;
            let mut never = 0;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if SEEN_CLIENT.load(Ordering::Relaxed) {
                    if STREAMS.load(Ordering::Relaxed) == 0 {
                        gone += 1;
                        if gone >= 3 {
                            // ~6s with no panes — longer than EventSource's reconnect, so a
                            // transient drop won't trip it. /bye (sendBeacon) is the fast path.
                            shutdown_now();
                        }
                    } else {
                        gone = 0;
                    }
                } else {
                    never += 1;
                    if never >= 15 {
                        shutdown_now(); // ~30s and no browser ever connected → give up
                    }
                }
            }
        });
    }

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let m = matchspec.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &m);
        });
    }
    Ok(())
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn handle(mut stream: TcpStream, seed: &str) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if t.len() >= 15 && t[..15].eq_ignore_ascii_case("content-length:") {
            content_length = t[15..].trim().parse().unwrap_or(0);
        }
    }

    // Every per-pane endpoint takes `?w=<id>`; absent it, default to the seed window the daemon
    // launched with. Each browser pane is a real kitty window addressed this way.
    let seed_id = seed.strip_prefix("id:").unwrap_or(seed);
    let win = query_param(query, "w");
    let matchspec = win.as_deref().map(|w| format!("id:{w}")).unwrap_or_else(|| seed.to_string());

    match (method, path) {
        ("GET", "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", page(seed_id).as_bytes())?,
        ("GET", "/xterm.js") => respond(
            &mut stream,
            "200 OK",
            "application/javascript; charset=utf-8",
            include_str!("vendor/xterm.js").as_bytes(),
        )?,
        ("GET", "/xterm.css") => respond(
            &mut stream,
            "200 OK",
            "text/css; charset=utf-8",
            include_str!("vendor/xterm.css").as_bytes(),
        )?,
        ("GET", "/size") => {
            let (c, r) = window_size(&matchspec);
            let body = json!({"cols": c, "rows": r, "name": window_name(matchspec.strip_prefix("id:").unwrap_or(&matchspec))});
            respond(&mut stream, "200 OK", "application/json", body.to_string().as_bytes())?;
        }
        ("GET", "/stream") => stream_loop(&mut stream, &matchspec)?,
        ("GET", "/heartbeat") => heartbeat_loop(&mut stream)?,
        ("POST", "/key") => {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body)?;
            send_input(&matchspec, &body);
            write!(stream, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        }
        ("POST", "/spawn") => {
            // draw a rectangle → a fresh terminal sized to it, in the seed's directory
            let cols = query_param(query, "cols").and_then(|s| s.parse().ok()).unwrap_or(80);
            let rows = query_param(query, "rows").and_then(|s| s.parse().ok()).unwrap_or(24);
            let cwd = window_cwd(seed_id).unwrap_or_default();
            let body = match spawn_window(cols, rows, &cwd) {
                Some(id) => json!({"w": id, "name": window_name(&id.to_string())}).to_string(),
                None => "{\"w\":null}".to_string(),
            };
            respond(&mut stream, "200 OK", "application/json", body.as_bytes())?;
        }
        ("POST", "/resize") => {
            // a card was resized → match its window's grid (owned windows only; never the user's seed)
            if let Some(w) = win.as_deref().and_then(|s| s.parse::<i64>().ok()).filter(|&w| is_owned(w)) {
                let cols = query_param(query, "cols").and_then(|s| s.parse().ok()).unwrap_or(80);
                let rows = query_param(query, "rows").and_then(|s| s.parse().ok()).unwrap_or(24);
                resize_window(w, cols, rows);
            }
            write!(stream, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        }
        ("POST", "/close") => {
            if let Some(w) = &win {
                let _ = Command::new("kitty")
                    .args(["@", "close-window", "--match", &format!("id:{w}")])
                    .status();
                if let Ok(id) = w.parse::<i64>() {
                    disown_window(id);
                }
            }
            write!(stream, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        }
        ("POST", "/bye") => {
            // sent by the page on tab close (sendBeacon) → end the session (owned workspaces also
            // get their hidden windows closed). Never returns.
            write!(stream, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
            let _ = stream.flush();
            shutdown_now();
        }
        _ => write!(stream, "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")?,
    }
    Ok(())
}

/// First `key=value` for `key` in a `&`-joined query string.
fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// Window `id`'s cwd from `kitty @ ls`, so a drawn terminal opens in the seed's directory.
fn window_cwd(id: &str) -> Option<String> {
    let out = Command::new("kitty").args(["@", "ls"]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for ow in v.as_array().into_iter().flatten() {
        for tab in ow["tabs"].as_array().into_iter().flatten() {
            for w in tab["windows"].as_array().into_iter().flatten() {
                if w["id"].as_u64().map(|x| x.to_string()).as_deref() == Some(id) {
                    return w["cwd"].as_str().map(String::from);
                }
            }
        }
    }
    None
}

/// SSE: poll the screen over a persistent kitty socket, send only changed rows, poll fast while
/// active and back off when idle.
/// Produces successive screen frames from the kitty socket — polls, SGR-normalises, row-diffs, and
/// tracks idle for adaptive pacing. Shared by the SSE (`stream_loop`) and WebTransport transports
/// so "what's on screen" has one implementation and two wire formats.
pub(crate) struct FrameSource {
    conn: Option<KittyConn>,
    prev: Vec<Vec<u8>>,
    first: bool,
    idle: u32,
    empty_streak: u32, // consecutive empty reads ⇒ the window likely went away
}

impl FrameSource {
    pub(crate) fn new() -> Self {
        Self { conn: None, prev: Vec::new(), first: true, idle: 0, empty_streak: 0 }
    }

    /// Poll once: the bytes to write to the client terminal if the screen changed (a full repaint
    /// on the first call / after a reconnect, a row-diff otherwise), else `None`.
    pub(crate) fn poll(&mut self, matchspec: &str) -> Option<Vec<u8>> {
        let body = sgr_to_legacy(&get_screen(&mut self.conn, matchspec));
        if body.is_empty() {
            self.empty_streak = self.empty_streak.saturating_add(1);
            return None;
        }
        self.empty_streak = 0;
        let cur: Vec<Vec<u8>> = body.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
        if self.first || cur != self.prev {
            let payload = frame_diff(&self.prev, &cur, self.first);
            self.prev = cur;
            self.first = false;
            self.idle = 0;
            Some(payload)
        } else {
            self.idle = self.idle.saturating_add(1);
            None
        }
    }

    /// Adaptive poll delay: ~30fps while active, easing to ~5fps when idle.
    pub(crate) fn delay_ms(&self) -> u64 {
        if self.idle < 15 { 33 } else { 200 }
    }
}

/// Decrements the live-stream count on drop, so the disconnect watchdog sees the browser leave no
/// matter how the stream loop returns.
struct StreamGuard;
impl Drop for StreamGuard {
    fn drop(&mut self) {
        STREAMS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// One SSE connection per open page, counted like a pane so the daemon stays alive while the page is
/// open even with zero terminals — and exits once the page (and so this stream) is gone. Emits the
/// same `event: end` on shutdown so the page can close itself.
fn heartbeat_loop(stream: &mut TcpStream) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    STREAMS.fetch_add(1, Ordering::Relaxed);
    SEEN_CLIENT.store(true, Ordering::Relaxed);
    let _guard = StreamGuard;
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            write!(stream, "event: end\r\ndata: bye\r\n\r\n")?;
            stream.flush()?;
            return Ok(());
        }
        stream.write_all(b": beat\r\n\r\n")?;
        stream.flush()?;
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn stream_loop(stream: &mut TcpStream, matchspec: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    STREAMS.fetch_add(1, Ordering::Relaxed);
    SEEN_CLIENT.store(true, Ordering::Relaxed);
    let _guard = StreamGuard;
    let mut src = FrameSource::new();
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            // session ending → tell the page to close its own tab, then drop the connection
            write!(stream, "event: end\r\ndata: bye\r\n\r\n")?;
            stream.flush()?;
            return Ok(());
        }
        match src.poll(matchspec) {
            Some(payload) => {
                write!(stream, "data: {}\r\n\r\n", STANDARD.encode(&payload))?;
                stream.flush()?;
            }
            None => {
                // empty for a while → this pane's window may have been closed in the terminal;
                // confirm it's truly gone and tell the page to drop just this pane (not the session)
                if src.empty_streak >= 3 {
                    let id = matchspec.strip_prefix("id:").unwrap_or("");
                    if !id.is_empty() && window_exists(id) == Some(false) {
                        write!(stream, "event: gone\r\ndata: {id}\r\n\r\n")?;
                        stream.flush()?;
                        return Ok(());
                    }
                }
                stream.write_all(b": ping\r\n\r\n")?;
                stream.flush()?;
            }
        }
        std::thread::sleep(Duration::from_millis(src.delay_ms()));
    }
}

/// One xterm write: on the first frame, disable wrap + clear + paint every row; after that, only
/// the rows that changed (each cleared first), plus blanking any rows that vanished on a resize.
/// Absolute `CSI row;1H` positioning means a width disagreement can't cascade; the inline cursor
/// escape (from `--add-cursor`) travels with the last content chunk, so it repositions for free.
fn frame_diff(prev: &[Vec<u8>], cur: &[Vec<u8>], first: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    if first {
        out.extend_from_slice(b"\x1b[?7l\x1b[m\x1b[2J"); // wrap off, reset, clear
    }
    for (i, row) in cur.iter().enumerate() {
        if first || prev.get(i) != Some(row) {
            out.extend_from_slice(format!("\x1b[{};1H\x1b[m\x1b[2K", i + 1).as_bytes());
            out.extend_from_slice(row);
        }
    }
    for i in cur.len()..prev.len() {
        out.extend_from_slice(format!("\x1b[{};1H\x1b[m\x1b[2K", i + 1).as_bytes());
    }
    out
}

/// kitty emits truecolor SGR in the colon sub-parameter form `38:2:R:G:B` (no colour-space
/// field). xterm.js follows the ISO form `38:2:<cs>:R:G:B`, so it reads the 5-part form
/// off-by-one — channels shift and blue drops, casting everything green. Normalise SGR colons to
/// the legacy `38;2;R;G;B` semicolon form, which every parser reads correctly. Only rewrites bytes
/// inside CSI `…m` (SGR) sequences — never text content (which can contain colons).
fn sgr_to_legacy(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b && data.get(i + 1) == Some(&b'[') {
            let mut j = i + 2;
            while j < data.len() && !(0x40..=0x7e).contains(&data[j]) {
                j += 1;
            }
            if j >= data.len() {
                out.extend_from_slice(&data[i..]); // unterminated CSI
                break;
            }
            if data[j] == b'm' {
                out.extend_from_slice(b"\x1b[");
                out.extend(data[i + 2..j].iter().map(|&b| if b == b':' { b';' } else { b }));
                out.push(b'm');
            } else {
                out.extend_from_slice(&data[i..=j]); // other CSI verbatim
            }
            i = j + 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Get the screen via the persistent socket; (re)connect lazily, fall back to spawning `kitty @`
/// if the socket is unavailable.
fn get_screen(conn: &mut Option<KittyConn>, matchspec: &str) -> Vec<u8> {
    if conn.is_none() {
        *conn = KittyConn::connect();
    }
    if let Some(c) = conn.as_mut() {
        if let Some(b) = c.get_text(matchspec) {
            return b;
        }
        *conn = None; // socket went bad → drop, reconnect next tick
    }
    capture_spawn(matchspec)
}

pub(crate) fn send_input(matchspec: &str, bytes: &[u8]) {
    if let Some(mut c) = KittyConn::connect() {
        if c.send_text(matchspec, bytes).is_some() {
            return;
        }
    }
    send_keys_spawn(matchspec, bytes);
}

// ── kitty remote-control over the unix socket (no fork+exec) ──────────────────────────────────

/// A persistent connection to kitty's remote-control socket (`$KITTY_LISTEN_ON`). Speaks the
/// DCS-framed JSON protocol: `ESC P @kitty-cmd {json} ESC \`.
struct KittyConn {
    sock: UnixStream,
}

impl KittyConn {
    fn connect() -> Option<KittyConn> {
        let path = std::env::var("KITTY_LISTEN_ON").ok()?.strip_prefix("unix:")?.to_string();
        let sock = UnixStream::connect(path).ok()?;
        sock.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        Some(KittyConn { sock })
    }

    fn cmd(&mut self, name: &str, payload: serde_json::Value) -> Option<serde_json::Value> {
        let msg = json!({"cmd": name, "version": [0, 14, 2], "payload": payload});
        self.sock.write_all(b"\x1bP@kitty-cmd").ok()?;
        self.sock.write_all(msg.to_string().as_bytes()).ok()?;
        self.sock.write_all(b"\x1b\\").ok()?;
        self.sock.flush().ok()?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = self.sock.read(&mut chunk).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.ends_with(b"\x1b\\") {
                break;
            }
        }
        let start = buf.windows(10).position(|w| w == b"@kitty-cmd")? + 10;
        let end = buf.len().checked_sub(2)?; // strip trailing ESC \
        (start <= end).then(|| serde_json::from_slice(&buf[start..end]).ok()).flatten()
    }

    fn get_text(&mut self, matchspec: &str) -> Option<Vec<u8>> {
        let r = self.cmd(
            "get-text",
            json!({"match": matchspec, "extent": "screen", "ansi": true, "add_cursor": true}),
        )?;
        if r.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
            r.get("data").and_then(serde_json::Value::as_str).map(|s| s.as_bytes().to_vec())
        } else {
            None
        }
    }

    fn send_text(&mut self, matchspec: &str, bytes: &[u8]) -> Option<()> {
        // send-text wants its data encoding-tagged; base64 keeps control bytes intact
        let data = format!("base64:{}", STANDARD.encode(bytes));
        let r = self.cmd("send-text", json!({"match": matchspec, "data": data}))?;
        (r.get("ok").and_then(serde_json::Value::as_bool) == Some(true)).then_some(())
    }
}

// ── spawn-based fallbacks (used only if the socket is unavailable) ─────────────────────────────

fn capture_spawn(matchspec: &str) -> Vec<u8> {
    Command::new("kitty")
        .args(["@", "get-text", "--match", matchspec, "--extent", "screen", "--ansi", "--add-cursor"])
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default()
}

fn send_keys_spawn(matchspec: &str, bytes: &[u8]) {
    if let Ok(mut child) = Command::new("kitty")
        .args(["@", "send-text", "--match", matchspec, "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(bytes);
        }
        let _ = child.wait();
    }
}

/// Whether window `id` still exists: Some(true/false) if `kitty @ ls` succeeded, None on a
/// transient failure (so the per-pane `gone` check won't fire on a hiccup).
fn window_exists(id: &str) -> Option<bool> {
    let out = Command::new("kitty").args(["@", "ls"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let found = v.as_array().into_iter().flatten().any(|ow| {
        ow["tabs"].as_array().into_iter().flatten().any(|t| {
            t["windows"].as_array().into_iter().flatten().any(|w| {
                w["id"].as_u64().map(|x| x.to_string()).as_deref() == Some(id)
            })
        })
    });
    Some(found)
}

/// The source window's grid size (so xterm matches it). Page-load only, so a spawn is fine.
fn window_size(matchspec: &str) -> (u16, u16) {
    let id = matchspec.strip_prefix("id:").unwrap_or("");
    let out = Command::new("kitty").args(["@", "ls"]).output().map(|o| o.stdout).unwrap_or_default();
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out) {
        for ow in v.as_array().into_iter().flatten() {
            for tab in ow["tabs"].as_array().into_iter().flatten() {
                for w in tab["windows"].as_array().into_iter().flatten() {
                    if w["id"].as_u64().map(|x| x.to_string()).as_deref() == Some(id) {
                        let c = w["columns"].as_u64().unwrap_or(80) as u16;
                        let r = w["lines"].as_u64().unwrap_or(24) as u16;
                        return (c.max(1), r.max(1));
                    }
                }
            }
        }
    }
    (80, 24)
}

/// kitty's full colour set from `get-colors` (background/foreground + the 16 ANSI palette
/// entries + cursor/selection). Page-load only.
fn kitty_colors() -> std::collections::HashMap<String, String> {
    let out = Command::new("kitty").args(["@", "get-colors"]).output().map(|o| o.stdout).unwrap_or_default();
    let mut m = std::collections::HashMap::new();
    for l in String::from_utf8_lossy(&out).lines() {
        let mut it = l.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if v.starts_with('#') {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    m
}

/// xterm.js `theme` object built from kitty's colours, so indexed/ANSI colours (what fzf and a
/// 256-colour nvim use) match your palette instead of xterm's saturated built-in defaults.
/// Indices 16–255 are the standard xterm cube in both, so only 0–15 need mapping.
fn theme_json(c: &std::collections::HashMap<String, String>) -> String {
    let names = [
        ("black", "color0"), ("red", "color1"), ("green", "color2"), ("yellow", "color3"),
        ("blue", "color4"), ("magenta", "color5"), ("cyan", "color6"), ("white", "color7"),
        ("brightBlack", "color8"), ("brightRed", "color9"), ("brightGreen", "color10"),
        ("brightYellow", "color11"), ("brightBlue", "color12"), ("brightMagenta", "color13"),
        ("brightCyan", "color14"), ("brightWhite", "color15"),
    ];
    let mut obj = serde_json::Map::new();
    let mut put = |xkey: &str, val: Option<&String>| {
        if let Some(v) = val {
            obj.insert(xkey.to_string(), json!(v));
        }
    };
    put("background", c.get("background"));
    put("foreground", c.get("foreground"));
    put("cursor", c.get("cursor"));
    put("selectionBackground", c.get("selection_background"));
    for (xname, kname) in names {
        put(xname, c.get(kname));
    }
    serde_json::Value::Object(obj).to_string()
}

/// The terminal font, read from kitty.conf. kitty clamps wide nerd/powerline glyphs to one cell
/// itself; the browser won't, so prefer the font's **Mono** variant (glyphs pre-clamped to a
/// single cell) to keep columns aligned.
fn font_family() -> String {
    let path = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/kitty/kitty.conf"))
        .unwrap_or_default();
    let configured = std::fs::read_to_string(path).ok().and_then(|c| {
        c.lines().rev().find_map(|l| {
            l.trim()
                .strip_prefix("font_family")
                .map(|r| r.trim().to_string())
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
        })
    });
    match configured {
        Some(f) => {
            let mut fams = Vec::new();
            if f.contains("Nerd Font") && !f.contains("Mono") && !f.contains("Propo") {
                fams.push(format!("'{f} Mono'"));
            }
            fams.push(format!("'{f}'"));
            fams.push("'Symbols Nerd Font Mono'".into());
            fams.push("monospace".into());
            fams.join(", ")
        }
        None => "'Symbols Nerd Font Mono', monospace".to_string(),
    }
}

fn page(seed: &str) -> String {
    let c = kitty_colors();
    let bg = c.get("background").cloned().unwrap_or_else(|| "#1e1e1e".into());
    // an owned seed (a hidden OS window we made) may be resized to its card; the user's own open-tab
    // seed must not be, so the page just scales it to fit.
    let owned = seed.parse::<i64>().map(is_owned).unwrap_or(false);
    PAGE.replace("__FONT__", &font_family())
        .replace("__THEME__", &theme_json(&c))
        .replace("__BG__", &bg)
        .replace("__SEED__", seed)
        .replace("__SEED_OWNED__", if owned { "true" } else { "false" })
}

// kittyweb: a spatial canvas. Draw a rectangle on empty space → a fresh terminal (a hidden kitty OS
// window sized to the rect). Drag the titlebar to move, the corner to resize, collapse to a name
// chip, close to dismiss. Each card is a real kitty window — xterm.js fed by SSE, input POSTed back;
// a /heartbeat stream keeps the daemon alive while the page is open, even with zero cards.
const PAGE: &str = r##"<!doctype html>
<html><head><meta charset=utf-8><title>kittyweb</title>
<link rel=stylesheet href=/xterm.css>
<style>
 :root{--mantle:#181825;--s0:#313244;--ov0:#6c7086;--ov1:#7f849c;--text:#cdd6f4;--sub:#a6adc8;
   --blue:#8aadf4;--red:#f38ba8;--green:#a6e3a1}
 html,body{margin:0;height:100%;overflow:hidden;font-family:ui-sans-serif,system-ui,sans-serif;color:var(--text)}
 .canvas{position:fixed;inset:0;background:#11111b;cursor:crosshair;
   background-image:radial-gradient(#262638 1.1px,transparent 1.1px);background-size:23px 23px;background-position:-1px -1px}
 .hud{position:fixed;left:50%;top:12px;transform:translateX(-50%);z-index:1000;pointer-events:none}
 .hud .pill{background:#181825e6;border:1px solid #2a2a40;border-radius:999px;padding:7px 14px;
   font:12.5px ui-monospace,monospace;color:var(--sub)}
 .hud .pill b{color:var(--blue)}
 #bar{position:fixed;right:12px;bottom:10px;z-index:1000;font:11px ui-monospace,monospace;
   color:var(--ov0);background:#181825aa;padding:3px 8px;border-radius:6px;pointer-events:none}
 .rubber{position:absolute;z-index:900;border:1.5px dashed var(--blue);background:#8aadf41a;
   border-radius:8px;pointer-events:none}
 .card{position:absolute;display:flex;flex-direction:column;background:__BG__;border-radius:10px;
   overflow:hidden;box-shadow:inset 0 0 0 1px #2c2c40,0 14px 36px -18px #000d;transition:box-shadow .14s}
 .card.act{box-shadow:inset 0 0 0 1.5px var(--blue),0 20px 48px -20px #000e}
 .card.spawn{animation:pop .16s ease}
 @keyframes pop{from{transform:scale(.96);opacity:.5}to{transform:scale(1);opacity:1}}
 .tbar{display:flex;align-items:center;gap:8px;height:28px;padding:0 6px 0 10px;flex:none;
   background:var(--mantle);cursor:grab;user-select:none;border-bottom:1px solid #0003}
 .tbar:active{cursor:grabbing}
 .dot{width:8px;height:8px;border-radius:50%;background:var(--green);flex:none;box-shadow:0 0 6px #a6e3a166}
 .name{font:12px ui-monospace,monospace;color:var(--sub);white-space:nowrap;overflow:hidden;
   text-overflow:ellipsis;flex:1;min-width:0}
 .tb{display:flex;gap:2px;flex:none}
 .tb button{width:20px;height:20px;border:0;background:transparent;color:var(--ov1);border-radius:5px;
   cursor:pointer;font:600 13px/1 ui-sans-serif,sans-serif;display:flex;align-items:center;
   justify-content:center;transition:background .12s,color .12s}
 .tb button:hover{background:var(--s0);color:var(--text)}
 .tb button.x:hover{background:var(--red);color:#11111b}
 .body{position:relative;flex:1;min-height:0;overflow:hidden}
 .host{position:absolute;top:0;left:0;transform-origin:0 0}
 .xterm,.xterm-viewport{background:__BG__ !important}
 .grip{position:absolute;right:0;bottom:0;width:16px;height:16px;cursor:nwse-resize;z-index:5}
 .grip::after{content:"";position:absolute;right:3px;bottom:3px;width:7px;height:7px;
   border-right:2px solid var(--ov1);border-bottom:2px solid var(--ov1)}
 .card.act .grip::after{border-color:var(--blue)}
 .card.col{width:auto!important;height:auto!important;cursor:default}
 .card.col .body,.card.col .grip{display:none}
 .card.col .tbar{border-bottom:0;border-radius:10px}
 .card.col .name{max-width:220px}
 @media (prefers-reduced-motion:reduce){*{transition:none!important;animation:none!important}}
</style></head>
<body>
<div class="canvas" id="canvas"></div>
<div class="hud"><div class="pill"><b>drag</b> empty space → new terminal · titlebar to move · corner to resize</div></div>
<div id=bar>kittyweb</div>
<script src=/xterm.js></script>
<script>
const THEME=__THEME__,FONT="__FONT__",SEED="__SEED__",SEED_OWNED=__SEED_OWNED__;
const CW=7.8,CH=17; // approx cell px at 13px font, for px→grid; the daemon returns the real grid
const canvas=document.getElementById('canvas'),bar=document.getElementById('bar');
const cards=new Set();
let z=10,active=null,ended=false,hb=null;

function endSession(){
  if(ended)return;ended=true;
  try{hb&&hb.close()}catch(_){}
  window.close();
  setTimeout(()=>{document.title='kittyweb — ended';
    document.body.innerHTML='<div style="position:fixed;inset:0;display:flex;align-items:center;'+
      'justify-content:center;color:#888;font:14px ui-monospace,monospace">session ended — you can close this tab</div>';},120);
}
function updateBar(){if(!ended)bar.textContent='kittyweb · '+cards.size+' terminal'+(cards.size===1?'':'s');}
function gridOf(w,h){return{cols:Math.max(8,Math.round(w/CW)),rows:Math.max(3,Math.round(h/CH))};}

// scale a card's terminal grid to fill its body box (≈1 since the window is resized to match)
function fit(card){const b=card.body,h=card.host,nw=h.offsetWidth,nh=h.offsetHeight;
  if(!nw||!nh||!b.clientWidth)return;
  h.style.transform='scale('+Math.min(b.clientWidth/nw,b.clientHeight/nh)+')';}
function refit(card){fetch('/size?w='+card.win).then(r=>r.json()).then(s=>{
  if(s&&s.cols&&s.rows)card.term.resize(s.cols,s.rows);
  if(s&&s.name)card.nameEl.textContent=s.name;fit(card);}).catch(()=>fit(card));}
// owned cards re-grid their kitty window to match the body (crisp text); the user's open-tab seed
// isn't owned, so we just scale its existing grid to fit.
function syncSize(card){
  if(!card.owned){fit(card);return;}
  const g=gridOf(card.body.clientWidth,card.body.clientHeight);
  fetch('/resize?w='+card.win+'&cols='+g.cols+'&rows='+g.rows,{method:'POST'}).then(()=>refit(card)).catch(()=>fit(card));
}

function openStream(card){
  const es=new EventSource('/stream?w='+card.win);card.es=es;
  es.onmessage=e=>{card.term.write(Uint8Array.from(atob(e.data),c=>c.charCodeAt(0)));fit(card);};
  es.addEventListener('end',endSession);
  es.addEventListener('gone',()=>dropCard(card,false)); // window closed in the terminal → drop card
}

function tb(txt,title,fn,cls){const b=document.createElement('button');b.textContent=txt;b.title=title;
  if(cls)b.className=cls;b.onclick=e=>{e.stopPropagation();fn();};return b;}

function makeCard(win,x,y,w,h,name,owned){
  const card={win,owned:!!owned,x,y,w,h,collapsed:false};
  const el=document.createElement('div');el.className='card spawn';
  el.style.cssText='left:'+x+'px;top:'+y+'px;width:'+w+'px;height:'+h+'px;z-index:'+(++z);
  const tbar=document.createElement('div');tbar.className='tbar';
  const dot=document.createElement('span');dot.className='dot';
  const nm=document.createElement('span');nm.className='name';nm.textContent=name||'term';
  const tbs=document.createElement('span');tbs.className='tb';
  const minb=tb('─','collapse',()=>toggleCollapse(card));
  tbs.append(minb,tb('×','close',()=>closeCard(card),'x'));
  tbar.append(dot,nm,tbs);
  const body=document.createElement('div');body.className='body';
  const host=document.createElement('div');host.className='host';body.appendChild(host);
  const grip=document.createElement('div');grip.className='grip';
  el.append(tbar,body,grip);canvas.appendChild(el);
  Object.assign(card,{el,body,host,nameEl:nm,collapseBtn:minb});
  const term=new Terminal({fontFamily:FONT,fontSize:13,cursorBlink:false,scrollback:0,theme:THEME});
  term.open(host);card.term=term;
  term.onData(d=>fetch('/key?w='+win,{method:'POST',body:d}));
  el.addEventListener('mousedown',e=>{e.stopPropagation();setActive(card);}); // don't start a draw
  tbar.addEventListener('mousedown',e=>{if(e.target.tagName==='BUTTON')return;e.stopPropagation();startDrag('move',card,e);});
  grip.addEventListener('mousedown',e=>{e.stopPropagation();startDrag('resize',card,e);});
  new ResizeObserver(()=>fit(card)).observe(body);
  cards.add(card);setActive(card);updateBar();
  setTimeout(()=>el.classList.remove('spawn'),180);
  if(owned)syncSize(card); else refit(card); // owned: grid to the card; seed: adopt its real grid
  openStream(card);
  return card;
}

function setActive(card){if(active)active.el.classList.remove('act');
  active=card;if(card){card.el.classList.add('act');card.el.style.zIndex=++z;card.term.focus();}}

function toggleCollapse(card){
  if(card.collapsed){
    card.collapsed=false;card.el.classList.remove('col');
    card.el.style.width=card.w+'px';card.el.style.height=card.h+'px';
    card.collapseBtn.textContent='─';openStream(card);refit(card);
  }else{
    card.w=card.el.offsetWidth;card.h=card.el.offsetHeight;
    card.collapsed=true;card.el.classList.add('col');card.collapseBtn.textContent='□';
    if(card.es){card.es.close();card.es=null;} // pause polling while it's just a chip
  }
}

function dropCard(card,tellKitty){
  if(!cards.has(card))return;
  if(tellKitty)fetch('/close?w='+card.win,{method:'POST'}).catch(()=>{});
  if(card.es)card.es.close();
  card.el.remove();cards.delete(card);if(active===card)active=null;
  updateBar(); // closing the last card just leaves an empty canvas — the heartbeat keeps us alive
}
function closeCard(card){dropCard(card,true);}

// ── drag: move / resize ──
let drag=null;
function startDrag(mode,card,e){
  setActive(card);if(card.collapsed&&mode==='resize')return;
  drag={mode,card,sx:e.clientX,sy:e.clientY,ox:card.el.offsetLeft,oy:card.el.offsetTop,
    ow:card.el.offsetWidth,oh:card.el.offsetHeight};
  document.body.style.cursor=mode==='resize'?'nwse-resize':'grabbing';
}
// ── draw a rectangle on empty canvas → spawn ──
let draw=null,rubber=null;
canvas.addEventListener('mousedown',e=>{if(e.button!==0)return;
  draw={sx:e.clientX,sy:e.clientY};
  rubber=document.createElement('div');rubber.className='rubber';
  rubber.style.cssText='left:'+e.clientX+'px;top:'+e.clientY+'px;width:0;height:0';
  canvas.appendChild(rubber);});
window.addEventListener('mousemove',e=>{
  if(drag){const dx=e.clientX-drag.sx,dy=e.clientY-drag.sy,c=drag.card;
    if(drag.mode==='move'){c.el.style.left=Math.max(0,drag.ox+dx)+'px';c.el.style.top=Math.max(0,drag.oy+dy)+'px';
      c.x=c.el.offsetLeft;c.y=c.el.offsetTop;}
    else{c.el.style.width=Math.max(160,drag.ow+dx)+'px';c.el.style.height=Math.max(80,drag.oh+dy)+'px';
      c.w=c.el.offsetWidth;c.h=c.el.offsetHeight;}
  }else if(draw){const x=Math.min(e.clientX,draw.sx),y=Math.min(e.clientY,draw.sy),
    w=Math.abs(e.clientX-draw.sx),h=Math.abs(e.clientY-draw.sy);
    rubber.style.cssText='left:'+x+'px;top:'+y+'px;width:'+w+'px;height:'+h+'px';}
});
window.addEventListener('mouseup',e=>{
  document.body.style.cursor='';
  if(drag){const d=drag;drag=null;if(d.mode==='resize')syncSize(d.card);return;}
  if(draw){const x=Math.min(e.clientX,draw.sx),y=Math.min(e.clientY,draw.sy),
    w=Math.abs(e.clientX-draw.sx),h=Math.abs(e.clientY-draw.sy);
    rubber.remove();rubber=null;draw=null;
    if(w>60&&h>50){const W=Math.max(200,w),H=Math.max(110,h),g=gridOf(W,H-28);
      fetch('/spawn?cols='+g.cols+'&rows='+g.rows,{method:'POST'}).then(r=>r.json()).then(o=>{
        if(o&&o.w)makeCard(o.w,x,y,W,H,o.name,true);else bar.textContent='kittyweb · spawn failed';
      }).catch(()=>{bar.textContent='kittyweb · spawn failed';});}
  }
});

// heartbeat keeps the daemon alive while this page is open (independent of how many cards exist)
hb=new EventSource('/heartbeat');hb.addEventListener('end',endSession);
addEventListener('pagehide',()=>{try{navigator.sendBeacon('/bye')}catch(_){}});
makeCard(SEED,60,72,560,340,'',SEED_OWNED); // the share's seed terminal
</script>
</body></html>"##;

/// Spawn the mirror daemon detached (setsid → its own session, inherits this process's kitty
/// socket env) for `window`, open the browser, return the URL. Called from the matou TUI, which is
/// a real kitty window (so it has KITTY_LISTEN_ON); the daemon survives matou closing.
pub fn start_detached(window: i64, port: u16, owned_seed: bool) -> String {
    // Reclaim the port: a daemon from an earlier share may still be squatting it
    // (its window outlived the share, or it predates the watchdog). Without this the
    // new daemon fails to bind and the browser connects to the stale one — which is
    // mirroring a dead window, so it serves no frames (the black-screen bug). The new
    // daemon's own bind-retry covers the brief gap while the old one releases.
    let _ = kill_port(port);
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "matou".into());
    let mut args = vec![
        "mirror".to_string(),
        "--window".into(),
        window.to_string(),
        "--port".into(),
        port.to_string(),
    ];
    if owned_seed {
        args.push("--owned-seed".into()); // a hidden OS-window seed the daemon closes on teardown
    }
    let _ = Command::new("setsid")
        .arg(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let url = format!("http://127.0.0.1:{port}");
    // Open in a normal tab via the user's default browser. A normal tab can't self-close, so on
    // exit the page falls back to an "ended" notice rather than disappearing.
    let _ = Command::new("kitty").args(["@", "launch", "--type=background", "xdg-open", &url]).status();
    url
}
