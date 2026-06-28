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
	promptSt  = fg("#cba6f7").Bold(true) // mauve
	selSt     = fg("#f5c2e7").Bold(true) // pink
	dim       = fg("#6c7086")            // overlay0
	metaSt    = fg("#7f849c")            // overlay1 — inline cmd/git
	dirSt     = fg("#a6adc8")            // subtext0 — project names
	openSt    = fg("#89b4fa")            // blue — open tab names
	focG      = fg("#a6e3a1")            // green
	runG      = fg("#fab387")            // peach
	failG     = fg("#f38ba8")            // red
	errSt     = fg("#f38ba8")
	statusSt  = fg("#f5c2e7")
	barStyle  = lipgloss.NewStyle().Background(lipgloss.Color("#45475a")).Foreground(lipgloss.Color("#cdd6f4"))
	borderC   = lipgloss.Color("#585b70")
	ruleSt    = lipgloss.NewStyle().Foreground(borderC)
	lavender  = fg("#b4befe")
	sectionSt = lavender.Bold(true) // right-pane section headers
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

// wrapLines word-wraps s to width w and returns the flattened display lines. ANSI-bearing
// lines pass through untouched (wrapping them would split escape sequences); plain prose is
// wrapped on spaces, with overlong words hard-broken. Used for the agent answer so long
// paragraphs read in full instead of being truncated with an ellipsis.
func wrapLines(s string, w int) []string {
	if w < 1 {
		w = 1
	}
	var out []string
	for _, line := range strings.Split(s, "\n") {
		switch {
		case line == "":
			out = append(out, "")
		case strings.Contains(line, "\x1b"):
			out = append(out, line)
		default:
			out = append(out, wrapPlain(line, w)...)
		}
	}
	return out
}

