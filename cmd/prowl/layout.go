package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// palette.py is the user's layout engine (names / sketch / build). prowl reuses it rather
// than reinventing layouts. Override the path with $PROWL_PALETTE.
func palettePath() string {
	if p := os.Getenv("PROWL_PALETTE"); p != "" {
		return p
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "kitty", "palette.py")
}

func paletteNames() []string {
	out, err := exec.Command("python3", palettePath(), "names").Output()
	if err != nil {
		return nil
	}
	var names []string
	for _, l := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if l = strings.TrimSpace(l); l != "" {
			names = append(names, l)
		}
	}
	return names
}

func paletteSketch(name string) string {
	out, _ := exec.Command("python3", palettePath(), "sketch", name).Output()
	return string(out)
}

// paletteBuild lays out the panes for `name` in a new tab cwd'd to `dir`.
func paletteBuild(name, dir string) error {
	return exec.Command("python3", palettePath(), "build", name, dir).Run()
}

// dirPreview is the right-pane preview for a directory: git branch + change count, then a
// plain listing. Plain text (no ANSI) so it's safe to truncate per line.
func dirPreview(dir string) string {
	var b strings.Builder
	if branch, changes, repo := gitStatus(dir); repo {
		fmt.Fprintf(&b, "%s    %s    %d changes\n\n", filepath.Base(dir), branch, changes)
	} else {
		fmt.Fprintf(&b, "%s    (not a git repo)\n\n", filepath.Base(dir))
	}
	if out, err := exec.Command("ls", "-1A", dir).Output(); err == nil {
		b.Write(out)
	}
	return b.String()
}
