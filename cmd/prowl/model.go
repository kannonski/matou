package main

import (
	"os"
	"path/filepath"
	"strings"
)

// labelKeys are the jump-label keys, easiest first (home row → top → bottom). Row i in the
// filtered view gets labelKeys[i]; press it to jump straight to that row.
const labelKeys = "asdfghjklqwertyuiopzxcvbnm"

// labelFor returns the jump label for a view position ("" past the alphabet).
func labelFor(viewIdx int) string {
	if viewIdx >= 0 && viewIdx < len(labelKeys) {
		return string(labelKeys[viewIdx])
	}
	return ""
}

// item is one palette row.
type item struct {
	kind   string // "relay" | "open" | "project" | "newtab" | "newwin"
	dir    string // cwd (relay/open) or project dir
	winID  int    // open: window to focus
	tabID  int    // open: tab to kill / rename
	title  string // open: tab title; newtab/newwin: the label
	status string // open: focused | running | idle | failed
}

func (it item) filterStr() string {
	s := it.title + " " + it.dir
	if it.dir != "" {
		s += " " + filepath.Base(it.dir)
	}
	return strings.ToLower(s)
}

type model struct {
	all   []item // relay, open tabs, [move targets], projects (display order)
	view  []int  // indices into all that match query
	query string
	cur   int // index into view

	mode    string // "" palette | "layout" | "rename"
	layouts []string
	layCur  int
	layDir  string // dir chosen to lay out

	rtab   int    // tab being renamed
	rinput string // rename input buffer
	source int    // window id to move on ctrl-s (0 = launched without a source)

	preview string
	cache   map[string]string // dir:/layout: → preview text

	w, h   int
	status string
	err    string
}

// reload rebuilds the rows from `kitty @ ls` + the project sources, preserving the query,
// cursor and source. Called on start and after kill/prune/rename.
func (m model) reload() model {
	tabs, openCwds, err := openTabs()
	if err != nil {
		m.err = "kitty @ ls failed — is remote control on?"
		return m
	}
	m.err = ""
	var all []item
	// recent open tabs first (so the easiest labels jump to where you most likely want to go)
	for _, t := range tabs {
		all = append(all, item{kind: "open", dir: t.cwd, winID: t.winID, tabID: t.tabID, title: t.title, status: t.status})
	}
	if cwd, e := os.Getwd(); e == nil && cwd != "" {
		all = append(all, item{kind: "relay", dir: cwd})
	}
	if m.source > 0 { // move targets — only meaningful when launched with a source window
		all = append(all,
			item{kind: "newtab", title: "move the pane here → a new tab"},
			item{kind: "newwin", title: "move the pane here → a new OS window"})
	}
	for _, d := range projectDirs(openCwds) { // zoxide frecency order
		all = append(all, item{kind: "project", dir: d})
	}
	m.all = all
	if m.cache == nil {
		m.cache = map[string]string{}
	}
	return m.applyFilter().refreshPreview()
}

// applyFilter recomputes the visible rows for the query (case-insensitive substring).
func (m model) applyFilter() model {
	q := strings.ToLower(strings.TrimSpace(m.query))
	view := make([]int, 0, len(m.all))
	for i, it := range m.all {
		if q == "" || strings.Contains(it.filterStr(), q) {
			view = append(view, i)
		}
	}
	m.view = view
	m.cur = clamp(m.cur, len(view))
	return m
}

func (m model) sel() (item, bool) {
	if m.cur < 0 || m.cur >= len(m.view) {
		return item{}, false
	}
	return m.all[m.view[m.cur]], true
}

// refreshPreview computes (and caches) the right-pane preview for the current selection.
func (m model) refreshPreview() model {
	if m.mode == "layout" {
		if m.layCur >= 0 && m.layCur < len(m.layouts) {
			m.preview = m.cached("layout:"+m.layouts[m.layCur], func() string { return paletteSketch(m.layouts[m.layCur]) })
		} else {
			m.preview = ""
		}
		return m
	}
	it, ok := m.sel()
	switch {
	case !ok:
		m.preview = ""
	case it.kind == "newtab":
		m.preview = "Enter — move the pane you came from into a new tab."
	case it.kind == "newwin":
		m.preview = "Enter — move the pane you came from into a new OS window."
	case it.dir != "":
		m.preview = m.cached("dir:"+it.dir, func() string { return dirPreview(it.dir) })
	default:
		m.preview = ""
	}
	return m
}

func (m model) cached(key string, gen func() string) string {
	if c, ok := m.cache[key]; ok {
		return c
	}
	v := gen()
	m.cache[key] = v
	return v
}

func clamp(v, n int) int {
	switch {
	case n <= 0:
		return 0
	case v >= n:
		return n - 1
	case v < 0:
		return 0
	}
	return v
}
