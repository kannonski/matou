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
	focG     = fg("#a6e3a1")            // green
	runG     = fg("#fab387")            // peach
	failG    = fg("#f38ba8")            // red
	errSt    = fg("#f38ba8")
	statusSt = fg("#f5c2e7")
	barStyle = lipgloss.NewStyle().Background(lipgloss.Color("#45475a")).Foreground(lipgloss.Color("#cdd6f4"))
	borderC  = lipgloss.Color("#585b70")
	ruleSt   = lipgloss.NewStyle().Foreground(borderC)
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
	if it.kind != "open" {
		return "+" // project
	}
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

func glyphStyle(it item) lipgloss.Style {
	if it.kind != "open" {
		return dim // project
	}
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

func nameOf(it item) string {
	n := filepath.Base(it.dir)
	if it.kind == "open" && (n == "" || n == "/" || n == ".") {
		n = it.title
	}
	return n
}

func nameStyle(it item) lipgloss.Style {
	if it.kind == "open" {
		return openSt
	}
	return dirSt
}

// meta is the compact trailing context for an open row: the running command + a dirty
// count, e.g. "nvim *3" or "zsh". The (often long) branch name lives in the preview.
func (it item) meta() string {
	if it.kind != "open" {
		return ""
	}
	s := it.proc
	if it.changes > 0 {
		if s != "" {
			s += " "
		}
		s += "*" + strconv.Itoa(it.changes)
	}
	return s
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

	zone := leftW - 4 // the name … meta area, after "  " + glyph + " "
	mlen := len([]rune(meta))
	if mlen > 0 && zone-mlen-1 < 6 { // not enough room for both → drop the meta
		meta, mlen = "", 0
	}
	nameMax := zone
	if mlen > 0 {
		nameMax = zone - mlen - 1
	}
	name = trunc(name, nameMax)
	pad := max(0, zone-len([]rune(name))-mlen) // right-align the meta

	if selected { // single-style highlight bar (= the cursor)
		return barStyle.Width(leftW).Render("  " + gr + " " + name + strings.Repeat(" ", pad) + meta)
	}
	out := "  " + glyphStyle(it).Render(gr) + " " + nameStyle(it).Render(name) + strings.Repeat(" ", pad)
	if meta != "" {
		out += metaSt.Render(meta)
	}
	return out
}

// rightContent returns exactly bodyH preview lines (lipgloss.Height only pads, never
// truncates — so we must cap here or a long listing overruns the frame).
func (m model) rightContent(rightW, bodyH int) string {
	src := strings.Split(m.preview, "\n")
	out := make([]string, bodyH)
	for i := range bodyH {
		if i < len(src) {
			l := src[i]
			if !strings.Contains(l, "\x1b") { // plain text → safe to truncate by rune width
				l = trunc(l, rightW)
			}
			out[i] = l
		}
	}
	return strings.Join(out, "\n")
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
	innerW := max(20, w-4) // inside the rounded frame: border(2) + padding(2)
	leftW := max(24, min((innerW-1)*2/5, 52))
	if leftW > innerW-14 { // always leave the preview some room
		leftW = max(10, innerW-14)
	}
	rightW := max(8, innerW-1-leftW)
	bodyH := max(3, h-6) // frame chrome; -6 (not -5) keeps it ≤ h despite a lipgloss border quirk

	// header (prompt) + footer (hints) per mode
	var prompt, footer string
	switch m.mode {
	case "layout":
		prompt = promptSt.Render("layout for "+filepath.Base(m.layDir)) + dim.Render("   ↵ build · esc back")
		footer = dim.Render("j/k pick · l/↵ build · h back")
	case "rename":
		prompt = promptSt.Render("rename tab ❯ ") + m.rinput + selSt.Render("▌")
		footer = dim.Render("enter save · esc cancel")
	case "filter":
		prompt = promptSt.Render("❯ ") + m.query + selSt.Render("▌") +
			dim.Render(fmt.Sprintf("   %d match", len(m.view)))
		footer = dim.Render("↵ go · ^s move · ^x kill · ^r rename · ^d prune · esc")
	default: // nav (vim)
		prompt = promptSt.Render("prowl") + dim.Render("   j/k nav · l open · / search")
		if m.status != "" {
			prompt += dim.Render("   ") + statusSt.Render(m.status)
		}
		footer = dim.Render("l open · . relayout · m move (M new tab · W new win) · x close · r rename · q quit")
	}

	// list column
	var rows []string
	if m.mode == "layout" {
		start, end := windowRange(m.layCur, len(m.layouts), bodyH)
		for i := start; i < end; i++ {
			if i == m.layCur {
				rows = append(rows, barStyle.Width(leftW).Render("  "+m.layouts[i]))
			} else {
				rows = append(rows, "  "+dim.Render(m.layouts[i]))
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
	twoPane := lipgloss.JoinHorizontal(lipgloss.Top, leftBox, rightBox)

	rule := ruleSt.Render(strings.Repeat("─", innerW))
	inner := prompt + "\n" + rule + "\n" + twoPane + "\n" + footer
	return lipgloss.NewStyle().Padding(0, 1).
		Border(lipgloss.RoundedBorder()).BorderForeground(borderC).
		Render(inner)
}
