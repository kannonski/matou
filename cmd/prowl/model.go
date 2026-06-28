package main

import (
	"os"
	"path/filepath"
	"strings"
)

// item is one palette row.
type item struct {
	kind    string // "open" | "project"
	dir     string // cwd (relay/open) or project dir
	winID   int    // open: window to focus
	tabID   int    // open: tab to kill / rename
	title   string // open: tab title; newtab/newwin: the label
	status  string // open: focused | running | idle | failed
	proc    string // open: foreground command
	branch  string // open: git branch
	changes int    // open: uncommitted changes
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
	cwd    string // launch dir, for the relayout key (.)

	// move mode: pick a pane (stage A: moveSrc==0), then a destination (stage B)
	moveSrc     int    // window id of the pane being moved
	moveSrcTab  int    // its tab (excluded as a destination)
	moveSrcName string // shown in the move-mode header

	preview string
	cache   map[string]string // dir:/layout: → local preview text

	// `:` agent: a floating panel over the palette
	agentInput   string            // instruction being typed
	agentDir     string            // dir the agent acts on (captured when : is pressed)
	agentName    string            // panel title
	agentResult  string            // the reply ("" = none yet)
	agentWorking bool              // true while the hook runs
	agentOff     int               // scroll offset in the reply
	replyCache   map[string]string // dir+\x00+instr → reply

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
	if cwd, e := os.Getwd(); e == nil {
		m.cwd = cwd // for the relayout key (.)
	}
	var all []item
	for _, t := range tabs { // recent open tabs first (frecency)
		all = append(all, item{
			kind: "open", dir: t.cwd, winID: t.winID, tabID: t.tabID, title: t.title,
			status: t.status, proc: t.proc, branch: t.branch, changes: t.changes,
		})
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

// topProjects caps how many projects show with no query — keeps the default list to the
// most-recent handful (open tabs + relay are always shown). `/` searches all of them.
const topProjects = 10

// applyFilter recomputes the visible rows for the query (case-insensitive substring). With
// no query it shows everything except the project long-tail (capped to topProjects, the
// most recent); typing a query searches the full set.
func (m model) applyFilter() model {
	if m.mode == "move" { // move mode lists tabs only (stage B excludes the source's tab)
		view := make([]int, 0, len(m.all))
		for i, it := range m.all {
			if it.kind != "open" {
				continue
			}
			if m.moveSrc != 0 && it.tabID == m.moveSrcTab {
				continue
			}
			view = append(view, i)
		}
		m.view = view
		m.cur = clamp(m.cur, len(view))
		return m
	}
	q := strings.ToLower(strings.TrimSpace(m.query))
	view := make([]int, 0, len(m.all))
	projects := 0
	for i, it := range m.all {
		if q != "" {
			if strings.Contains(it.filterStr(), q) {
				view = append(view, i)
			}
			continue
		}
		if it.kind == "project" {
			if projects >= topProjects {
				continue
			}
			projects++
		}
		view = append(view, i)
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
