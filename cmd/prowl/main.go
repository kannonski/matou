// prowl — a flat fuzzy palette for kitty: jump to an open project tab, or open a directory
// (zoxide + your ~/Project roots) in a chosen layout, with a live preview and per-tab status
// (focused · running · idle · failed). Reuses your palette.py layout engine. A remote-control
// client (not a kitten): run inside kitty with allow_remote_control + listen_on. Launched as
// an overlay tagged user_var prowl=1; it self-toggles (closes a sibling prowl on startup), so
// the bound key opens it once and dismisses it on a second press — no Python kitten needed.
package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"strconv"

	tea "github.com/charmbracelet/bubbletea"
)

func loadModel() (model, error) {
	m := model{
		cache:       map[string]string{},
		replyCache:  map[string]string{},
		workingDirs: map[string]bool{},
		lastInstr:   map[string]string{},
	}
	m = m.reload()
	if m.err != "" {
		return m, errors.New(m.err)
	}
	return m, nil
}

func main() {
	once := flag.Bool("once", false, "render once to stdout and exit (no TUI)")
	flag.Parse()
	loadConfig()

	// Singleton toggle: prowl is launched as an overlay tagged user_var prowl=1. If one is
	// already open in this tab, close it and exit (the new overlay flashes briefly, then both
	// close) — pressing the bound key again dismisses prowl. No kitten needed.
	if !*once {
		if self, _ := strconv.Atoi(os.Getenv("KITTY_WINDOW_ID")); self != 0 {
			if tree, err := kittyLS(); err == nil {
				if other := findOtherProwl(tree, self); other != 0 {
					_ = closeWindow(other)
					return
				}
			}
		}
	}

	m, err := loadModel()
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
