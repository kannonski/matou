package main

import (
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// palette.go — the layout engine, ported from the old palette.py (now pure Go, no python3).
// Layouts stay defined in palette.layouts (a small TOML file), the editable single source of
// truth; this file parses it, draws the preview sketch, and launches the panes via `kitty @`.
// Shapes: single · columns · rows · main+right · main+bottom · main+rightstack · main+bottomrow.

type layout struct {
	name    string
	shape   string
	panes   []string  // commands in order; the first is the editor you land in
	ratio   []float64 // size %, shape-specific (leading panes; the rest share the remainder)
	caption string
	order   int
}

func layoutsPath() string {
	if p := os.Getenv("PROWL_LAYOUTS"); p != "" {
		return p
	}
	if p := os.Getenv("KITTY_PALETTE_LAYOUTS"); p != "" {
		return p
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "kitty", "palette.layouts")
}

// loadLayouts parses palette.layouts (a small TOML subset: [name] sections with string / int /
// array values) into layouts ordered by (order, name). nil if the file is missing/unreadable.
func loadLayouts() []layout {
	data, err := os.ReadFile(layoutsPath())
	if err != nil {
		return nil
	}
	var ls []layout
	cur := -1
	for _, raw := range strings.Split(string(data), "\n") {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			ls = append(ls, layout{name: strings.TrimSpace(line[1 : len(line)-1]), shape: "single", order: 999})
			cur = len(ls) - 1
			continue
		}
		if cur < 0 {
			continue
		}
		key, val, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key, val = strings.TrimSpace(key), strings.TrimSpace(val)
		switch key {
		case "shape":
			ls[cur].shape = tomlStr(val)
		case "caption":
			ls[cur].caption = tomlStr(val)
		case "order":
			ls[cur].order = tomlInt(val)
		case "panes":
			ls[cur].panes = tomlStrArray(val)
		case "ratio":
			ls[cur].ratio = tomlFloatArray(val)
		}
	}
	for i := range ls {
		if len(ls[i].panes) == 0 {
			ls[i].panes = []string{"zsh"}
		}
		if len(ls[i].panes) == 1 {
			ls[i].shape = "single"
		}
	}
	sort.SliceStable(ls, func(i, j int) bool {
		if ls[i].order != ls[j].order {
			return ls[i].order < ls[j].order
		}
		return ls[i].name < ls[j].name
	})
	return ls
}

func tomlStr(v string) string { return strings.Trim(strings.TrimSpace(v), `"`) }

func tomlInt(v string) int { n, _ := strconv.Atoi(strings.TrimSpace(v)); return n }

func tomlArrayElems(v string) []string {
	v = strings.TrimSpace(v)
	v = strings.TrimSuffix(strings.TrimPrefix(v, "["), "]")
	var out []string
	for _, e := range strings.Split(v, ",") {
		if e = strings.TrimSpace(e); e != "" {
			out = append(out, e)
		}
	}
	return out
}

func tomlStrArray(v string) []string {
	var out []string
	for _, e := range tomlArrayElems(v) {
		out = append(out, tomlStr(e))
	}
	return out
}

func tomlFloatArray(v string) []float64 {
	var out []float64
	for _, e := range tomlArrayElems(v) {
		if f, err := strconv.ParseFloat(strings.TrimSpace(e), 64); err == nil {
			out = append(out, f)
		}
	}
	return out
}

// ── appearance (sketch only) — Catppuccin Mocha ──
type rgb [3]int

var (
	cGreen    = rgb{166, 227, 161}
	cBlue     = rgb{137, 180, 250}
	cMauve    = rgb{203, 166, 247}
	cTeal     = rgb{148, 226, 213}
	cPeach    = rgb{250, 179, 135}
	cRed      = rgb{243, 139, 168}
	cSapphire = rgb{116, 199, 236}
	cSurface0 = rgb{49, 50, 68}
	cSurface1 = rgb{69, 71, 90}
	cOverlay0 = rgb{108, 112, 134}
	cText     = rgb{205, 214, 244}
	cSubtext  = rgb{166, 173, 200}
	cBorder   = cOverlay0
)

