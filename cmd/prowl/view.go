package main

import (
	"fmt"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

// Catppuccin Mocha
func fg(hex string) lipgloss.Style { return lipgloss.NewStyle().Foreground(lipgloss.Color(hex)) }

var (
	promptSt = fg("#cba6f7").Bold(true) // mauve
	selSt    = fg("#f5c2e7").Bold(true) // pink
	dim      = fg("#6c7086")            // overlay0
	metaSt   = fg("#7f849c")            // overlay1 — inline cmd/git
	dirSt    = fg("#a6adc8")            // subtext0 — project names
	openSt   = fg("#89b4fa")            // blue — open tab names
	relaySt  = fg("#cba6f7")            // mauve — relay / move targets
	focG     = fg("#a6e3a1")            // green
	runG     = fg("#fab387")            // peach
	failG    = fg("#f38ba8")            // red
	errSt    = fg("#f38ba8")
	statusSt = fg("#f5c2e7")
	barStyle = lipgloss.NewStyle().Background(lipgloss.Color("#45475a")).Foreground(lipgloss.Color("#cdd6f4"))
	borderC  = lipgloss.Color("#585b70")
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

func glyphRune(it item) string {
	switch it.kind {
	case "relay":
		return "↻"
	case "newtab", "newwin", "project":
		return "+"
	case "open":
		switch it.status {
		case "focused":
			return "●"
		case "running":
			return "⏵"
		case "failed":
			return "✗"
		default:
			return "○"
		}
	}
	return " "
}

func glyphStyle(it item) lipgloss.Style {
	switch it.kind {
	case "relay", "newtab", "newwin":
		return relaySt
	case "open":
		switch it.status {
		case "focused":
			return focG
		case "running":
			return runG
		case "failed":
			return failG
		default:
			return openSt
		}
	}
	return dim
}

func nameOf(it item) string {
	switch it.kind {
	case "relay":
		return "relayout · " + filepath.Base(it.dir)
	case "newtab", "newwin":
		return it.title
	case "open":
		n := filepath.Base(it.dir)
		if n == "" || n == "/" || n == "." {
			n = it.title
		}
		return n
	default:
		return filepath.Base(it.dir)
	}
}

func nameStyle(it item) lipgloss.Style {
	switch it.kind {
	case "relay", "newtab", "newwin":
		return relaySt
	case "open":
		return openSt
	default:
		return dirSt
	}
}

// meta is the dim trailing context for an open row: "nvim  main *3".
func (it item) meta() string {
	if it.kind != "open" {
		return ""
	}
	var parts []string
	if it.proc != "" {
		parts = append(parts, it.proc)
	}
	if it.branch != "" {
		b := it.branch
		if it.changes > 0 {
			b += " *" + strconv.Itoa(it.changes)
		}
		parts = append(parts, b)
	}
	return strings.Join(parts, "  ")
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
	gr := glyphRune(it)
	name := nameOf(it)
	meta := it.meta()

	avail := leftW - 4 // lead(2) + glyph(1) + space(1)
	metaW := 0
	if meta != "" {
		metaW = len([]rune(meta)) + 2 // "  " + meta
	}
	nameMax := avail - metaW
	if nameMax < 6 { // too tight — drop meta, give the name the room
		nameMax, meta, metaW = avail, "", 0
	}
	name = trunc(name, nameMax)

	if selected { // single-style highlight bar (no per-segment ANSI to break the bg) = the cursor
		line := "  " + gr + " " + name
		if meta != "" {
			line += "  " + meta
		}
		return barStyle.Width(leftW).Render(line)
	}

	out := "  " + glyphStyle(it).Render(gr) + " " + nameStyle(it).Render(name)
	if meta != "" {
		out += "  " + metaSt.Render(meta)
	}
	return out
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
	leftW := max(30, min(w*2/5, 54))
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
	default: // nav (vim)
		prompt = promptSt.Render("prowl") + dim.Render("   j/k move · l open · / search · q quit")
		if m.status != "" {
			prompt += dim.Render("   ") + statusSt.Render(m.status)
		}
		footer = dim.Render("  l/↵ open · h back · ^s move · ^x kill · ^r rename · ^d prune")
	}

	// left column
	var rows []string
	if m.mode == "layout" {
		start, end := windowRange(m.layCur, len(m.layouts), bodyH)
		for i := start; i < end; i++ {
			if i == m.layCur {
				rows = append(rows, barStyle.Width(leftW).Render("  "+m.layouts[i]))
			} else {
				rows = append(rows, dim.Render("  "+m.layouts[i]))
			}
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
