package main

import (
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"
)

// key builds a rune key message (e.g. "a", "j"); special keys use tea.KeyMsg{Type: ...}.
func key(s string) tea.KeyMsg { return tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune(s)} }

// projectModel is a minimal model with one project row selected, agent enabled.
func projectModel() model {
	agentHook = "true"
	m := model{
		all:         []item{{kind: "project", dir: "/p"}},
		replyCache:  map[string]string{},
		lastInstr:   map[string]string{},
		workingDirs: map[string]bool{},
		cache:       map[string]string{},
		w:           100, h: 30,
	}
	return m.applyFilter()
}

func TestNavQuitKeys(t *testing.T) {
	for _, k := range []string{"q", "h"} {
		if _, cmd := projectModel().updateNav(key(k)); cmd == nil {
			t.Errorf("%q should quit (non-nil cmd)", k)
		}
	}
}

func TestAgentTriggerAndRestore(t *testing.T) {
	m := projectModel()
	// fresh dir → opens in input focus, empty
	got, _ := m.updateNav(key("a"))
	if mm := got.(model); mm.mode != "agent" || mm.agentFocus != "input" || mm.agentResult != "" {
		t.Fatalf("fresh `a`: mode=%q focus=%q result=%q", mm.mode, mm.agentFocus, mm.agentResult)
	}
	// dir with a cached answer → opens in read focus with the full answer + question restored
	m.lastInstr["/p"] = "what is this"
	m.replyCache["/p\x00what is this"] = "line1\nline2\nthe full answer"
	got, _ = m.updateNav(key("a"))
	mm := got.(model)
	if mm.agentFocus != "read" {
		t.Fatalf("cached answer should open in read focus, got %q", mm.agentFocus)
	}
	if mm.agentResult != "line1\nline2\nthe full answer" {
		t.Fatalf("panel should restore the full answer, got %q", mm.agentResult)
	}
	if mm.agentInput != "what is this" {
		t.Fatalf("panel should pre-fill the question, got %q", mm.agentInput)
	}
}

func TestAgentReadScroll(t *testing.T) {
	m := projectModel()
	m.mode, m.agentDir, m.agentFocus = "agent", "/p", "read"
	m.agentResult = strings.Repeat("x\n", 100)
	_, bodyH := agentDims(m.w, m.h)

	step := func(k tea.KeyMsg) { mm, _ := m.updateAgent(k); m = mm.(model) }
	step(key("j"))
	if m.agentOff != 1 {
		t.Fatalf("j → off 1, got %d", m.agentOff)
	}
	step(tea.KeyMsg{Type: tea.KeyCtrlD})
	if m.agentOff != 1+max(1, bodyH/2) {
		t.Fatalf("ctrl+d half-page, got %d", m.agentOff)
	}
	step(key("G"))
	if m.agentOff < 1<<20 {
		t.Fatalf("G → far down, got %d", m.agentOff)
	}
	step(key("g"))
	if m.agentOff != 0 {
		t.Fatalf("g → top, got %d", m.agentOff)
	}
	step(key("i"))
	if m.agentFocus != "input" {
		t.Fatalf("i → input focus, got %q", m.agentFocus)
	}
}

func TestLayoutPickerNav(t *testing.T) {
	useSampleLayouts(t)
	m := projectModel()
	// l on a project enters the layout picker, loaded from palette.layouts (actOn returns model)
	m, _ = m.actOn(0)
	if m.mode != "layout" || len(m.layouts) != 3 {
		t.Fatalf("actOn project → layout picker with 3 layouts, got mode=%q n=%d", m.mode, len(m.layouts))
	}
	got, _ := m.updateLayout(key("j"))
	if got.(model).layCur != 1 {
		t.Fatalf("j → layCur 1, got %d", got.(model).layCur)
	}
	got, _ = m.updateLayout(key("h"))
	if got.(model).mode != "" {
		t.Fatalf("h → back out of layout mode, got %q", got.(model).mode)
	}
}

func TestMoveTwoStage(t *testing.T) {
	m := model{
		all: []item{
			{kind: "open", winID: 11, tabID: 1, title: "a"},
			{kind: "open", winID: 22, tabID: 2, title: "b"},
		},
		cache: map[string]string{},
	}
	m.mode = "move"
	m = m.applyFilter()
	// stage A: pick the pane (esc here cancels to nav)
	got, _ := m.updateMove(key("k")) // move cursor
	m = got.(model)
	if m.moveSrc != 0 {
		t.Fatal("stage A: no source picked yet")
	}
}
