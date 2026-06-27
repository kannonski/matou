package main

import "os"

// previewHook is an optional command that generates the right-pane content for the selected
// directory — e.g. an AI brief of the repo. It's run as `<cmd> <dir>` and its stdout is
// shown in the preview pane (async, debounced, cached). Set it via $PROWL_PREVIEW_CMD;
// empty means the pane just shows the local git + listing.
var previewHook string

func loadConfig() {
	previewHook = os.Getenv("PROWL_PREVIEW_CMD")
}