// tool maps a pane command's first word to a nerd-font icon, a body sketch kind, and an accent.
type tool struct {
	icon   string
	kind   string // nvim | sh | git | k9s | logs
	accent rgb
}

var toolMap = map[string]tool{
	"claude":     {"\U000f1a90", "sh", cPeach},
	"nvim":       {"\U0000e6ae", "nvim", cGreen},
	"vim":        {"\U0000e6ae", "nvim", cGreen},
	"vi":         {"\U0000e6ae", "nvim", cGreen},
	"zsh":        {"\U0000e795", "sh", cBlue},
	"bash":       {"\U0000e795", "sh", cBlue},
	"fish":       {"\U0000e795", "sh", cBlue},
	"sh":         {"\U0000e795", "sh", cBlue},
	"lazygit":    {"\U0000e702", "git", cMauve},
	"gitui":      {"\U0000e702", "git", cMauve},
	"git":        {"\U0000e702", "git", cMauve},
	"k9s":        {"\U000f10fe", "k9s", cTeal},
	"kubectl":    {"\U000f10fe", "k9s", cTeal},
	"lazydocker": {"\U0000f308", "logs", cSapphire},
	"docker":     {"\U0000f308", "logs", cSapphire},
	"stern":      {"\U0000f0f6", "logs", cPeach},
	"tail":       {"\U0000f0f6", "logs", cPeach},
	"btop":       {"\U0000f0f6", "logs", cPeach},
	"htop":       {"\U0000f0f6", "logs", cPeach},
	"go":         {"\U0000e627", "sh", cTeal},
	"npm":        {"\U0000e71e", "sh", cRed},
}

var defaultTool = tool{"\U0000e795", "sh", cBlue}

// labelOf is the basename of a command's first word (the tool name), "sh" if empty.
func labelOf(cmd string) string {
	toks := shlexSplit(cmd)
	if len(toks) == 0 || toks[0] == "" {
		return "sh"
	}
	base := toks[0]
	if i := strings.LastIndex(base, "/"); i >= 0 {
		base = base[i+1:]
	}
	if base == "" {
		return "sh"
	}
	return base
}

func appear(cmd string) tool {
	if t, ok := toolMap[labelOf(cmd)]; ok {
		return t
	}
	return defaultTool
}

// shlexSplit splits a command line on whitespace, honoring single/double quotes. Enough for
// the layout `panes` commands ("nvim {dir}", "npm run dev", "claude --resume").
func shlexSplit(s string) []string {
	var out []string
	var cur strings.Builder
	var quote rune
	inWord := false
	flush := func() {
		if inWord {
			out = append(out, cur.String())
			cur.Reset()
			inWord = false
		}
	}
	for _, r := range s {
		switch {
		case quote != 0:
			if r == quote {
				quote = 0
			} else {
				cur.WriteRune(r)
			}
			inWord = true
		case r == '\'' || r == '"':
			quote = r
			inWord = true
		case r == ' ' || r == '\t':
			flush()
		default:
			cur.WriteRune(r)
			inWord = true
		}
	}
	flush()
	return out
}

// norm returns sizes for n panes: leading values from sizes, the rest split equally.
func norm(sizes []float64, n int) []float64 {
	if n < 0 {
		n = 0
	}
	var ws []float64
	for _, x := range sizes {
		if len(ws) >= n {
			break
		}
		ws = append(ws, x)
	}
	if len(ws) < n {
		sum := 0.0
		for _, w := range ws {
			sum += w
		}
		rem := math.Max(0, 100-sum)
		k := n - len(ws)
		for range k {
			ws = append(ws, rem/float64(k))
		}
	}
	if len(ws) == 0 {
		return []float64{100}
	}
	return ws
}

