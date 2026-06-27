package main

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
)

// ── kitty remote-control client ──
// prowl is a remote-control client, not a kitten: it shells out to `kitty @ <cmd>`, which
// resolves the control socket from $KITTY_LISTEN_ON. So it needs to run inside kitty with
// `allow_remote_control` + `listen_on` enabled (kittens are Python-only; Go can't be one).

type osWindow struct {
	ID        int   `json:"id"`
	IsFocused bool  `json:"is_focused"`
	Tabs      []tab `json:"tabs"`
}

type tab struct {
	ID        int    `json:"id"`
	Title     string `json:"title"`
	IsActive  bool   `json:"is_active"`
	IsFocused bool   `json:"is_focused"`
	Layout    string `json:"layout"`
	Windows   []kwin `json:"windows"`
}

type kwin struct {
	ID                  int               `json:"id"`
	Title               string            `json:"title"`
	PID                 int               `json:"pid"`
	CWD                 string            `json:"cwd"`
	Cmdline             []string          `json:"cmdline"`
	ForegroundProcesses []procInfo        `json:"foreground_processes"`
	IsFocused           bool              `json:"is_focused"`
	IsActive            bool              `json:"is_active"`
	AtPrompt            bool              `json:"at_prompt"`
	LastExit            int               `json:"last_cmd_exit_status"`
	LastFocusedAt       float64           `json:"last_focused_at"`
	UserVars            map[string]string `json:"user_vars"`
}

type procInfo struct {
	Cmdline []string `json:"cmdline"`
}

// kittyLS runs `kitty @ ls` and parses the OS-window → tab → window tree.
func kittyLS() ([]osWindow, error) {
	out, err := exec.Command("kitty", "@", "ls").Output()
	if err != nil {
		return nil, err
	}
	var ws []osWindow
	if err := json.Unmarshal(out, &ws); err != nil {
		return nil, err
	}
	return ws, nil
}

// focusWindow brings a window (and thus its tab + OS window) to the foreground.
func focusWindow(id int) error {
	return exec.Command("kitty", "@", "focus-window", "--match", "id:"+strconv.Itoa(id)).Run()
}

func closeTab(tabID int) error {
	return exec.Command("kitty", "@", "close-tab", "--match", "id:"+strconv.Itoa(tabID)).Run()
}

func closeWindow(winID int) error {
	return exec.Command("kitty", "@", "close-window", "--match", "id:"+strconv.Itoa(winID)).Run()
}

// findOtherProwl returns the id of another prowl overlay (tagged user_var prowl=1) in the
// focused tab, excluding self — the startup self-toggle uses it to dismiss an open prowl.
func findOtherProwl(tree []osWindow, self int) int {
	for _, ow := range tree {
		if !ow.IsFocused {
			continue
		}
		for _, t := range ow.Tabs {
			if !t.IsActive {
				continue
			}
			for _, w := range t.Windows {
				if w.ID != self && w.UserVars["prowl"] == "1" {
					return w.ID
				}
			}
		}
	}
	return 0
}

func setTabTitle(tabID int, title string) error {
	return exec.Command("kitty", "@", "set-tab-title", "--match", "id:"+strconv.Itoa(tabID), title).Run()
}

// moveToTab detaches a window into an existing tab, then focuses that tab.
func moveToTab(srcWin, tabID int) error {
	if err := exec.Command("kitty", "@", "detach-window",
		"--match", "id:"+strconv.Itoa(srcWin), "--target-tab", "id:"+strconv.Itoa(tabID)).Run(); err != nil {
		return err
	}
	return exec.Command("kitty", "@", "focus-tab", "--match", "id:"+strconv.Itoa(tabID)).Run()
}

// openTab is one open project tab (its active window) — a jump target.
type openTab struct {
	winID     int
	tabID     int
	cwd       string
	title     string
	status    string  // focused | running | idle | failed
	focusedAt float64 // last_focused_at, for recency ordering
	proc      string  // foreground command (nvim / claude / zsh …)
	branch    string  // git branch of cwd
	changes   int     // uncommitted changes
}

// procName is the foreground command basename in a window (the shell when idle).
func procName(w kwin) string {
	if n := len(w.ForegroundProcesses); n > 0 {
		if cl := w.ForegroundProcesses[n-1].Cmdline; len(cl) > 0 {
			return filepath.Base(cl[0])
		}
	}
	if len(w.Cmdline) > 0 {
		return filepath.Base(w.Cmdline[0])
	}
	return ""
}

// openTabs flattens `kitty @ ls` to one jump target per tab (the active window), skipping
// prowl's own window, and returns the set of cwds already open (to dedup the project list).
func openTabs() ([]openTab, map[string]bool, error) {
	tree, err := kittyLS()
	if err != nil {
		return nil, nil, err
	}
	self := os.Getenv("KITTY_WINDOW_ID")
	var tabs []openTab
	cwds := map[string]bool{}
	for _, ow := range tree {
		for _, t := range ow.Tabs {
			var wins []kwin
			for _, w := range t.Windows {
				if strconv.Itoa(w.ID) != self {
					wins = append(wins, w)
				}
			}
			if len(wins) == 0 {
				continue
			}
			a := wins[0]
			for _, w := range wins {
				if w.IsActive || w.IsFocused {
					a = w
					break
				}
			}
			st := "idle"
			switch {
			case a.IsFocused && t.IsActive && ow.IsFocused: // the one globally-focused window (active tab of the focused OS window)
				st = "focused"
			case a.LastExit != 0:
				st = "failed"
			case !a.AtPrompt:
				st = "running"
			}
			branch, changes, _ := gitStatus(a.CWD)
			tabs = append(tabs, openTab{
				winID: a.ID, tabID: t.ID, cwd: a.CWD, title: t.Title, status: st,
				focusedAt: a.LastFocusedAt, proc: procName(a), branch: branch, changes: changes,
			})
			if a.CWD != "" {
				cwds[a.CWD] = true
			}
		}
	}
	sort.SliceStable(tabs, func(i, j int) bool { return tabs[i].focusedAt > tabs[j].focusedAt }) // most-recent first
	return tabs, cwds, nil
}
