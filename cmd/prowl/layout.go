package main

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
)

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