// ── build (launch the real panes via kitty @) ──
func kittyOut(args ...string) string {
	out, _ := exec.Command("kitty", append([]string{"@"}, args...)...).Output()
	return strings.TrimSpace(string(out))
}

func argvFor(cmd, dir string) []string { return shlexSplit(strings.ReplaceAll(cmd, "{dir}", dir)) }

func launchTab(cmd, title, dir string) string {
	args := append([]string{"launch", "--type=tab", "--tab-title", title, "--cwd", dir, "--"}, argvFor(cmd, dir)...)
	return kittyOut(args...)
}

func launchSplit(loc string, bias float64, nextTo, cmd, dir string) string {
	args := []string{"launch", "--location=" + loc, "--bias=" + strconv.Itoa(int(math.Round(bias)))}
	if nextTo != "" {
		args = append(args, "--next-to", "id:"+nextTo)
	}
	args = append(args, "--cwd", dir, "--")
	args = append(args, argvFor(cmd, dir)...)
	return kittyOut(args...)
}

// chain lays cmds[1:] along one axis next to cmds[0] (already launched as window `first`).
func chain(first string, cmds []string, sizes []float64, loc, dir string) {
	ws := norm(sizes, len(cmds))
	prev := first
	for i := 1; i < len(cmds); i++ {
		var num, den float64
		for _, w := range ws[i:] {
			num += w
		}
		for _, w := range ws[i-1:] {
			den += w
		}
		bias := 0.0
		if den != 0 {
			bias = num / den * 100
		}
		prev = launchSplit(loc, bias, prev, cmds[i], dir)
	}
}

func mainRatio(L layout) float64 {
	if len(L.ratio) > 0 {
		return L.ratio[0]
	}
	return 60
}

func restRatio(L layout) []float64 {
	if len(L.ratio) > 1 {
		return L.ratio[1:]
	}
	return nil
}

// layoutBuild launches the layout's panes in a new tab cwd'd to dir, then focuses the editor.
func layoutBuild(L layout, dir string) {
	title := filepath.Base(strings.TrimRight(dir, "/"))
	if title == "" {
		title = dir
	}
	ed := launchTab(L.panes[0], title, dir)
	switch L.shape {
	case "single":
	case "columns", "main+right":
		chain(ed, L.panes, L.ratio, "vsplit", dir)
	case "rows", "main+bottom":
		chain(ed, L.panes, L.ratio, "hsplit", dir)
	case "main+rightstack":
		right := launchSplit("vsplit", 100-mainRatio(L), ed, L.panes[1], dir)
		chain(right, L.panes[1:], restRatio(L), "hsplit", dir)
	case "main+bottomrow":
		bot := launchSplit("hsplit", 100-mainRatio(L), ed, L.panes[1], dir)
		chain(bot, L.panes[1:], restRatio(L), "vsplit", dir)
	}
	if ed != "" {
		_ = exec.Command("kitty", "@", "focus-window", "--match", "id:"+ed).Run()
	}
	_ = exec.Command("zoxide", "add", dir).Run()
}

// ── sketch (a colored ASCII mock-up that mirrors the same spec) ──
type rect struct{ c0, r0, c1, r1 int }

type cell struct {
	ch     string
	fg, bg *rgb
	bold   bool
}