func wrapPlain(line string, w int) []string {
	var out []string
	cur := make([]rune, 0, w)
	hardBreak := func(wr []rune) []rune { // emit full-width chunks of an overlong word
		for len(wr) > w {
			out = append(out, string(wr[:w]))
			wr = wr[w:]
		}
		return wr
	}
	for _, word := range strings.Split(line, " ") {
		wr := []rune(word)
		switch {
		case len(cur) == 0:
			cur = append(cur, hardBreak(wr)...)
		case len(cur)+1+len(wr) <= w:
			cur = append(append(cur, ' '), wr...)
		default:
			out = append(out, string(cur))
			cur = append(cur[:0], hardBreak(wr)...)
		}
	}
	return append(out, string(cur))
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

// navActions builds the footer hints for the current selection + launch context, so a key
// only shows when it actually does something here.
func (m model) navActions() string {
	var a []string
	it, ok := m.sel()
	switch {
	case ok && it.kind == "open":
		a = append(a, "↵ jump", "x close", "r rename")
	case ok: // project
		a = append(a, "↵ open")
	}
	a = append(a, "m move")
	if agentHook != "" {
		a = append(a, "a ask")
	}
	if m.cwd != "" {
		a = append(a, ". relayout")
	}
	a = append(a, "/ search", "q quit")
	return strings.Join(a, " · ")
}

// agentDims sizes the floating panel: content width and reply body height. Shared by the
// renderer and the scroller (updateAgent) so half-page/page jumps match what's on screen.
func agentDims(w, h int) (innerW, bodyH int) {
	if w <= 0 {
		w = 100
	}
	if h <= 0 {
		h = 30
	}
	pw := max(40, min(w-8, 90))
	ph := max(8, min(h-6, 24))
	return pw - 4, max(1, ph-6)
}

// agentPanel renders the floating `a` agent panel — a centered box (solid backdrop): the
// question line (a live cursor while typing, dimmed while reading), the reply (scrollable in
// READ focus, with a position indicator), and a focus-aware footer.
func (m model) agentPanel() string {
	w, h := m.w, m.h
	if w <= 0 {
		w = 100
	}
	if h <= 0 {
		h = 30
	}
	innerW, bodyH := agentDims(w, h)
	reading := m.agentFocus == "read"

	header := promptSt.Render("🤖 " + trunc(m.agentName, innerW-3))

	qText := trunc(m.agentInput, innerW-3)
	input := promptSt.Render("❯ ") + qText + selSt.Render("▌")
	if reading { // de-emphasise the question; the cursor lives in the answer now
		input = dim.Render("❯ " + qText)
	}
	rule := ruleSt.Render(strings.Repeat("─", innerW))

	body := make([]string, bodyH)
	total, off := 0, 0
	switch {
	case m.agentWorking:
		body[0] = runG.Render("🤖 working…")
	case m.agentResult == "":
		body[0] = dim.Render("type a question, then enter")
	default:
		src := wrapLines(m.agentResult, innerW) // wrap, don't truncate — read the whole answer
		total = len(src)
		off = max(0, min(m.agentOff, max(0, total-bodyH)))
		for i := range bodyH {
			if off+i < total {
				body[i] = src[off+i]
			}
		}
	}

	// focus-aware footer + scroll position (built plain, dimmed once, so trunc stays safe)
	var f string
	switch {
	case reading:
		f = "j/k scroll · ^d/^u half · g/G ends · i ask · esc"
	case m.agentResult != "":
		f = "enter ask · tab read · esc close"
	default:
		f = "enter ask · esc close"
	}
	if total > bodyH {
		f += fmt.Sprintf("   %d–%d/%d", off+1, min(off+bodyH, total), total)
	}
	footer := dim.Render(trunc(f, innerW))

	content := header + "\n" + input + "\n" + rule + "\n" + strings.Join(body, "\n") + "\n" + footer
	box := lipgloss.NewStyle().Width(innerW).Padding(0, 1).
		Border(lipgloss.RoundedBorder()).BorderForeground(borderC).Render(content)
	return lipgloss.Place(w, h, lipgloss.Center, lipgloss.Center, box)
}

// windowRange returns the [start,end) slice of n items to show in h rows, centring cur.
func windowRange(cur, n, h int) (int, int) {
	if h >= n {
		return 0, n
	}
	start := max(0, min(cur-h/2, n-h))
	return start, start + h
}

// layoutRow is a self-describing picker row: the layout name (a fixed column) + its dim
// caption, e.g. "dev    editor · shell · lazygit". Selected = the highlight bar.
func (m model) layoutRow(i, leftW int, selected bool) string {
	const nameW = 7 // widest layout name is ~6 chars; pad to align the captions
	name := m.layouts[i]
	nm := trunc(name, nameW)
	nm += strings.Repeat(" ", max(0, nameW-len([]rune(nm))))
	desc := trunc(m.layoutDefs[name].caption, max(1, leftW-4-nameW)) // after "  " + name + " "

	if selected { // single-style highlight bar (= the cursor)
		return barStyle.Width(leftW).Render("  " + nm + " " + desc)
	}
	return "  " + promptSt.Render(nm) + " " + dim.Render(desc)
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

// rightContent composes the right pane as clear sections: the ambient `a` agent teaser
// (the question + at most 10 reply lines — the full answer is read in the `a` panel), then
// the dir's REPO + FILES (built in dirPreview). Returns exactly bodyH lines (lipgloss.Height
// only pads, never truncates — so we cap here or a long listing overruns the frame).
func (m model) rightContent(rightW, bodyH int) string {
	if m.mode == "layout" { // the selected layout's live sketch, sized to the pane
		if m.layCur >= 0 && m.layCur < len(m.layouts) {
			if L, ok := m.layoutDefs[m.layouts[m.layCur]]; ok {
				return layoutSketch(L, rightW, bodyH)
			}
		}
		return ""
	}
	var secs []string
	if it, ok := m.sel(); ok && it.dir != "" {
		switch {
		case m.workingDirs[it.dir]:
			secs = append(secs, sectionHead("AGENT")+"\n"+runG.Render("🤖 working…"))
		default:
			if li := m.lastInstr[it.dir]; li != "" {
				if r := m.replyCache[it.dir+"\x00"+li]; r != "" {
					secs = append(secs, agentSection(li, r, rightW))
				}
			}
		}
	}
	if m.preview != "" {
		secs = append(secs, m.preview) // REPO + FILES sections (built in dirPreview)
	}
	return clampLines(strings.Join(secs, "\n\n"), rightW, bodyH)
}

// agentSection is the right-pane agent teaser: the question + at most 10 wrapped reply lines
// (wrapped, not truncated, so it reads as prose). The full, scrollable answer lives in the `a`
// panel (press `a` to read it).
func agentSection(question, reply string, w int) string {
	const maxReply = 10
	lines := wrapLines(strings.TrimRight(reply, "\n"), w)
	clipped := false
	if len(lines) > maxReply {
		lines, clipped = lines[:maxReply], true
	}
	body := strings.Join(lines, "\n")
	if clipped {
		body += "\n" + dim.Render("… press a for the full answer")
	}
	return sectionHead("AGENT") + "\n" + promptSt.Render(trunc("🤖 "+question, w)) + "\n" + body
}

// sectionHead renders a right-pane section label, e.g. "▌ REPO".
func sectionHead(title string) string { return sectionSt.Render("▌ " + title) }

// clampLines fits text into exactly h rows, truncating each plain (non-ANSI) line to w runes.
func clampLines(s string, w, h int) string {
	src := strings.Split(s, "\n")
	out := make([]string, h)
	for i := range h {
		if i < len(src) {
			l := src[i]
			if !strings.Contains(l, "\x1b") { // plain text → safe to truncate by rune width
				l = trunc(l, w)
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
	if m.mode == "agent" {
		return m.agentPanel()
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
	if m.mode == "layout" { // a slim list → the (wide) sketch gets the rest
		leftW = max(20, min(leftW, 34))
	}
	if leftW > innerW-14 { // always leave the preview some room
		leftW = max(10, innerW-14)
	}
	rightW := max(8, innerW-1-leftW)
	bodyH := max(3, h-6) // frame chrome; -6 (not -5) keeps it ≤ h despite a lipgloss border quirk

	// header (prompt) + footer (hints) per mode (layout/agent own the whole screen above)
	var prompt, footer string
	switch m.mode {
	case "layout":
		prompt = promptSt.Render("layout for "+filepath.Base(m.layDir)) + dim.Render("   ↵ build · esc back")
		footer = dim.Render("j/k pick · l/↵ build · h back")
	case "rename":
		prompt = promptSt.Render("rename tab ❯ ") + m.rinput + selSt.Render("▌")
		footer = dim.Render("enter save · esc cancel")
	case "move":
		if m.moveSrc == 0 { // stage A — pick the pane to move
			prompt = promptSt.Render("move which pane?")
			footer = dim.Render("j/k pick · ↵ choose this pane · esc cancel")
		} else { // stage B — pick a destination tab
			prompt = promptSt.Render("move " + m.moveSrcName + " → which tab?")
			footer = dim.Render("↵ move into this tab · esc back")
		}
	case "filter":
		prompt = promptSt.Render("❯ ") + m.query + selSt.Render("▌") +
			dim.Render(fmt.Sprintf("   %d match", len(m.view)))
		footer = dim.Render("↵ go · esc back to nav")
	default: // nav (vim)
		prompt = promptSt.Render("matou") + dim.Render("   j/k nav · / search")
		if m.status != "" {
			prompt += dim.Render("   ") + statusSt.Render(m.status)
		}
		footer = dim.Render(trunc(m.navActions(), innerW)) // context-aware; trunc so it never wraps the frame
	}

	// list column
	var rows []string
	if m.mode == "layout" {
		start, end := windowRange(m.layCur, len(m.layouts), bodyH)
		for i := start; i < end; i++ {
			rows = append(rows, m.layoutRow(i, leftW, i == m.layCur))
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
