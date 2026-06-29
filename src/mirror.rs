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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// End this mirror session and exit: signal connected pages (`event: end`), drop the pidfile, go.
/// Leaves the user's windows alone — sharing just stops mirroring them.
fn shutdown_now() -> ! {
    SHUTDOWN.store(true, Ordering::Relaxed);
    let _ = std::fs::remove_file(pidfile());
    std::thread::sleep(Duration::from_millis(300));
    std::process::exit(0);
}

pub fn run(args: &[String]) -> Result<()> {
    let mut window: Option<String> = None;
    let mut port: u16 = 9123;
    let mut bind = "127.0.0.1".to_string();
    let mut do_stop = false;
    let mut p2p = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stop" => do_stop = true,
            "--p2p" => p2p = true,
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

    // watchdog: the daemon is detached, so exit on its own once the workspace is gone — otherwise
    // closing the tab leaves an orphaned daemon (the "zombie"). kittyweb panes are all windows in
    // the seed's tab, so track the *tab*: the session lives as long as any pane remains, and ends
    // only when the tab is empty/closed (falls back to the seed window if the tab can't resolve).
    {
        let win = window.clone();
        let tab_id = tab_of_window(&window);
        std::thread::spawn(move || {
            let mut misses = 0;
            let mut gone = 0;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                // browser gone (every pane's SSE dropped after one ever connected) → stop serving
                if SEEN_CLIENT.load(Ordering::Relaxed) && STREAMS.load(Ordering::Relaxed) == 0 {
                    gone += 1;
                    if gone >= 3 {
                        // ~6s with no panes — longer than EventSource's reconnect, so a transient
                        // drop won't trip it. /bye (sendBeacon on tab close) handles the fast path.
                        shutdown_now();
                    }
                } else {
                    gone = 0;
                }
                let alive = match tab_id {
                    Some(t) => tab_alive(t),
                    None => window_exists(&win),
                };
                match alive {
                    Some(false) => {
                        misses += 1;
                        if misses >= 2 {
                            shutdown_now();
                        }
                    }
                    _ => misses = 0, // alive, or a transient `kitty @ ls` failure → don't exit
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
            respond(&mut stream, "200 OK", "application/json", format!("{{\"cols\":{c},\"rows\":{r}}}").as_bytes())?;
        }
        ("GET", "/stream") => stream_loop(&mut stream, &matchspec)?,
        ("POST", "/key") => {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body)?;
            send_input(&matchspec, &body);
            write!(stream, "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        }
        ("POST", "/split") => {
            // grow the workspace: split the given window and report the new window's id
            let side = query_param(query, "side").unwrap_or_default();
            let from = win.clone().unwrap_or_else(|| seed_id.to_string());
            let body = match split_window(&from, &side) {
                Some(id) => format!("{{\"w\":{id}}}"),
                None => "{\"w\":null}".to_string(),
            };
            respond(&mut stream, "200 OK", "application/json", body.as_bytes())?;
        }
        ("POST", "/close") => {
            if let Some(w) = &win {
                let _ = Command::new("kitty")
                    .args(["@", "close-window", "--match", &format!("id:{w}")])
                    .status();
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

/// Split window `w` (`right` → vsplit, else `down`/hsplit) and return the new window's id. The new
/// pane inherits `w`'s cwd. `--next-to` only honours a target in the *active* tab, so focus `w`
/// first — otherwise the split lands in whatever tab the user last touched. `--keep-focus` then
/// leaves focus on `w` (the new pane is grafted in beside it).
fn split_window(w: &str, side: &str) -> Option<i64> {
    let loc = if side == "down" { "hsplit" } else { "vsplit" };
    let _ = Command::new("kitty")
        .args(["@", "focus-window", "--match", &format!("id:{w}")])
        .status();
    let mut args = vec![
        "@".to_string(),
        "launch".into(),
        format!("--location={loc}"),
        "--next-to".into(),
        format!("id:{w}"),
        "--keep-focus".into(),
    ];
    if let Some(cwd) = window_cwd(w) {
        args.push("--cwd".into());
        args.push(cwd);
    }
    let out = Command::new("kitty").args(&args).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Window `id`'s cwd from `kitty @ ls`, so a new split opens in the same directory.
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
/// matter how `stream_loop` returns.
struct StreamGuard;
impl Drop for StreamGuard {
    fn drop(&mut self) {
        STREAMS.fetch_sub(1, Ordering::Relaxed);
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

/// The id of the tab containing window `id`, resolved once at startup so the watchdog can track the
/// whole workspace (all kittyweb panes live in this tab) rather than the single seed window.
fn tab_of_window(id: &str) -> Option<i64> {
    let out = Command::new("kitty").args(["@", "ls"]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for ow in v.as_array().into_iter().flatten() {
        for tab in ow["tabs"].as_array().into_iter().flatten() {
            for w in tab["windows"].as_array().into_iter().flatten() {
                if w["id"].as_u64().map(|x| x.to_string()).as_deref() == Some(id) {
                    return tab["id"].as_i64();
                }
            }
        }
    }
    None
}

/// Whether tab `tab_id` still holds at least one window. None on a transient `kitty @ ls` failure.
fn tab_alive(tab_id: i64) -> Option<bool> {
    let out = Command::new("kitty").args(["@", "ls"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let alive = v.as_array().into_iter().flatten().any(|ow| {
        ow["tabs"].as_array().into_iter().flatten().any(|t| {
            t["id"].as_i64() == Some(tab_id)
                && t["windows"].as_array().is_some_and(|w| !w.is_empty())
        })
    });
    Some(alive)
}

/// Whether window `id` still exists: Some(true/false) if `kitty @ ls` succeeded, None on a
/// transient failure (so the watchdog won't kill the daemon on a hiccup).
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
    PAGE.replace("__FONT__", &font_family())
        .replace("__THEME__", &theme_json(&c))
        .replace("__BG__", &bg)
        .replace("__SEED__", seed)
}

// kittyweb: a composable workspace. Each pane is a real kitty window (xterm.js fed by SSE, input
// POSTed back). The browser owns the split tree — hovering a pane shows a `+` on its right edge
// (vsplit) and bottom edge (hsplit); clicking launches a real kitty split and grafts the new pane
// into the tree as nested flexboxes. No preset layouts — you build the layout by growing it.
const PAGE: &str = r##"<!doctype html>
<html><head><meta charset=utf-8><title>kittyweb</title>
<link rel=stylesheet href=/xterm.css>
<style>
 html,body{margin:0;height:100%;background:__BG__;overflow:hidden;font-family:monospace}
 #root{position:absolute;inset:0;display:flex}
 #root>*{flex:1 1 0;min-width:0;min-height:0}
 .split{display:flex;flex:1 1 0;min-width:0;min-height:0;gap:2px}
 .split.row{flex-direction:row}
 .split.col{flex-direction:column}
 .split>*{flex:1 1 0;min-width:0;min-height:0}
 .pane{position:relative;overflow:hidden;display:flex;align-items:center;justify-content:center;
   background:__BG__;box-shadow:inset 0 0 0 1px #2a2a3a}
 .pane.act{box-shadow:inset 0 0 0 1px #8aadf4}
 .pane .term{transform-origin:center center}
 .xterm,.xterm-viewport{background:__BG__ !important}
 .add,.cls{position:absolute;z-index:5;opacity:0;transition:opacity .1s;cursor:pointer;border:0;
   color:#cdd6f4;background:#1e1e2ecc;font:14px monospace;line-height:1;
   display:flex;align-items:center;justify-content:center}
 .pane:hover .add,.pane:hover .cls{opacity:.55}
 .add:hover,.cls:hover{opacity:1 !important;background:#8aadf4dd;color:#11111b}
 .addR{top:0;bottom:0;right:0;width:15px}
 .addD{left:0;right:0;bottom:0;height:15px}
 .cls{top:0;right:15px;width:18px;height:15px;border-radius:0 0 0 4px}
 #bar{position:fixed;bottom:0;right:0;font:11px monospace;color:#999;background:#000a;
   padding:3px 7px;z-index:10}
</style></head>
<body>
<div id=root></div>
<div id=bar>kittyweb · live</div>
<script src=/xterm.js></script>
<script>
const THEME=__THEME__,FONT="__FONT__",SEED="__SEED__";
const bar=document.getElementById('bar'),root=document.getElementById('root');
const leaves=new Set();
let active=null,ended=false;

function endSession(){
  if(ended)return;ended=true;
  bar.textContent='kittyweb · ended';
  window.close(); // works for a script-opened window; a normal tab can't self-close → notice below
  setTimeout(()=>{document.title='kittyweb — ended';
    document.body.innerHTML='<div style="position:absolute;inset:0;display:flex;'+
      'align-items:center;justify-content:center;color:#888;font:14px monospace">'+
      'session ended — you can close this tab</div>';},120);
}
function updateBar(){if(!ended)bar.textContent='kittyweb · '+leaves.size+' pane'+(leaves.size>1?'s':'');}

// scale a pane's source-sized grid to fit its (flex-sized) box
function fit(leaf){const el=leaf.el,h=leaf.host,nw=h.offsetWidth,nh=h.offsetHeight;
  if(!nw||!nh||!el.clientWidth)return;
  const s=Math.min((el.clientWidth-2)/nw,(el.clientHeight-2)/nh);
  h.style.transform='scale('+(s>0?s:0.01)+')';}
// re-poll the window's *real* grid — kitty re-tiles each window on split/close, so the grid shrinks
// with the box and the scale stays consistent across panes (without this the old grid is squeezed
// into an ever-smaller box and the pane zooms way out).
function refit(leaf){
  fetch('/size?w='+leaf.win).then(r=>r.json()).then(s=>{
    if(s&&s.cols&&s.rows)leaf.term.resize(s.cols,s.rows);
    fit(leaf);
  }).catch(()=>fit(leaf));
}
function refitAll(){setTimeout(()=>leaves.forEach(refit),60);setTimeout(()=>leaves.forEach(refit),300);}

function btn(cls,txt,fn){const b=document.createElement('button');b.className=cls;b.textContent=txt;
  b.onclick=e=>{e.stopPropagation();fn();};return b;}

function setActive(leaf){if(active)active.el.classList.remove('act');
  active=leaf;if(leaf){leaf.el.classList.add('act');leaf.term.focus();}}

function makeLeaf(win){
  const el=document.createElement('div');el.className='pane';
  const host=document.createElement('div');host.className='term';el.appendChild(host);
  const leaf={kind:'leaf',win,el,host,term:null,es:null,parent:null};
  el.append(btn('add addR','+',()=>split(leaf,'right')),
            btn('add addD','+',()=>split(leaf,'down')),
            btn('cls','×',()=>closeLeaf(leaf)));
  const term=new Terminal({fontFamily:FONT,fontSize:13,cursorBlink:false,scrollback:0,theme:THEME});
  term.open(host);leaf.term=term;
  el.addEventListener('mousedown',()=>setActive(leaf));
  term.onData(d=>fetch('/key?w='+win,{method:'POST',body:d}));
  refit(leaf); // size to the real window grid, then scale to the box
  const es=new EventSource('/stream?w='+win);leaf.es=es;
  es.onmessage=e=>{term.write(Uint8Array.from(atob(e.data),c=>c.charCodeAt(0)));fit(leaf);};
  es.addEventListener('end',endSession);
  es.addEventListener('gone',()=>dropLeaf(leaf,false)); // window closed in the terminal → drop pane
  new ResizeObserver(()=>fit(leaf)).observe(el);
  leaves.add(leaf);updateBar();
  return leaf;
}

// graft `newn` where `oldn` sits (root, or a child slot of its parent split)
function replaceNode(oldn,newn){
  const p=oldn.parent;newn.parent=p;
  if(!p){root.replaceChild(newn.el,oldn.el);return;}
  if(p.a===oldn)p.a=newn;else p.b=newn;
  p.el.replaceChild(newn.el,oldn.el);
}

function split(leaf,side){
  fetch('/split?w='+leaf.win+'&side='+side,{method:'POST'}).then(r=>r.json()).then(({w})=>{
    if(!w){bar.textContent='kittyweb · split failed';return;}
    const nl=makeLeaf(w);
    const sp={kind:'split',dir:side==='down'?'col':'row',a:leaf,b:nl,
      el:document.createElement('div'),parent:null};
    sp.el.className='split '+sp.dir;
    replaceNode(leaf,sp);          // sp takes leaf's place in the tree + DOM
    leaf.parent=sp;nl.parent=sp;
    sp.el.append(leaf.el,nl.el);   // then leaf + new pane live inside sp
    setActive(nl);
    refitAll();                    // kitty halved the old window's grid — match it everywhere
  }).catch(()=>{bar.textContent='kittyweb · split failed';});
}

// remove a pane from the tree. tellKitty=true (× button) also closes the kitty window; false is for
// a window that's already gone (closed in the terminal → server sent `event: gone`).
function dropLeaf(leaf,tellKitty){
  if(!leaves.has(leaf))return;     // already dropped (guards a close/gone race)
  if(tellKitty)fetch('/close?w='+leaf.win,{method:'POST'}).catch(()=>{});
  if(leaf.es)leaf.es.close();
  leaves.delete(leaf);updateBar();
  const p=leaf.parent;
  if(!p){endSession();return;}     // dropped the last pane
  const sib=p.a===leaf?p.b:p.a;    // sibling is promoted into p's slot
  replaceNode(p,sib);
  if(active===leaf)setActive(firstLeaf(sib));
  refitAll();                      // kitty reclaimed the space — match the new grids
}
function closeLeaf(leaf){dropLeaf(leaf,true);}
function firstLeaf(n){while(n.kind!=='leaf')n=n.a;return n;}

const seed=makeLeaf(SEED);root.appendChild(seed.el);setActive(seed);
addEventListener('resize',()=>leaves.forEach(fit));
addEventListener('pagehide',()=>{try{navigator.sendBeacon('/bye')}catch(_){}}); // browser gone → tear down
</script>
</body></html>"##;

/// Spawn the mirror daemon detached (setsid → its own session, inherits this process's kitty
/// socket env) for `window`, open the browser, return the URL. Called from the matou TUI, which is
/// a real kitty window (so it has KITTY_LISTEN_ON); the daemon survives matou closing.
pub fn start_detached(window: i64, port: u16) -> String {
    // Reclaim the port: a daemon from an earlier share may still be squatting it
    // (its window outlived the share, or it predates the watchdog). Without this the
    // new daemon fails to bind and the browser connects to the stale one — which is
    // mirroring a dead window, so it serves no frames (the black-screen bug). The new
    // daemon's own bind-retry covers the brief gap while the old one releases.
    let _ = kill_port(port);
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "matou".into());
    let args = [
        "mirror".to_string(),
        "--window".into(),
        window.to_string(),
        "--port".into(),
        port.to_string(),
    ];
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
