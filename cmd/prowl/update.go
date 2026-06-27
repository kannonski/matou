package main

import (
	"strings"

	tea "github.com/charmbracelet/bubbletea"
)

func (m model) Init() tea.Cmd { return settleTick(m.settleGen) }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
	case previewMsg: // an agent preview came back
		m.agentCache[msg.dir] = msg.text
		if m.pending == msg.dir {
			m.pending = ""
		}
		if it, ok := m.sel(); ok && it.dir == msg.dir {
			m = m.refreshPreview()
		}
	case settleMsg:
		return m.settle(msg.gen)
	case tea.KeyMsg:
		switch m.mode {
		case "layout":
			return m.updateLayout(msg)
		case "rename":
			return m.updateRename(msg)
		case "filter":
			return m.updateFilter(msg)
		case "move":
			return m.updateMove(msg)
		default:
			return m.updateNav(msg)
		}
	}
	return m, nil
}

// moved refreshes the preview after a cursor/selection change and (re)arms the settle
// debounce, so the agent hook fires only once the cursor rests on a row.
func (m model) moved() (model, tea.Cmd) {
	m = m.refreshPreview()
	if previewHook == "" {
		return m, nil
	}
	m.settleGen++
	return m, settleTick(m.settleGen)
}

// settle fires when the cursor has rested: if the selected dir has no agent preview yet,
// kick off the hook (the pane shows ⏳ until it returns).
func (m model) settle(gen int) (tea.Model, tea.Cmd) {
	if gen != m.settleGen || previewHook == "" || m.mode == "layout" || m.mode == "rename" {
		return m, nil
	}
	it, ok := m.sel()
	if !ok || it.dir == "" {
		return m, nil
	}
	if _, done := m.agentCache[it.dir]; done || m.pending == it.dir {
		return m, nil
	}
	m.pending = it.dir
	return m.refreshPreview(), previewCmd(it.dir)
}

// actOn performs a row's primary action: jump (open) · move (newtab/newwin) · pick-a-layout
// (relay/project). Used by both a label tap and enter.
func (m model) actOn(idx int) (model, tea.Cmd) {
	m.cur = clamp(idx, len(m.view))
	it, ok := m.sel()
	if !ok {
		return m, nil
	}
	if it.kind == "open" {
		_ = focusWindow(it.winID)
		return m, tea.Quit
	}
	// project → pick a layout for that dir
	m.mode, m.layDir, m.layCur = "layout", it.dir, 0
	m.layouts = paletteNames()
	return m.moved()
}

// updateNav (default mode): vim hjkl navigation. j/k move · l/enter open/drill · h back
// out · g/G top/bottom · "/" search · ctrl-s/x/r/d row actions. No typing-to-filter here —
// letters are navigation; search lives behind "/".
func (m model) updateNav(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "q", "esc", "ctrl+c", "h":
		return m, tea.Quit
	case "j", "down", "ctrl+n":
		m.cur = clamp(m.cur+1, len(m.view))
		return m.moved()
	case "k", "up", "ctrl+p":
		m.cur = clamp(m.cur-1, len(m.view))
		return m.moved()
	case "g", "home":
		m.cur = 0
		return m.moved()
	case "G", "end":
		m.cur = clamp(1<<30, len(m.view))
		return m.moved()
	case "l", "enter":
		return m.actOn(m.cur)
	case "/":
		m.mode, m.query = "filter", ""
		return m.applyFilter().moved()
	case "m": // move a pane → enter move mode (pick the pane, then a destination)
		m.mode, m.moveSrc, m.moveSrcTab, m.moveSrcName = "move", 0, 0, ""
		return m.applyFilter().moved()
	case ".": // relayout the current dir → pick a layout
		if m.cwd != "" {
			m.mode, m.layDir, m.layCur = "layout", m.cwd, 0
			m.layouts = paletteNames()
			return m.moved()
		}
	case "x": // close the highlighted tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			_ = closeTab(it.tabID)
			return m.reload().moved()
		}
	case "r": // rename the highlighted tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			m.mode, m.rtab, m.rinput = "rename", it.tabID, it.title
			return m, nil
		}
	}
	return m, nil
}

