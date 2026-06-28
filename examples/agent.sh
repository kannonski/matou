#!/usr/bin/env bash
# Sample MATOU_AGENT_CMD — answer an instruction about the selected project, in matou's
# floating `a` panel. Enable with:  export MATOU_AGENT_CMD="$HOME/.config/kitty/matou-agent.sh"
# matou calls it as `<this> <dir> "<instruction>"`; print the answer to stdout.
set -uo pipefail
dir=${1:?usage: agent.sh <dir> <instruction>}
instr=${2:?usage: agent.sh <dir> <instruction>}
cd "$dir" 2>/dev/null || { echo "(directory is gone)"; exit 0; }
command -v claude >/dev/null 2>&1 || { echo "claude not found on PATH"; exit 0; }

log=$(git log --oneline -8 2>/dev/null)
status=$(git status --short 2>/dev/null | head -12)
readme=$(head -40 README* readme* 2>/dev/null)

timeout 60 claude -p "Answer this for a developer eyeing a project in a switcher. Be concrete and terse, plain text (no markdown headers). You MAY read files and run read-only git/rg/ls to ground the answer; do NOT modify anything.

dir: $dir
instruction: $instr

recent commits:
$log
uncommitted:
$status
README (head):
$readme" \
  --model "${MATOU_MODEL:-sonnet}" \
  --allowedTools "Read" "Bash(git:*)" "Bash(rg:*)" "Bash(ls:*)" "Bash(cat:*)" 2>/dev/null
