#!/usr/bin/env bash
# Sample PROWL_PREVIEW_CMD — brief the selected project in prowl's right pane.
# Enable it with:  export PROWL_PREVIEW_CMD="$HOME/.config/kitty/prowl-preview.sh"
# prowl calls it as `<this> <dir>` (debounced: only for rows you pause on; cached per
# session). It receives the directory as $1 and prints a short brief to stdout.
#
# Cost note: this runs your AI once per project you pause on. The cache means revisits
# are free, but if you don't want LLM calls, leave PROWL_PREVIEW_CMD unset (the pane then
# just shows git + listing).
set -uo pipefail
dir=${1:?usage: prowl-preview.sh <dir>}
cd "$dir" 2>/dev/null || { echo "(directory is gone)"; exit 0; }

log=$(git log --oneline -6 2>/dev/null)
status=$(git status --short 2>/dev/null | head -8)
readme=$(head -40 README* readme* 2>/dev/null)

# No AI available → just show the recent commits.
if ! command -v claude >/dev/null 2>&1; then
	printf 'recent\n%s\n' "${log:-(no git history)}"
	exit 0
fi

timeout 30 claude -p "Brief this project in 3–4 short lines for someone deciding whether to jump into it: what it is, what's in flight (uncommitted / recent commits), and one sensible next action. Terse, no preamble, no markdown headers.

dir: $dir
recent commits:
$log
uncommitted:
$status
README (head):
$readme" --model "${PROWL_MODEL:-sonnet}" 2>/dev/null