func clampi(v, lo, hi int) int {
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

// layoutSketch renders the layout's colored pane diagram sized to w×h (the right pane). Pure
// Go, rendered live — no subprocess, no cache. Mirrors layoutBuild's spec so they never drift.
func layoutSketch(L layout, w, h int) string {
	const (
		bN = 1
		bE = 2
		bS = 4
		bW = 8
	)
	cw := clampi(w-2, 22, 72)
	ch := clampi(h-3, 8, 24)

	bit := make([][]int, ch)
	lab := make([][]*cell, ch)
	for i := range bit {
		bit[i] = make([]int, cw)
		lab[i] = make([]*cell, cw)
	}
	sb := func(r, c, b int) {
		if r >= 0 && r < ch && c >= 0 && c < cw {
			bit[r][c] |= b
		}
	}
	frame := func(c0, r0, c1, r1 int) {
		for c := c0 + 1; c < c1; c++ {
			sb(r0, c, bE|bW)
			sb(r1, c, bE|bW)
		}
		for r := r0 + 1; r < r1; r++ {
			sb(r, c0, bN|bS)
			sb(r, c1, bN|bS)
		}
		sb(r0, c0, bE|bS)
		sb(r0, c1, bS|bW)
		sb(r1, c0, bN|bE)
		sb(r1, c1, bN|bW)
	}
	vline := func(c, r0, r1 int) {
		for r := r0 + 1; r < r1; r++ {
			sb(r, c, bN|bS)
		}
		sb(r0, c, bS)
		sb(r1, c, bN)
	}
	hline := func(r, c0, c1 int) {
		for c := c0 + 1; c < c1; c++ {
			sb(r, c, bE|bW)
		}
		sb(r, c0, bE)
		sb(r, c1, bW)
	}
	sliceRect := func(c0, r0, c1, r1 int, ws []float64, axis byte) []rect {
		tot := 0.0
		for _, x := range ws {
			tot += x
		}
		if tot == 0 {
			tot = 1
		}
		var rects []rect
		if axis == 'v' {
			xs := []int{c0}
			f := 0.0
			for _, x := range ws[:max(0, len(ws)-1)] {
				f += x / tot
				xs = append(xs, c0+int(math.Round(f*float64(c1-c0))))
			}
			xs = append(xs, c1)
			for i := range ws {
				a, b := xs[i], xs[i+1]
				if i > 0 {
					vline(a, r0, r1)
				}
				rects = append(rects, rect{a, r0, b, r1})
			}
		} else {
			ys := []int{r0}
			f := 0.0
			for _, x := range ws[:max(0, len(ws)-1)] {
				f += x / tot
				ys = append(ys, r0+int(math.Round(f*float64(r1-r0))))
			}
			ys = append(ys, r1)
			for i := range ws {
				a, b := ys[i], ys[i+1]
				if i > 0 {
					hline(a, c0, c1)
				}
				rects = append(rects, rect{c0, a, c1, b})
			}
		}
		return rects
	}

	frame(0, 0, cw-1, ch-1)
	full := rect{0, 0, cw - 1, ch - 1}
	var rects []rect
	switch L.shape {
	case "columns", "main+right":
		rects = sliceRect(0, 0, cw-1, ch-1, norm(L.ratio, len(L.panes)), 'v')
	case "rows", "main+bottom":
		rects = sliceRect(0, 0, cw-1, ch-1, norm(L.ratio, len(L.panes)), 'h')
	case "main+rightstack":
		cols := sliceRect(0, 0, cw-1, ch-1, []float64{mainRatio(L), 100 - mainRatio(L)}, 'v')
		rc := cols[1]
		rects = append([]rect{cols[0]}, sliceRect(rc.c0, rc.r0, rc.c1, rc.r1, norm(restRatio(L), len(L.panes)-1), 'h')...)
	case "main+bottomrow":
		rows := sliceRect(0, 0, cw-1, ch-1, []float64{mainRatio(L), 100 - mainRatio(L)}, 'h')
		rc := rows[1]
		rects = append([]rect{rows[0]}, sliceRect(rc.c0, rc.r0, rc.c1, rc.r1, norm(restRatio(L), len(L.panes)-1), 'v')...)
	default: // single
		rects = []rect{full}
	}

	put := func(r, c int, s string, fg, bg *rgb, bold bool) {
		if r >= 0 && r < ch && c >= 0 && c < cw {
			lab[r][c] = &cell{s, fg, bg, bold}
		}
	}
	pillL, pillR := string(rune(0xE0B6)), string(rune(0xE0B4)) // powerline rounded caps
	title := func(c0, r0, c1 int, icon, name string, accent rgb) {
		row, a, b := r0+1, c0+1, c1-1
		if row >= ch || a > b {
			return
		}
		seg := " " + name + " "
		iconIdx := -1
		if icon != "" {
			seg = " " + icon + " " + name + " "
			iconIdx = 1
		}
		segR := []rune(seg)
		if b-a+1 < len(segR)+2 { // too narrow for a pill → plain label
			nameR := []rune(name)
			for i := 0; i < b-a+1 && i < len(nameR); i++ {
				ac := accent
				put(row, a+i, string(nameR[i]), &ac, nil, true)
			}
			return
		}
		x := a
		put(row, x, pillL, &cSurface0, nil, false)
		x++
		for i, r := range segR {
			if i == iconIdx {
				ac := accent
				put(row, x, string(r), &ac, &cSurface0, true)
			} else {
				put(row, x, string(r), &cText, &cSurface0, false)
			}
			x++
		}
		put(row, x, pillR, &cSurface0, nil, false)
	}
	body := func(c0, r0, c1, r1 int, kind string) {
		switch kind {
		case "nvim":
			for r := r0 + 3; r < r1-1; r++ {
				put(r, c0+2, "~", &cOverlay0, nil, false)
			}
		case "sh":
			put(r0+3, c0+2, "❯", &cGreen, nil, true)
		default:
			for r := r0 + 3; r < r1-1; r += 2 {
				for c := c0 + 2; c < c1-1; c++ {
					put(r, c, "─", &cSurface1, nil, false)
				}
			}
		}
	}
	for i := 0; i < len(rects) && i < len(L.panes); i++ {
		rc, cmd := rects[i], L.panes[i]
		t := appear(cmd)
		title(rc.c0, rc.r0, rc.c1, t.icon, labelOf(cmd), t.accent)
		body(rc.c0, rc.r0, rc.c1, rc.r1, t.kind)
	}

	gl := map[int]string{0: " ", 1: "╵", 2: "╶", 3: "╰", 4: "╷", 5: "│", 6: "╭", 7: "├",
		8: "╴", 9: "╯", 10: "─", 11: "┴", 12: "╮", 13: "┤", 14: "┬", 15: "┼"}
	fgseq := func(c rgb) string { return fmt.Sprintf("\x1b[38;2;%d;%d;%dm", c[0], c[1], c[2]) }
	bgseq := func(c rgb) string { return fmt.Sprintf("\x1b[48;2;%d;%d;%dm", c[0], c[1], c[2]) }
	const rst, bld = "\x1b[0m", "\x1b[1m"
	render := func(r, c int) string {
		if cl := lab[r][c]; cl != nil {
			pre := ""
			if cl.bg != nil {
				pre += bgseq(*cl.bg)
			}
			if cl.fg != nil {
				pre += fgseq(*cl.fg)
			}
			if cl.bold {
				pre += bld
			}
			if pre != "" {
				return pre + cl.ch + rst
			}
			return cl.ch
		}
		if g := gl[bit[r][c]]; g != " " {
			return fgseq(cBorder) + g + rst
		}
		return " "
	}

	pad := strings.Repeat(" ", max(1, (w-cw)/2))
	lines := []string{""}
	for r := range ch {
		var row strings.Builder
		row.WriteString(pad)
		for c := range cw {
			row.WriteString(render(r, c))
		}
		lines = append(lines, row.String())
	}
	lines = append(lines, "", pad+bld+fgseq(appear(L.panes[0]).accent)+L.name+rst+"   "+fgseq(cSubtext)+L.caption+rst)
	if h > 0 && len(lines) > h { // never overrun the pane height
		lines = lines[:h]
	}
	return strings.Join(lines, "\n")
}
