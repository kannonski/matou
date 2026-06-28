package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
)

// agentStore is the on-disk persistence of the `a` agent: replies keyed by dir+\x00+instr and
// the last instruction per dir. prowl is launched fresh on every Ctrl+Shift+O, so without this
// the right-pane teaser and the panel would be empty until you re-ask. Loaded on start, saved
// whenever a reply lands or a question is asked.
type agentStore struct {
	Replies   map[string]string `json:"replies"`
	LastInstr map[string]string `json:"last_instr"`
}

func agentCachePath() string {
	dir := os.Getenv("XDG_CACHE_HOME")
	if dir == "" {
		home, _ := os.UserHomeDir()
		dir = filepath.Join(home, ".cache")
	}
	return filepath.Join(dir, "prowl", "agent.json")
}

// loadAgentCache reads the persisted caches (empty maps on a missing/corrupt file).
func loadAgentCache() (replies, lastInstr map[string]string) {
	replies, lastInstr = map[string]string{}, map[string]string{}
	b, err := os.ReadFile(agentCachePath())
	if err != nil {
		return
	}
	var s agentStore
	if json.Unmarshal(b, &s) == nil {
		if s.Replies != nil {
			replies = s.Replies
		}
		if s.LastInstr != nil {
			lastInstr = s.LastInstr
		}
	}
	return
}

// saveAgentCache atomically persists the agent caches (pid-tagged temp + rename, so a brief
// second prowl during the toggle flash can't corrupt the file). Best-effort: errors are
// swallowed — a lost cache write just means re-asking.
func saveAgentCache(replies, lastInstr map[string]string) {
	p := agentCachePath()
	if os.MkdirAll(filepath.Dir(p), 0o755) != nil {
		return
	}
	b, err := json.Marshal(agentStore{Replies: replies, LastInstr: lastInstr})
	if err != nil {
		return
	}
	tmp := p + "." + strconv.Itoa(os.Getpid()) + ".tmp"
	if os.WriteFile(tmp, b, 0o644) != nil {
		return
	}
	_ = os.Rename(tmp, p)
}
