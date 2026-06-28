package main

import (
	"strings"
	"testing"
)

func TestWrapPlain(t *testing.T) {
	got := wrapPlain("the quick brown fox jumps", 9)
	for _, l := range got {
		if len([]rune(l)) > 9 {
			t.Fatalf("line %q exceeds width 9", l)
		}
	}
	if strings.Join(got, " ") != "the quick brown fox jumps" {
		t.Fatalf("words lost/reordered: %v", got)
	}
	// an overlong word hard-breaks rather than ellipsizing
	hb := wrapPlain("supercalifragilistic", 5)
	if strings.Contains(strings.Join(hb, ""), "…") {
		t.Fatalf("hard-break should not ellipsize: %v", hb)
	}
}

func TestWrapLinesKeepsANSI(t *testing.T) {
	ansi := "\x1b[38;2;1;2;3mcolored and very long line well past the width\x1b[0m"
	got := wrapLines(ansi, 10)
	if len(got) != 1 || got[0] != ansi {
		t.Fatalf("ANSI line should pass through untouched, got %v", got)
	}
}

func TestAgentSectionCapsAtTen(t *testing.T) {
	reply := strings.Repeat("word\n", 40) // 40 short lines
	out := agentSection("what is this", reply, 60)
	body := strings.TrimPrefix(out, sectionHead("AGENT")+"\n")
	if n := strings.Count(body, "word"); n != 10 {
		t.Fatalf("teaser should cap at 10 reply lines, got %d", n)
	}
	if !strings.Contains(out, "press a") {
		t.Fatal("a clipped teaser should hint at pressing a")
	}
	if !strings.Contains(out, "what is this") {
		t.Fatal("teaser should show the question")
	}
}

func TestRightPaneSections(t *testing.T) {
	m := model{
		all:         []item{{kind: "project", dir: "/p"}},
		preview:     sectionHead("REPO") + "\np · main · 0 changes\n\n" + sectionHead("FILES") + "\nREADME.md",
		replyCache:  map[string]string{"/p\x00q": "the answer"},
		lastInstr:   map[string]string{"/p": "q"},
		workingDirs: map[string]bool{},
		cache:       map[string]string{},
	}
	m = m.applyFilter()
	out := m.rightContent(60, 40)
	for _, want := range []string{"AGENT", "q", "the answer", "REPO", "FILES", "README.md"} {
		if !strings.Contains(out, want) {
			t.Fatalf("right pane missing %q:\n%s", want, out)
		}
	}
	// while a query is in flight, AGENT shows working and the rest still renders
	m.workingDirs["/p"] = true
	if w := m.rightContent(60, 40); !strings.Contains(w, "working") || !strings.Contains(w, "FILES") {
		t.Fatalf("working pane wrong:\n%s", w)
	}
}

func TestLayoutRow(t *testing.T) {
	m := model{
		layouts:    []string{"zsh", "dev"},
		layoutDefs: map[string]layout{"zsh": {name: "zsh", caption: "just a shell"}, "dev": {name: "dev", caption: "editor · shell"}},
	}
	sel := m.layoutRow(1, 40, true)
	if !strings.Contains(sel, "dev") || !strings.Contains(sel, "editor · shell") {
		t.Fatalf("selected row should show name + caption: %q", sel)
	}
	un := m.layoutRow(0, 40, false)
	if !strings.Contains(un, "zsh") || !strings.Contains(un, "just a shell") {
		t.Fatalf("row should show name + caption: %q", un)
	}
}

func TestTrunc(t *testing.T) {
	if got := trunc("hello", 10); got != "hello" {
		t.Fatalf("no truncation when it fits: %q", got)
	}
	if got := trunc("hello world", 5); got != "hell…" {
		t.Fatalf("trunc(…,5) = %q, want hell…", got)
	}
}
