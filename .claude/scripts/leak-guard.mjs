#!/usr/bin/env node
// PreToolUse guard: block a `gh pr|issue|release create|edit|comment` or a
// `git commit` whose text carries an agent-session artifact, before it reaches
// a PUBLIC repo. The rule this enforces already lived in CLAUDE.md; this is the
// part that does not depend on a session remembering it. A commit is covered
// because a squash merged to a public `main` stays reachable by SHA forever.
// Exit 2 blocks the call; anything the guard cannot parse or read fails OPEN
// (exit 0), because a guard must never block work by breaking.
//
// Classes checked (each is a class of artifact, not one incident):
//   - claude.ai session URLs and Claude-Session: trailers
//   - local absolute paths (machine layout + username)
//   - a --body-file whose newlines were destroyed (PowerShell array-join
//     flattening: one giant line where a document should be)
//
// The path handed to --body-file, --file or -F is dropped from the inline scan
// first: it says where the body sits and never lands in it, and a scratch
// directory under the user's profile is exactly the shape the path class matches.

import { readFileSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";

const PATTERNS = [
	["agent session URL", /claude\.ai\/code\/session[_/][A-Za-z0-9_-]+/i],
	["Claude-Session trailer", /^\s*Claude-Session:/im],
	["local absolute path", /(?:^|[\s"'=])[A-Za-z]:\\(?:Users|Dev)\\[^\s"']+/],
];

let input = "";
try {
	input = readFileSync(0, "utf8");
} catch {
	process.exit(0);
}

let command = "";
try {
	command = JSON.parse(input)?.tool_input?.command ?? "";
} catch {
	process.exit(0);
}
if (typeof command !== "string") process.exit(0);

const publishes = /\bgh\s+(pr|issue|release)\s+(create|edit|comment)\b/.test(command);
const commits = /\bgit\s+commit\b/.test(command);
if (!publishes && !commits) process.exit(0);

const block = (what, hits) => {
	console.error(
		`BLOCKED: ${what} is not safe to publish from this PUBLIC repo.\n` +
			hits.map((h) => `  - ${h}`).join("\n") +
			`\nFix the content, then re-run. (House rule #1 in CLAUDE.md; this hook is its enforcement.)`,
	);
	process.exit(2);
};

const m = /(?:--body-file|--file|-F)[= ]\s*(?:"([^"]+)"|'([^']+)'|(\S+))/.exec(command);
const bodyFile = m ? (m[1] ?? m[2] ?? m[3]) : null;
const inline = m ? command.replace(m[0], " ") : command;

const inlineHits = PATTERNS.filter(([, re]) => re.test(inline)).map(([label]) => label);
if (inlineHits.length > 0) block("this command's inline body", inlineHits);

if (!bodyFile) process.exit(0);

let body = "";
try {
	const path = isAbsolute(bodyFile) ? bodyFile : resolve(process.env.CLAUDE_PROJECT_DIR ?? process.cwd(), bodyFile);
	body = readFileSync(path, "utf8");
} catch {
	process.exit(0);
}

const hits = PATTERNS.filter(([, re]) => re.test(body)).map(([label]) => label);
if (publishes && body.length > 300 && !body.trim().includes("\n")) {
	hits.push("flattened body: one line where a document should be (PowerShell array-join; use [System.IO.File]::WriteAllText with a `n join)");
}
if (hits.length > 0) block(bodyFile, hits);
process.exit(0);
