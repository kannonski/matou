package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const sampleLayouts = `
[zsh]
shape   = "single"
panes   = ["zsh"]
caption = "just a shell"
order   = 0

[dev]
shape   = "main+rightstack"
panes   = ["nvim {dir}", "zsh", "lazygit"]
ratio   = [62, 50]
caption = "editor · shell · lazygit"
order   = 1

[k8s]
shape   = "main+right"
panes   = ["k9s", "zsh"]
ratio   = [60]
caption = "k9s · shell"
order   = 5
`

// useSampleLayouts points loadLayouts at a temp palette.layouts for the test's duration.
func useSampleLayouts(t *testing.T) {
	t.Helper()
	p := filepath.Join(t.TempDir(), "palette.layouts")
	if err := os.WriteFile(p, []byte(sampleLayouts), 0o644); err != nil {
		t.Fatal(err)
	}
	t.Setenv("MATOU_LAYOUTS", p)
}

func TestLoadLayouts(t *testing.T) {
	useSampleLayouts(t)
	ls := loadLayouts()
	if got := names(ls); !equalStrings(got, []string{"zsh", "dev", "k8s"}) {
		t.Fatalf("ordered names = %v, want [zsh dev k8s]", got)
	}
	dev := ls[1]
	if dev.shape != "main+rightstack" || dev.caption != "editor · shell · lazygit" {
		t.Fatalf("dev = %+v", dev)
	}
	if !equalStrings(dev.panes, []string{"nvim {dir}", "zsh", "lazygit"}) {
		t.Fatalf("dev.panes = %v", dev.panes)
	}
	if len(dev.ratio) != 2 || dev.ratio[0] != 62 || dev.ratio[1] != 50 {
		t.Fatalf("dev.ratio = %v", dev.ratio)
	}
	if ls[0].shape != "single" { // a one-pane layout is forced to single
		t.Fatalf("zsh shape = %q, want single", ls[0].shape)
	}
}

func TestLoadLayoutsMissingFile(t *testing.T) {
	t.Setenv("MATOU_LAYOUTS", filepath.Join(t.TempDir(), "does-not-exist"))
	if ls := loadLayouts(); ls != nil {
		t.Fatalf("missing file should yield nil, got %v", ls)
	}
}

func TestShlexSplit(t *testing.T) {
	cases := []struct {
		in   string
		want []string
	}{
		{`nvim {dir}`, []string{"nvim", "{dir}"}},
		{`npm "run dev"`, []string{"npm", "run dev"}},
		{`claude --resume`, []string{"claude", "--resume"}},
		{`  spaced   out  `, []string{"spaced", "out"}},
		{``, nil},
	}
	for _, c := range cases {
		if got := shlexSplit(c.in); !equalStrings(got, c.want) {
			t.Errorf("shlexSplit(%q) = %v, want %v", c.in, got, c.want)
		}
	}
}

func TestArgvForAndLabel(t *testing.T) {
	if got := argvFor("nvim {dir}", "/home/x/p"); !equalStrings(got, []string{"nvim", "/home/x/p"}) {
		t.Fatalf("argvFor {dir} = %v", got)
	}
	if labelOf("/usr/bin/nvim {dir}") != "nvim" {
		t.Fatal("labelOf should take the basename")
	}
	if labelOf("") != "sh" {
		t.Fatal("labelOf(empty) should be sh")
	}
}

func TestNorm(t *testing.T) {
	cases := []struct {
		sizes []float64
		n     int
		want  []float64
	}{
		{[]float64{60}, 2, []float64{60, 40}},        // leading value, rest take the remainder
		{nil, 2, []float64{50, 50}},                  // no sizes → equal split
		{[]float64{62, 50}, 3, []float64{62, 50, 0}}, // remainder is 0 once leading sums ≥ 100... here 62+50>100 → 0
		{nil, 1, []float64{100}},
	}
	for _, c := range cases {
		got := norm(c.sizes, c.n)
		if !equalFloats(got, c.want) {
			t.Errorf("norm(%v, %d) = %v, want %v", c.sizes, c.n, got, c.want)
		}
	}
}

func TestLayoutSketch(t *testing.T) {
	useSampleLayouts(t)
	var dev layout
	for _, L := range loadLayouts() {
		if L.name == "dev" {
			dev = L
		}
	}
	out := layoutSketch(dev, 58, 20)
	if !strings.Contains(out, "╭") || !strings.Contains(out, "│") {
		t.Fatal("sketch should draw box borders")
	}
	if !strings.Contains(out, "dev") || !strings.Contains(out, "editor · shell · lazygit") {
		t.Fatal("sketch should carry the name + caption")
	}
	if n := strings.Count(out, "\n") + 1; n > 20 {
		t.Fatalf("sketch must fit within h=20, got %d lines", n)
	}
}

func names(ls []layout) []string {
	var out []string
	for _, L := range ls {
		out = append(out, L.name)
	}
	return out
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func equalFloats(a, b []float64) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
