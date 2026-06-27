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
		default:
			return m.updatePalette(msg)
		}
	}
	return m, nil
}

// updatePalette: type to filter; arrows / ctrl-n,p move; enter acts; ctrl-s/x/r/d are the
// per-row actions (move the source pane · close tab · rename tab · prune from zoxide).
func (m model) updatePalette(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.Type {
	case tea.KeyCtrlC, tea.KeyEsc:
		return m, tea.Quit
	case tea.KeyEnter:
		if it, ok := m.sel(); ok {
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
	case tea.KeyCtrlS: // move the source pane into the highlighted open tab
		if m.source > 0 {
			if it, ok := m.sel(); ok && it.kind == "open" {
				_ = moveToTab(m.source, it.tabID)
				return m, tea.Quit
			}
		}
	case tea.KeyCtrlX: // close the highlighted open tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			_ = closeTab(it.tabID)
			return m.reload(), nil
		}
	case tea.KeyCtrlD: // prune a project from zoxide
		if it, ok := m.sel(); ok && it.kind == "project" {
			_ = zoxideRemove(it.dir)
			return m.reload(), nil
		}
	case tea.KeyCtrlR: // rename the highlighted open tab
		if it, ok := m.sel(); ok && it.kind == "open" {
			m.mode, m.rtab, m.rinput = "rename", it.tabID, it.title
			return m, nil
		}
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
