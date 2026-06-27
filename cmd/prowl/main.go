// prowl — a flat fuzzy palette for kitty: jump to an open project tab, or open a directory
// (zoxide + your ~/Project roots) in a chosen layout, with a live preview and per-tab status
// (focused · running · idle · failed). Move/kill/rename/prune are the per-row actions.
// Reuses your palette.py layout engine. A remote-control client (not a kitten): run inside
// kitty with allow_remote_control + listen_on. The prowl.py kitten launches it with --source.
package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"strconv"

	tea "github.com/charmbracelet/bubbletea"
)

func loadModel(source int) (model, error) {
	m := model{cache: map[string]string{}, source: source}
	m = m.reload()
	if m.err != "" {
		return m, errors.New(m.err)
	}
	return m, nil
}

func main() {
	once := flag.Bool("once", false, "render once to stdout and exit (no TUI)")
	source := flag.Int("source", 0, "window id to move on ctrl-s (set by the prowl.py kitten)")
	flag.Parse()

	m, err := loadModel(*source)
	if err != nil {
		fmt.Fprintln(os.Stderr, "prowl: `kitty @ ls` failed — run inside kitty with remote control enabled")
		fmt.Fprintln(os.Stderr, "  (allow_remote_control + listen_on). detail:", err)
		os.Exit(1)
	}
	if *once { // honor COLUMNS/LINES so the layout can be checked headlessly
		if c, _ := strconv.Atoi(os.Getenv("COLUMNS")); c > 0 {
			m.w = c
		}
		if l, _ := strconv.Atoi(os.Getenv("LINES")); l > 0 {
			m.h = l
		}
		fmt.Println(m.View())
		return
	}
	if _, err := tea.NewProgram(m, tea.WithAltScreen()).Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
