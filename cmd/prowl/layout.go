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

// dirPreview is the right-pane preview for a directory, in two labelled sections: REPO (git
// branch + change count) and FILES (a plain listing). Section heads are styled; the bodies
// are plain text so rightContent can truncate them per line.
func dirPreview(dir string) string {
	var b strings.Builder
	b.WriteString(sectionHead("REPO") + "\n")
	if branch, changes, repo := gitStatus(dir); repo {
		fmt.Fprintf(&b, "%s · %s · %d changes\n", filepath.Base(dir), branch, changes)
	} else {
		fmt.Fprintf(&b, "%s · not a git repo\n", filepath.Base(dir))
	}
	b.WriteString("\n" + sectionHead("FILES") + "\n")
	if out, err := exec.Command("ls", "-1A", dir).Output(); err == nil {
		b.Write(out)
	}
	return b.String()
}
