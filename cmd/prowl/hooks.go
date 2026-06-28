package main

import (
	"os/exec"
	"strings"

	tea "github.com/charmbracelet/bubbletea"
)

// agentMsg carries the agent's reply for a (dir, instruction), back into the panel.
type agentMsg struct {
	dir   string
	instr string
	text  string
}

// agentCmd runs the agent hook for `:` in the background: `<hook> <dir> "<instruction>"`.
func agentCmd(dir, instr string) tea.Cmd {
	parts := strings.Fields(agentHook)
	if len(parts) == 0 {
		return nil
	}
	return func() tea.Msg {
		args := append(append([]string{}, parts[1:]...), dir, instr)
		out, err := exec.Command(parts[0], args...).Output()
		text := strings.TrimRight(string(out), "\n")
		if text == "" && err != nil {
			text = "⚠ agent failed (" + err.Error() + ")"
		}
		return agentMsg{dir: dir, instr: instr, text: text}
	}
}
