package main

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

var (
	promptSt   = lipgloss.NewStyle().Foreground(lipgloss.Color("212")).Bold(true)
	selSt      = lipgloss.NewStyle().Foreground(lipgloss.Color("212")).Bold(true)
	labelStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("220")).Bold(true)
	dim        = lipgloss.NewStyle().Foreground(lipgloss.Color("240"))
	dirSt      = lipgloss.NewStyle().Foreground(lipgloss.Color("180"))
	openSt     = lipgloss.NewStyle().Foreground(lipgloss.Color("117"))
	relaySt    = lipgloss.NewStyle().Foreground(lipgloss.Color("183"))
	focG       = lipgloss.NewStyle().Foreground(lipgloss.Color("120"))
	runG       = lipgloss.NewStyle().Foreground(lipgloss.Color("214"))
	failG      = lipgloss.NewStyle().Foreground(lipgloss.Color("203"))
	errSt      = lipgloss.NewStyle().Foreground(lipgloss.Color("203"))
	statusSt   = lipgloss.NewStyle().Foreground(lipgloss.Color("212"))
	borderC    = lipgloss.Color("238")
)

func trunc(s string, n int) string {
	r := []rune(s)
	if n < 1 {
		n = 1
	}
	if len(r) <= n {
		return s
	}
	return string(r[:n-1]) + "…"
}

func glyph(status string) string {
	switch status {
	case "focused":
		return focG.Render("●")
	case "running":
		return runG.Render("⏵")
	case "failed":
		return failG.Render("✗")
	default:
		return openSt.Render("○") // open but idle at the prompt
	}
}

// windowRange returns the [start,end) slice of n items to show in h rows, centring cur.
func windowRange(cur, n, h int) (int, int) {
	if h >= n {
		return 0, n
	}
	start := max(0, min(cur-h/2, n-h))
	return start, start + h
}

func (m model) leftRow(viewIdx, leftW int, selected bool) string {
	it := m.all[m.view[viewIdx]]
	avail := leftW - 4
	// lead: the jump label (label mode) or a cursor marker (filter mode)
	lead := "  "
	if m.mode == "" {
		if k := labelFor(viewIdx); k != "" {
			lead = labelStyle.Render(k) + " "
		}
	} else if selected {
		lead = selSt.Render("▸ ")
	}
	var g, name string
	nameSt := dirSt
	switch it.kind {
	case "relay":
		g, name, nameSt = relaySt.Render("↻"), "relayout · "+filepath.Base(it.dir), relaySt
	case "open":
		g = glyph(it.status)
		name = filepath.Base(it.dir)
		if name == "" || name == "/" || name == "." {
			name = it.title
		}
		nameSt = openSt
	case "newtab", "newwin": // move-the-source-pane targets
		g, name, nameSt = relaySt.Render("+"), it.title, relaySt
	default: // project — "+" = open a new tab here (vs ○/● = jump to an open one)
		g, name = dim.Render("+"), filepath.Base(it.dir)
	}
	if selected {
		nameSt = selSt
	}
	return lead + g + " " + nameSt.Render(trunc(name, avail))
}

func (m model) rightContent(rightW, bodyH int) string {
	lines := strings.Split(m.preview, "\n")
	if len(lines) > bodyH {
		lines = lines[:bodyH]
	}
	for i, l := range lines {
		if !strings.Contains(l, "\x1b") { // plain text → safe to truncate by rune width
			lines[i] = trunc(l, rightW)
		}
	}
	return strings.Join(lines, "\n")
}

func (m model) View() string {
	if m.err != "" {
		return errSt.Render("  " + m.err)
	}
	w, h := m.w, m.h
	if w <= 0 {
		w = 100
	}
	if h <= 0 {
		h = 30
	}
	leftW := max(30, min(w*2/5, 52))
	rightW := max(10, w-leftW-4)
	bodyH := max(3, h-3)

	// prompt + footer per mode
	var prompt, footer string
	switch m.mode {
	case "layout":
		prompt = promptSt.Render("layout for " + filepath.Base(m.layDir) + " ❯")
		footer = dim.Render("  ↑↓ pick · enter build · esc back")
	case "rename":
		prompt = promptSt.Render("rename tab ❯ ") + m.rinput + selSt.Render("▌")
		footer = dim.Render("  enter save · esc cancel")
	case "filter":
		prompt = promptSt.Render("❯ ") + m.query + selSt.Render("▌") +
			dim.Render(fmt.Sprintf("   %d", len(m.view)))
		footer = dim.Render("  ↵ go · ^s move · ^x kill · ^r rename · ^d prune · esc")
	default: // label mode
		prompt = promptSt.Render("prowl") + dim.Render("   tap a key to jump   ·   / search")
		if m.status != "" {
			prompt += dim.Render("   ") + statusSt.Render(m.status)
		}
		footer = dim.Render("  key/↵ go · ^s move · ^x kill · ^r rename · ^d prune · esc")
	}

	// left column
	var rows []string
	if m.mode == "layout" {
		start, end := windowRange(m.layCur, len(m.layouts), bodyH)
		for i := start; i < end; i++ {
			c, st := "  ", dim
			if i == m.layCur {
				c, st = selSt.Render("▸ "), selSt
			}
			rows = append(rows, c+st.Render(m.layouts[i]))
		}
	} else {
		start, end := windowRange(m.cur, len(m.view), bodyH)
		for i := start; i < end; i++ {
			rows = append(rows, m.leftRow(i, leftW, i == m.cur))
		}
	}

	leftBox := lipgloss.NewStyle().Width(leftW).Height(bodyH).
		Border(lipgloss.NormalBorder(), false, true, false, false).BorderForeground(borderC).
		Render(strings.Join(rows, "\n"))
	rightBox := lipgloss.NewStyle().Width(rightW).Height(bodyH).PaddingLeft(1).
		Render(m.rightContent(rightW, bodyH))
	body := lipgloss.JoinHorizontal(lipgloss.Top, leftBox, rightBox)
	return prompt + "\n" + body + "\n" + footer
}
