#!/usr/bin/env node
// Stop guard: a pass whose pull request has merged ends with a record in the
// vault, or is told so, once. The record is the requirement and the tool is
// only the route, so the check is on the note rather than on the call: any
// note under the project's notes that names the branch's issue counts, filed
// by hand or not.
//
// Fails OPEN on anything it cannot read. Never fires on `main`, on a branch
// whose name carries no issue number, or before that branch's pull request has
// merged, because until then there is nothing to record. Blocks at most once
// per session, so a note the vault refuses cannot hold a session hostage.
//
// Test seams, never set in a real run: RECORD_GUARD_STATE stands in for the
// pull request state `gh` would report, and VIGIL_VAULT points at a fixture.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

let input;
try {
	input = JSON.parse(readFileSync(0, "utf8"));
} catch {
	process.exit(0);
}
if (input?.stop_hook_active) process.exit(0);

const cwd = typeof input?.cwd === "string" ? input.cwd : process.cwd();
const run = (cmd, args) => {
	try {
		return execFileSync(cmd, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
	} catch {
		return null;
	}
};

const branch = run("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
if (!branch || branch === "main" || branch === "HEAD") process.exit(0);
// `issue-<n>-<slug>` is the convention and `<n>-<slug>` the older one. A number
// anywhere else in a name, such as a date, is not an issue.
const issue = /^(?:issue-)?([0-9]{1,5})(?:-|$)/.exec(branch)?.[1];
if (!issue) process.exit(0);

const state = process.env.RECORD_GUARD_STATE ?? run("gh", ["pr", "view", "--json", "state", "--jq", ".state"]);
if (state !== "MERGED") process.exit(0);

const projectDir = process.env.CLAUDE_PROJECT_DIR ?? cwd;
let project = "";
try {
	project = readFileSync(join(projectDir, ".om-project"), "utf8").trim();
} catch {
	process.exit(0);
}
const vault = process.env.VIGIL_VAULT ?? resolve(projectDir, "..", "vigil-mind");
const notes = join(vault, "projects", project, "notes");
if (!existsSync(notes)) process.exit(0);

const names = new RegExp(`#${issue}(?![0-9])`);
let recorded = false;
try {
	recorded = readdirSync(notes)
		.filter((name) => name.endsWith(".md"))
		.some((name) => names.test(readFileSync(join(notes, name), "utf8")));
} catch {
	process.exit(0);
}
if (recorded) process.exit(0);

const marker = join(tmpdir(), `${project}-record-guard-${input?.session_id ?? "session"}`);
if (existsSync(marker)) process.exit(0);
try {
	writeFileSync(marker, "");
} catch {
	process.exit(0);
}

console.error(
	`The pull request for #${issue} has merged and no note under projects/${project}/notes names #${issue}.\n` +
		`Record the work before finishing: record_work through vigil, or file the note by hand in the vault. ` +
		`This fires once per session; if the note exists elsewhere, finish and say where.`,
);
process.exit(2);
