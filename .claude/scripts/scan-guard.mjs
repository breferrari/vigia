#!/usr/bin/env node
// PreToolUse guard: refuse a `find` whose start path is `/`. Under Git Bash on
// Windows `/` is the MSYS root, so `~/.cargo` is not under it and no match is
// possible, and `/proc/registry` mounts the Windows registry as directories,
// so the walk enumerates the hives through live Win32 calls and never ends.
// Six of these, left orphaned, burned 26.8 CPU-hours in five hours. Exit 2
// blocks the call; anything the guard cannot read fails OPEN.

import { readFileSync } from "node:fs";

let command = "";
try {
	command = JSON.parse(readFileSync(0, "utf8"))?.tool_input?.command ?? "";
} catch {
	process.exit(0);
}
if (typeof command !== "string") process.exit(0);

// `find`, its option flags, then a start path that is `/` alone. `find /c/Dev`
// and `find . -path /x` are not this.
const rooted = /(?:^|[\s;&|(])find\s+(?:-[A-Z]\s+)*\/(?=\s|$)/;
const m = rooted.exec(command);
if (!m) process.exit(0);

// A walk bounded by `timeout` is what CLAUDE.md asks for instead. The bound
// has to sit on this command: a `timeout` in an earlier command of the same
// line, past a `;` or a pipe, bounds nothing here.
const simple = command.slice(0, m.index + 1).split(/\n|;|&&|\|\|?/).pop() ?? "";
if (/(?:^|\s)timeout\s/.test(simple)) process.exit(0);

console.error(
	"BLOCKED: `find /` walks the MSYS root, which mounts the Windows registry under /proc and never terminates.\n" +
		"Point Glob or Grep at an explicit path, or bound the scan with `timeout`. (CLAUDE.md; this hook is its enforcement.)",
);
process.exit(2);
