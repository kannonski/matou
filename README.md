# prowl

A **palette for [kitty](https://sw.kovidgoyal.net/kitty/)** — vim-navigated, one keystroke to
jump to an open project tab, or open a directory in a chosen layout. A remote-control client
(it shells out to `kitty @`), not a kitten, so it's a normal Go TUI.

```
prowl   j/k nav · l open · / search
  ⏵ infra-base   nvim main *4 │ infra-base    main    4 changes
  ○ scripts              zsh  │ Justfile  README.md  modules/  …
  + ibm-helper                │
```

The list is just jump/open targets:

- **`⏵ ○ ● ✗` open tabs** — `l`/enter jumps (no duplicate). The glyph is live status:
  running · idle · focused · last command failed; the meta is the running cmd + git dirty.
- **`+` projects** — directories from [zoxide](https://github.com/ajeetdsouza/zoxide) +
  your `~/Project/{gitlab,github}` roots, deduped against what's open → `l` picks a layout.

Actions live on keys, not rows: `.` relayout the current dir · `m` move a pane (pick the
pane, then drop it into a destination tab).

The right pane previews the selection in clear sections — **REPO** (git branch + change
count) and **FILES** (listing) for a directory, the layout sketch when you're choosing one,
and an **AGENT** teaser on top once you've asked the `?` agent something (see below).

## Keys

Vim navigation; search lives behind `/`.

| Key | Action |
|-----|--------|
| `j`/`k` · `↑`/`↓` | move (`g`/`G` top/bottom) |
| `l` / `enter` | open: jump to a tab · pick-a-layout for a dir |
| `m` | move a pane — pick the pane, then `↵` to drop it into a destination tab (`esc` steps back) |
| `.` | relayout the current dir (layout picker for where you launched) |
| `?` | ask the agent about the selected dir (floating panel) |
| `x` | close the highlighted tab |
| `r` | rename the highlighted tab |
| `h` | back out (in the layout picker → back to the list) |
| `/` | search **all** projects — type to filter, `esc` back to nav |
| `q` / `esc` | quit |

The default list is short — open tabs, relay, and the **~10 most recent projects**. Hit `/`
to search the full set.

## Requirements

- **kitty** with `allow_remote_control` + `listen_on` (so `kitty @` works).
- A **layout engine** at `~/.config/kitty/palette.py` exposing `names` / `sketch <name>` /
  `build <name> <dir>` (override the path with `$PROWL_PALETTE`). prowl reuses it rather than
  reinventing layouts.
- Optional: `zoxide` (project frecency), `git` + `ls` (previews).

## Agent — `?` (optional)

Press **`?`** to ask an agent about the selected directory. A floating panel opens
(over the palette); type an instruction, `enter` runs `$PROWL_AGENT_CMD <dir> "<instruction>"`
**async** (`🤖 working…` until it returns), the reply fills the panel (`↑↓` scroll), `esc`
closes. Replies are **cached** per dir+instruction and **persist across restarts** (in
`$XDG_CACHE_HOME/prowl/agent.json`). Unset = `?` is disabled.

Since it's async you needn't wait: close the panel and keep browsing — the right pane's
**AGENT** section shows `🤖 working…` for that dir, then a **10-line teaser** of the answer
(with the question) once it lands, and keeps it there when you revisit. To read the full
answer, press `?` again — the floating panel restores the last Q&A and is where the whole
reply lives (`↑↓` to scroll).

```sh
export PROWL_AGENT_CMD="$HOME/.config/kitty/prowl-agent.sh"   # called: <cmd> <dir> "<instruction>"
```

See [`examples/agent.sh`](examples/agent.sh) for a sample that answers with an LLM (`claude`),
grounded in the repo's git state + README, with read-only tools.

## Install

```sh
go install github.com/kannonski/prowl/cmd/prowl@latest
```

Bind it to a self-toggling overlay (no kitten — pure Go). The `--var prowl=1` tag lets
prowl detect a sibling on startup and dismiss it on a second press:

```conf
# kitty.conf  (needs allow_remote_control + listen_on)
map ctrl+shift+o launch --type=overlay --cwd=current --title=prowl --var prowl=1 ~/.local/bin/prowl
```

`--cwd=current` gives the relayout key (`.`) its directory. Run `prowl` directly to try it
without binding.

## License

[MIT](LICENSE).
