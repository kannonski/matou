package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestAgentCacheRoundTrip(t *testing.T) {
	t.Setenv("XDG_CACHE_HOME", t.TempDir())

	replies := map[string]string{"/p\x00what is this": "a long answer"}
	last := map[string]string{"/p": "what is this"}
	saveAgentCache(replies, last)

	if _, err := os.Stat(filepath.Join(os.Getenv("XDG_CACHE_HOME"), "matou", "agent.json")); err != nil {
		t.Fatalf("cache file not written: %v", err)
	}
	gotReplies, gotLast := loadAgentCache()
	if gotReplies["/p\x00what is this"] != "a long answer" { // \x00 keys survive JSON
		t.Fatalf("replies not round-tripped: %v", gotReplies)
	}
	if gotLast["/p"] != "what is this" {
		t.Fatalf("lastInstr not round-tripped: %v", gotLast)
	}
}

func TestLoadAgentCacheMissing(t *testing.T) {
	t.Setenv("XDG_CACHE_HOME", t.TempDir()) // empty dir → no file
	replies, last := loadAgentCache()
	if len(replies) != 0 || len(last) != 0 {
		t.Fatalf("missing cache should yield empty maps, got %v / %v", replies, last)
	}
}
