#!/usr/bin/env node
// PreToolUse guard: block publishing a branch that both FILED a decision and
// RULED on it. Exit 2 blocks the call; anything the guard cannot read fails
// OPEN (exit 0) — a guard must never block work by breaking.
//
// The rule this enforces already lives in `.claude/skills/take-next/SKILL.md`
// ("the label is the reader's"), and in CLAUDE.md's refusal section. This is the
// part that does not depend on a session remembering it. That distinction is
// the whole reason this file exists: the prose was correct, was in context, and
// was quoted back while being breached.
//
// What it catches. On 2026-08-29 a pass took #177 — a feature request carrying
// no labels — wrote the ROADMAP.md row classifying it as a decision, wrote the
// SPEC.md ruling declining it, and closed the issue, in one commit. Every other
// guard here is evadable that way: a session that can decide what KIND of issue
// it is holding can always satisfy the rules about how that kind is handled.
//
// Deliberately ONE rule rather than a suite. Filing a decision is normal.
// Answering one is normal. Doing both in a single branch is the shape with no
// honest use, because the reader has had no interval in which to see the
// question, label it, or disagree.
//
// This runs locally, in a session's own harness. It is not CI: a contributor
// never sees it and no build of vigia depends on it.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

/** Runs git and returns stdout, or null when the command cannot be run. */
function git(...args) {
	try {
		return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
	} catch {
		return null;
	}
}

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

// The publishing acts. A commit is still local and recoverable; these are the
// points where a branch becomes something the reader has to argue with.
if (
	typeof command !== "string" ||
	!/\bgh\s+(?:pr\s+(?:create|ready)|issue\s+close)\b/.test(command)
) {
	process.exit(0);
}

const base = (git("merge-base", "HEAD", "origin/main") ?? "").trim();
if (!base) process.exit(0);

// A decision row that merely CHANGES STATUS reads as an added line in a diff,
// so the two sides are compared as sets of `decision:` text. Flipping a row to
// done leaves the text identical on both sides and is not a filing.
const decisions = (text) =>
	new Set(
		(text ?? "")
			.split("\n")
			.map((line) => line.match(/decision:.*/i)?.[0]?.trim())
			.filter(Boolean),
	);

const head = decisions(git("show", "HEAD:ROADMAP.md"));
const before = decisions(git("show", `${base}:ROADMAP.md`));
const filed = [...head].filter((row) => !before.has(row));
if (filed.length === 0) process.exit(0);

const spec = git("diff", `${base}...HEAD`, "--", "SPEC.md") ?? "";
const ruled = spec
	.split("\n")
	.filter((line) => line.startsWith("+") && !line.startsWith("+++"))
	.filter((line) => /\*\*B\d+/.test(line));
if (ruled.length === 0) process.exit(0);

const show = (lines) => lines.map((l) => `    ${l.slice(0, 160)}`).join("\n");
process.stderr.write(
	`BLOCKED: this branch both filed a decision and ruled on it.\n\n` +
		`decision rows this branch introduced:\n${show(filed)}\n\n` +
		`rulings this branch added to SPEC.md:\n${show(ruled)}\n\n` +
		`A decision the reader has not seen cannot be answered in the branch that\n` +
		`invents it. The label is his. File the row, let it be labelled, and rule in\n` +
		`a second branch — or, if this started as a feature request, build the thing\n` +
		`and put the question in the report.\n\n` +
		`See .claude/skills/take-next/SKILL.md, "A decision issue is ruled first".\n`,
);
process.exit(2);
