package main

import "os"

// agentHook is the command behind the `:` agent: invoked as `<cmd> <dir> "<instruction>"`,
// its stdout fills the floating agent panel. Set via $PROWL_AGENT_CMD; empty disables `:`.
var agentHook string

func loadConfig() {
	agentHook = os.Getenv("PROWL_AGENT_CMD")
}
