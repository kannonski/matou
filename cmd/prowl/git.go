package main

import (
	"os/exec"
	"strings"
)

// gitStatus returns a directory's branch and uncommitted-change count (repo=false if not a
// git work tree). Used both inline on open rows and in the preview pane.
func gitStatus(dir string) (branch string, changes int, repo bool) {
	out, err := exec.Command("git", "-C", dir, "rev-parse", "--is-inside-work-tree").Output()
	if err != nil || strings.TrimSpace(string(out)) != "true" {
		return "", 0, false
	}
	repo = true
	if b, err := exec.Command("git", "-C", dir, "branch", "--show-current").Output(); err == nil {
		branch = strings.TrimSpace(string(b))
	}
	if branch == "" {
		branch = "detached"
	}
	if st, err := exec.Command("git", "-C", dir, "status", "--porcelain").Output(); err == nil {
		for _, l := range strings.Split(strings.TrimSpace(string(st)), "\n") {
			if strings.TrimSpace(l) != "" {
				changes++
			}
		}
	}
	return
}
