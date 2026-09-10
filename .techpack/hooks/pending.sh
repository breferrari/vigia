#!/bin/bash
# Puts one line in front of the reader's next prompt saying what their notes are
# waiting on. Stdout is that line, so it is left alone: anything written here
# reaches the prompt.
set -uo pipefail
trap 'exit 0' ERR

# Runs before every prompt, so the quiet paths matter most. With no vigia, no
# worktree or nothing pending, it prints nothing and costs the prompt nothing.
command -v vigia >/dev/null 2>&1 || exit 0

vigia mcp pending || exit 0
