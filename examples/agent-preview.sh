#!/usr/bin/env bash
# Sample PROWL_PREVIEW_CMD — brief the selected project in prowl's right pane.
# Enable with:  export PROWL_PREVIEW_CMD="$HOME/.config/kitty/prowl-preview.sh"
#
# SMART-GATED: it only spends an LLM call on repos with work in flight — uncommitted
# changes, or a commit in the last 7 days. Idle/clean repos print nothing, so prowl just
# shows its local git + listing (zero cost). prowl runs this debounced (only for rows you
# pause on) and caches the result per session, so a browse rarely costs more than a call
# or two. Receives the directory as $1.
set -uo pipefail
dir=${1:?usage: agent-preview.sh <dir>}
cd "$dir" 2>/dev/null || exit 0
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0

branch=$(git branch --show-current 2>/dev/null); branch=${branch:-detached}
dirty=$(git status --short 2>/dev/null | grep -c .)
age=$(git log -1 --format=%cr 2>/dev/null)
recent=$(git log -1 --since="7 days ago" --format=%h 2>/dev/null)

# Gate: only repos with something in flight are worth an LLM call.
[ "${dirty:-0}" -eq 0 ] && [ -z "${recent:-}" ] && exit 0
command -v claude >/dev/null 2>&1 || exit 0

diffstat=$(git diff --stat 2>/dev/null | tail -4)
log=$(git log --oneline -6 2>/dev/null)
timeout 25 claude -p "Brief someone returning to this repo through a project switcher. Exactly 3 short lines, no preamble, no markdown headers:
1) the state it's in
2) what's uncommitted / in-flight
3) the single most sensible next action
Be concrete — name files/areas — and terse.

branch: $branch · ${dirty} uncommitted · last commit ${age}
git diff --stat:
$diffstat
recent commits:
$log" --model "${PROWL_MODEL:-sonnet}" 2>/dev/null