// updateFilter: type to narrow, arrows + enter to act, esc back to labels.
func (m model) updateFilter(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.Type {
	case tea.KeyCtrlC:
		return m, tea.Quit
	case tea.KeyEsc:
		m.mode, m.query = "", ""
		return m.applyFilter().moved()
	case tea.KeyEnter:
		return m.actOn(m.cur)
	case tea.KeyUp, tea.KeyCtrlP:
		m.cur = clamp(m.cur-1, len(m.view))
		return m.moved()
	case tea.KeyDown, tea.KeyCtrlN:
		m.cur = clamp(m.cur+1, len(m.view))
		return m.moved()
	case tea.KeyBackspace:
		if r := []rune(m.query); len(r) > 0 {
			m.query = string(r[:len(r)-1])
		}
		return m.applyFilter().moved()
	case tea.KeyCtrlU:
		m.query = ""
		return m.applyFilter().moved()
	case tea.KeySpace:
		m.query += " "
		return m.applyFilter().moved()
	case tea.KeyRunes:
		m.query += string(msg.Runes)
		return m.applyFilter().moved()
	}
	return m, nil
}

func (m model) updateLayout(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "ctrl+c", "q":
		return m, tea.Quit
	case "esc", "h": // back to the palette
		m.mode = ""
		return m.moved()
	case "enter", "l":
		if m.layCur >= 0 && m.layCur < len(m.layouts) {
			_ = paletteBuild(m.layouts[m.layCur], m.layDir)
			return m, tea.Quit
		}
	case "j", "down", "ctrl+n":
		m.layCur = clamp(m.layCur+1, len(m.layouts))
		return m.moved()
	case "k", "up", "ctrl+p":
		m.layCur = clamp(m.layCur-1, len(m.layouts))
		return m.moved()
	}
	return m, nil
}

// updateMove drives the two-stage move. Stage A (moveSrc==0): pick the pane to move from
// the tab list. Stage B: pick a destination — enter into the highlighted tab, M = a new
// tab, W = a new OS window. esc steps back (B→A) or cancels (A→nav).
func (m model) updateMove(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "ctrl+c", "q":
		return m, tea.Quit
	case "esc":
		if m.moveSrc != 0 { // stage B → back to picking the pane
			m.moveSrc, m.moveSrcTab, m.moveSrcName = 0, 0, ""
			return m.applyFilter().moved()
		}
		m.mode = "" // stage A → back to nav
		return m.applyFilter().moved()
	case "j", "down", "ctrl+n":
		m.cur = clamp(m.cur+1, len(m.view))
		return m.moved()
	case "k", "up", "ctrl+p":
		m.cur = clamp(m.cur-1, len(m.view))
		return m.moved()
	case "enter", "l":
		it, ok := m.sel()
		if !ok {
			return m, nil
		}
		if m.moveSrc == 0 { // stage A: this pane will be moved
			m.moveSrc, m.moveSrcTab, m.moveSrcName = it.winID, it.tabID, nameOf(it)
			return m.applyFilter().moved()
		}
		_ = moveToTab(m.moveSrc, it.tabID) // stage B: into the highlighted tab
		return m, tea.Quit
	case "M": // stage B: to a new tab
		if m.moveSrc != 0 {
			_ = moveToNewTab(m.moveSrc)
			return m, tea.Quit
		}
	case "W": // stage B: to a new OS window
		if m.moveSrc != 0 {
			_ = moveToNewOSWindow(m.moveSrc)
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m model) updateRename(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.Type {
	case tea.KeyCtrlC:
		return m, tea.Quit
	case tea.KeyEsc:
		m.mode = ""
		return m, nil
	case tea.KeyEnter:
		if strings.TrimSpace(m.rinput) != "" {
			_ = setTabTitle(m.rtab, m.rinput)
		}
		m.mode = ""
		return m.reload(), nil
	case tea.KeyBackspace:
		if r := []rune(m.rinput); len(r) > 0 {
			m.rinput = string(r[:len(r)-1])
		}
	case tea.KeyCtrlU:
		m.rinput = ""
	case tea.KeySpace:
		m.rinput += " "
	case tea.KeyRunes:
		m.rinput += string(msg.Runes)
	}
	return m, nil
}
