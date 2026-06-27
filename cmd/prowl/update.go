package main

import (
	"strings"

	tea "github.com/charmbracelet/bubbletea"
)

func (m model) Init() tea.Cmd { return nil }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
	case tea.KeyMsg:
		switch m.mode {
		case "layout":
			return m.updateLayout(msg)
		case "rename":
			return m.updateRename(msg)
		case "filter":
			return m.updateFilter(msg)
		default:
			return m.updateLabel(msg)
		}
	}
	return m, nil
}

// actOn performs a row's primary action: jump (open) · move (newtab/newwin) · pick-a-layout
// (relay/project). Used by both a label tap and enter.
func (m model) actOn(idx int) (model, tea.Cmd) {
	m.cur = clamp(idx, len(m.view))
	it, ok := m.sel()
	if !ok {
		return m, nil
	}
	switch it.kind {
	case "open":
		_ = focusWindow(it.winID)
		return m, tea.Quit
	case "newtab":
		if m.source > 0 {
			_ = moveToNewTab(m.source)
		}
		return m, tea.Quit
	case "newwin":
		if m.source > 0 {
			_ = moveToNewOSWindow(m.source)
		}
		return m, tea.Quit
	default: // relay / project → pick a layout for that dir
		m.mode, m.layDir, m.layCur = "layout", it.dir, 0
		m.layouts = paletteNames()
		return m.refreshPreview(), nil
	}
}

// rowAction handles the per-row action keys (move / kill / rename / prune), shared by the
// label and filter modes. handled=true means the key was an action key (consumed).
func (m model) rowAction(msg tea.KeyMsg) (model, tea.Cmd, bool) {
	switch msg.Type {
	case tea.KeyCtrlS: // move the source pane into the highlighted open tab
		if m.source > 0 {
			if it, ok := m.sel(); ok && it.kind == "open" {
				_ = moveToTab(m.source, it.tabID)
				return m, tea.Quit, true
			}
		}
		return m, nil, true
	case tea.KeyCtrlX: // close the highlighted open tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			_ = closeTab(it.tabID)
			return m.reload(), nil, true
		}
		return m, nil, true
	case tea.KeyCtrlD: // prune a project from zoxide
		if it, ok := m.sel(); ok && it.kind == "project" {
			_ = zoxideRemove(it.dir)
			return m.reload(), nil, true
		}
		return m, nil, true
	case tea.KeyCtrlR: // rename the highlighted open tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			m.mode, m.rtab, m.rinput = "rename", it.tabID, it.title
			return m, nil, true
		}
		return m, nil, true
	}
	return m, nil, false
}

// updateLabel (default mode): tap a label key to jump · arrows + enter for the cursor ·
// "/" to search · ctrl-s/x/r/d row actions.
func (m model) updateLabel(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if nm, cmd, ok := m.rowAction(msg); ok {
		return nm, cmd
	}
	switch msg.Type {
	case tea.KeyCtrlC, tea.KeyEsc:
		return m, tea.Quit
	case tea.KeyEnter:
		return m.actOn(m.cur)
	case tea.KeyUp, tea.KeyCtrlP:
		m.cur = clamp(m.cur-1, len(m.view))
		return m.refreshPreview(), nil
	case tea.KeyDown, tea.KeyCtrlN:
		m.cur = clamp(m.cur+1, len(m.view))
		return m.refreshPreview(), nil
	case tea.KeyRunes:
		if len(msg.Runes) == 1 {
			r := msg.Runes[0]
			if r == '/' { // drop into search
				m.mode, m.query = "filter", ""
				return m.applyFilter().refreshPreview(), nil
			}
			if idx := strings.IndexRune(labelKeys, r); idx >= 0 && idx < len(m.view) {
				return m.actOn(idx)
			}
		}
	}
	return m, nil
}

// updateFilter: type to narrow, arrows + enter to act, esc back to labels.
func (m model) updateFilter(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if nm, cmd, ok := m.rowAction(msg); ok {
		return nm, cmd
	}
	switch msg.Type {
	case tea.KeyCtrlC:
		return m, tea.Quit
	case tea.KeyEsc:
		m.mode, m.query = "", ""
		return m.applyFilter().refreshPreview(), nil
	case tea.KeyEnter:
		return m.actOn(m.cur)
	case tea.KeyUp, tea.KeyCtrlP:
		m.cur = clamp(m.cur-1, len(m.view))
		return m.refreshPreview(), nil
	case tea.KeyDown, tea.KeyCtrlN:
		m.cur = clamp(m.cur+1, len(m.view))
		return m.refreshPreview(), nil
	case tea.KeyBackspace:
		if r := []rune(m.query); len(r) > 0 {
			m.query = string(r[:len(r)-1])
		}
		return m.applyFilter().refreshPreview(), nil
	case tea.KeyCtrlU:
		m.query = ""
		return m.applyFilter().refreshPreview(), nil
	case tea.KeySpace:
		m.query += " "
		return m.applyFilter().refreshPreview(), nil
	case tea.KeyRunes:
		m.query += string(msg.Runes)
		return m.applyFilter().refreshPreview(), nil
	}
	return m, nil
}

func (m model) updateLayout(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.Type {
	case tea.KeyCtrlC:
		return m, tea.Quit
	case tea.KeyEsc:
		m.mode = ""
		return m.refreshPreview(), nil
	case tea.KeyEnter:
		if m.layCur >= 0 && m.layCur < len(m.layouts) {
			_ = paletteBuild(m.layouts[m.layCur], m.layDir)
			return m, tea.Quit
		}
	case tea.KeyUp, tea.KeyCtrlP:
		m.layCur = clamp(m.layCur-1, len(m.layouts))
		return m.refreshPreview(), nil
	case tea.KeyDown, tea.KeyCtrlN:
		m.layCur = clamp(m.layCur+1, len(m.layouts))
		return m.refreshPreview(), nil
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
