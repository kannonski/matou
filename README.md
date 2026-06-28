# matou

A **palette for [kitty](https://sw.kovidgoyal.net/kitty/)** — vim-navigated, one keystroke to
jump to an open project tab, or open a directory in a chosen layout. A remote-control client
(it shells out to `kitty @`), not a kitten, so it's a normal Go TUI.

## Demo

*Plain text here; in a real [Nerd Font](https://www.nerdfonts.com/) kitty it's
Catppuccin-colored with tool glyphs.*

The palette — jump to an open tab, or open a project dir in a layout. Live
per-tab status (`●` focused · `⏵` running · `○` idle), running cmd + git dirty,
and a sectioned preview on the right:

```
╭────────────────────────────────────────────────────────────────────────╮
│ matou   j/k nav · / search                                             │
│ ────────────────────────────────────────────────────────────────────── │
│   ● matou           nvim *2│ ▌ REPO                                    │
│   ⏵ infra               k9s│ matou · main · 2 changes                  │
│   ○ scripts             zsh│                                           │
│   + dotfiles               │ ▌ FILES                                   │
│   + umtls                  │ cmd                                       │
│                            │ Dockerfile                                │
│                            │ examples                                  │
│                            │ .git                                      │
│ ↵ jump · x close · r rename · m move · a ask · . relayout · / search … │
╰────────────────────────────────────────────────────────────────────────╯
```

Pick a layout — its colored sketch previews live on the right:

```
╭────────────────────────────────────────────────────────────────────────╮
│ layout for matou   ↵ build · esc back                                  │
│ ────────────────────────────────────────────────────────────────────── │
│   zsh     just a shell     │                                           │
│   dev     editor · shell … │  ╭───────────────────────┬──────────────╮ │
│   tf      editor + shell … │  │  nvim              │  zsh      │ │
│   go      editor · two sh… │  │                       │              │ │
│   docs    file explorer (… │  │ ~                     │ ❯            │ │
│   k8s     k9s · shell      │  │ ~                     ├──────────────┤ │
│   logs    k9s · logs (ste… │  │ ~                     │  lazygit  │ │
│   claude  resume a Claude… │  │                       │              │ │
│                            │  ╰───────────────────────┴──────────────╯ │
│                            │                                           │
│ j/k pick · l/↵ build · h back                                          │
╰────────────────────────────────────────────────────────────────────────╯
```

Ask the agent about the selected project with `a` — async, scrollable, cached:

```
╭────────────────────────────────────────────────────────────────╮
│ 🤖 matou                                                       │
│ ❯ what does this build do?                                     │
│ ────────────────────────────────────────────────────────────── │
│ matou is a kitty palette in Go. It lists your open project     │
│ tabs and recent project dirs; `l` jumps to a tab or opens a    │
│ dir in a chosen layout. `a` asks an agent about the project.   │
│                                                                │
│                                                                │
│                                                                │
│ j/k scroll · ^d/^u half · g/G ends · i ask · esc               │
╰────────────────────────────────────────────────────────────────╯
```

The list is just jump/open targets:

- **`⏵ ○ ● ✗` open tabs** — `l`/enter jumps (no duplicate). The glyph is live status:
  running · idle · focused · last command failed; the meta is the running cmd + git dirty.
- **`+` projects** — directories from [zoxide](https://github.com/ajeetdsouza/zoxide) +
  your `~/Project/{gitlab,github}` roots, deduped against what's open → `l` picks a layout.

Actions live on keys, not rows: `.` relayout the current dir · `m` move a pane (pick the
pane, then drop it into a destination tab).

The right pane previews the selection in clear sections — **REPO** (git branch + change
count) and **FILES** (listing) for a directory, the layout's **sketch** when you're picking
one, and an **AGENT** teaser on top once you've asked the `a` agent something (see below).

Picking a layout (`l`/enter on a project, or `.` to relayout) shows a self-describing list —
each layout's name + caption — with its colored sketch previewed on the right. `j`/`k` pick,
`l`/`enter` builds, `h`/`esc` back.

## Keys

Vim navigation; search lives behind `/`.

| Key | Action |
|-----|--------|
| `j`/`k` · `↑`/`↓` | move (`g`/`G` top/bottom) |
| `l` / `enter` | open: jump to a tab · pick-a-layout for a dir |
| `m` | move a pane — pick the pane, then `↵` to drop it into a destination tab (`esc` steps back) |
| `.` | relayout the current dir (layout picker for where you launched) |
| `a` | ask the agent about the selected dir (floating panel) |
| `x` | close the highlighted tab |
| `r` | rename the highlighted tab |
| `h` | back out (in the layout picker → back to the list) |
| `/` | search **all** projects — type to filter, `esc` back to nav |
| `q` / `esc` | quit |

The default list is short — open tabs, relay, and the **~10 most recent projects**. Hit `/`
to search the full set.

## Requirements

- **kitty** with `allow_remote_control` + `listen_on` (so `kitty @` works).
- A **layouts file** at `~/.config/kitty/palette.layouts` (TOML) defining your layouts.
  matou's built-in Go engine parses it, draws the preview sketch, and launches the panes — no
  external script. Override the path with `$MATOU_LAYOUTS`. See
  [`examples/palette.layouts`](examples/palette.layouts) for the format.
- Optional: `zoxide` (project frecency, and `zoxide add` on build), `git` + `ls` (previews).

## Agent — `a` (optional)

Press **`a`** to ask an agent about the selected directory. The floating panel has two
focuses: **type** the question (`enter` runs `$MATOU_AGENT_CMD <dir> "<instruction>"` **async**,
`🤖 working…` until it returns) and **read** the answer (vim scroll: `j`/`k`, `^d`/`^u`
half-page, `g`/`G` ends — the footer shows a `12–24/80` position). `Tab` toggles focus; a reply
landing drops you straight into read, `i` jumps back to ask a follow-up, `esc` closes. Replies
are **cached** per dir+instruction and **persist across restarts** (in
`$XDG_CACHE_HOME/matou/agent.json`). Unset = `a` is disabled.

Since it's async you needn't wait: close the panel and keep browsing — the right pane's
**AGENT** section shows `🤖 working…` for that dir, then a **10-line teaser** of the answer
(with the question) once it lands, and keeps it there when you revisit. To read the full
answer, press `a` again — the floating panel restores the last Q&A and is where the whole
reply lives (`↑↓` to scroll).

```sh
export MATOU_AGENT_CMD="$HOME/.config/kitty/matou-agent.sh"   # called: <cmd> <dir> "<instruction>"
```

See [`examples/agent.sh`](examples/agent.sh) for a sample that answers with an LLM (`claude`),
grounded in the repo's git state + README, with read-only tools.

## Install

```sh
go install github.com/kannonski/matou/cmd/matou@latest
```

Bind it to a self-toggling overlay (no kitten — pure Go). The `--var matou=1` tag lets
matou detect a sibling on startup and dismiss it on a second press:

```conf
# kitty.conf  (needs allow_remote_control + listen_on)
map ctrl+shift+o launch --type=overlay --cwd=current --title=matou --var matou=1 ~/.local/bin/matou
```

`--cwd=current` gives the relayout key (`.`) its directory. Run `matou` directly to try it
without binding.

## License

[MIT](LICENSE).
