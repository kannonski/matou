# prowl

A flat, fuzzy **palette for [kitty](https://sw.kovidgoyal.net/kitty/)** — one keystroke to
jump to an open project tab, or open a directory in a chosen layout. A remote-control client
(it shells out to `kitty @`), not a kitten, so it's a normal Go TUI.

```
❯ infra▌   3
  ⏵ infra-base            │ infra-base    main    4 changes
  ○ infra-base            │
  + infra-base            │ Justfile  README.md  modules/  …
  ↻ relayout · infra-base │
```

One filterable list:

- **`↻` relay** — the current dir → pick a layout.
- **`⏵ ○ ● ✗` open tabs** — jump to one (no duplicate). The glyph is its live status:
  running · idle · focused · last command failed.
- **`+` projects** — directories from [zoxide](https://github.com/ajeetdsouza/zoxide) +
  your `~/Project/{gitlab,github}` roots, deduped against what's already open → pick a layout.

The right pane previews the selection: git branch + change count + listing for a directory,
or the layout sketch when you're choosing one.

## Keys

Vim navigation; search lives behind `/`.

| Key | Action |
|-----|--------|
| `j`/`k` · `↑`/`↓` | move (`g`/`G` top/bottom) |
| `l` / `enter` | open: jump to a tab · pick-a-layout for a dir · move (move-targets) |
| `m` | move the pane you came from into the highlighted tab |
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

To bind it (and capture the source window for `ctrl-s`), launch via a tiny kitten:

```python
# ~/.config/kitty/prowl.py
import os
from kittens.tui.handler import result_handler
def main(args): pass
@result_handler(no_ui=True)
def handle_result(args, answer, target_window_id, boss):
    w = boss.active_window
    if w:
        boss.call_remote_control(w, ("launch", "--type=tab", "--cwd=current",
            "--title=prowl", os.path.expanduser("~/.local/bin/prowl"), "--source", str(w.id)))
```

```conf
# kitty.conf
map ctrl+shift+o kitten prowl.py
```

Run `prowl` directly to try it without the launcher (move-pane is then disabled).

## License

[MIT](LICENSE).
