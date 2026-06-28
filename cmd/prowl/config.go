package main

import "os"

// agentHook is the command behind the `a` agent: invoked as `<cmd> <dir> "<instruction>"`,
// its stdout fills the floating agent panel. Set via $PROWL_AGENT_CMD; empty disables `a`.
var agentHook string

func loadConfig() {
	agentHook = os.Getenv("PROWL_AGENT_CMD")
}
