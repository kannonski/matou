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

The right pane previews the selection: git branch + change count + listing for a directory,
or the layout sketch when you're choosing one.

## Keys

Vim navigation; search lives behind `/`.

| Key | Action |
|-----|--------|
| `j`/`k` · `↑`/`↓` | move (`g`/`G` top/bottom) |
| `l` / `enter` | open: jump to a tab · pick-a-layout for a dir · move (move-targets) |
| `m` | move a pane — pick the pane, then `↵` to drop it into a destination tab (`esc` steps back) |
| `.` | relayout the current dir (layout picker for where you launched) |
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

## Right-pane agent (optional)

Set `$PROWL_PREVIEW_CMD` to a command and prowl runs it for the selected directory and
shows its output **on top of** the git + listing preview — e.g. an AI brief of the repo.
It's **debounced** (only fires for rows you pause on, ~350 ms), **async** (nav stays
snappy; the pane shows `⏳` until it returns), and **cached** per session. Unset = no hook.

```sh
export PROWL_PREVIEW_CMD="$HOME/.config/kitty/prowl-preview.sh"   # receives <dir> as $1
```

See [`examples/agent-preview.sh`](examples/agent-preview.sh) for a sample that briefs the
repo with an LLM (falls back to `git log` when none is available).

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
