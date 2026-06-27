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

| Key | Action |
|-----|--------|
| *type* | fuzzy-filter the list |
| `↑`/`↓` · `ctrl-p`/`ctrl-n` | move |
| `enter` | jump (open tab) · pick-a-layout (relay/project) · move (move-targets) |
| `ctrl-s` | move the pane you came from into the highlighted tab |
| `ctrl-x` | close the highlighted tab |
| `ctrl-r` | rename the highlighted tab |
| `ctrl-d` | prune a project from zoxide |
| `esc` / `ctrl-c` | quit |

## Requirements

- **kitty** with `allow_remote_control` + `listen_on` (so `kitty @` works).
- A **layout engine** at `~/.config/kitty/palette.py` exposing `names` / `sketch <name>` /
  `build <name> <dir>` (override the path with `$PROWL_PALETTE`). prowl reuses it rather than
  reinventing layouts.
- Optional: `zoxide` (project frecency), `git` + `ls` (previews).

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
