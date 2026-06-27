package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// projectDirs returns candidate project directories — zoxide (by frecency) first, then the
// static ~/Project roots and a couple of fixed dirs — deduped and minus the cwds already
// open as tabs (so the list never offers what you can just jump to). Mirrors the sources
// in kitty_project.sh.
func projectDirs(openCwds map[string]bool) []string {
	seen := map[string]bool{}
	var out []string
	add := func(d string) {
		d = strings.TrimRight(strings.TrimSpace(d), "/")
		if d == "" || seen[d] || openCwds[d] {
			return
		}
		if fi, err := os.Stat(d); err != nil || !fi.IsDir() {
			return
		}
		seen[d] = true
		out = append(out, d)
	}
	if b, err := exec.Command("zoxide", "query", "-l").Output(); err == nil {
		for _, line := range strings.Split(strings.TrimSpace(string(b)), "\n") {
			add(line)
		}
	}
	home, _ := os.UserHomeDir()
	for _, pat := range []string{"Project/gitlab/*", "Project/github/*"} {
		matches, _ := filepath.Glob(filepath.Join(home, pat))
		for _, m := range matches {
			add(m)
		}
	}
	add(filepath.Join(home, ".local/share/chezmoi"))
	add(filepath.Join(home, ".config/nvim"))
	return out
}

func zoxideRemove(dir string) error {
	return exec.Command("zoxide", "remove", dir).Run()
}
