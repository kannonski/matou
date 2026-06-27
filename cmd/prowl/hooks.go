package main

import (
	"os/exec"
	"strings"
	"time"

	tea "github.com/charmbracelet/bubbletea"
)

// previewMsg carries an agent-generated preview back for a directory.
type previewMsg struct {
	dir  string
	text string
}

// settleMsg fires once the cursor has rested (debounce), so we only ask the agent about
// rows you actually pause on — not every row you scroll past.
type settleMsg struct{ gen int }

func settleTick(gen int) tea.Cmd {
	return tea.Tick(350*time.Millisecond, func(time.Time) tea.Msg { return settleMsg{gen: gen} })
}

// previewCmd runs the configured preview hook for dir in the background.
func previewCmd(dir string) tea.Cmd {
	parts := strings.Fields(previewHook)
	if len(parts) == 0 {
		return nil
	}
	return func() tea.Msg {
		out, err := exec.Command(parts[0], append(parts[1:], dir)...).Output()
		text := strings.TrimRight(string(out), "\n")
		if text == "" && err != nil {
			text = "⚠ preview hook failed"
		}
		return previewMsg{dir: dir, text: text}
	}
}
