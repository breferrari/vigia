#!/bin/bash
# Records the session's own socket beside the note store, so that pressing Enter
# in the pane posts the note into this session instead of leaving it to wait.
#
# Registered on both SessionStart and SessionEnd: the payload on stdin carries
# the event name, and `vigia mcp register` branches on it to record the socket or
# to clear it. Neither stream is redirected, so that payload reaches it.
set -uo pipefail
trap 'exit 0' ERR

# A note is a convenience, and must never be why a session fails to start. With
# no vigia, no worktree or no store, this says nothing and exits clean.
command -v vigia >/dev/null 2>&1 || exit 0

vigia mcp register || exit 0
