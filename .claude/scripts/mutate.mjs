#!/usr/bin/env node
// Mutation battery: for each mutation, apply it, run the test command, restore
// the file's exact bytes, and report whether the command went red.
//
//   node .claude/scripts/mutate.mjs battery.json -- cargo test -p vigia --test x
//
// battery.json is [{ "name", "file", "old", "new" }], `old` a literal that must
// occur exactly once in `file`. Run it from the repository root on a clean
// tree: the restore writes the saved bytes back and never checks anything out,
// so uncommitted work cannot be lost, and a clean tree makes any leftover
// change visible. Exit 0 all killed, 1 any survived, 2 aborted.

import { createHash } from "node:crypto";
import { execSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const split = process.argv.indexOf("--");
const [batteryPath] = process.argv.slice(2, split < 0 ? undefined : split);
const command = split < 0 ? [] : process.argv.slice(split + 1);
if (!batteryPath || command.length === 0) {
	console.error("usage: mutate.mjs battery.json -- <test command>");
	process.exit(2);
}

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const dirty = () => execSync("git status --porcelain", { encoding: "utf8" }).trim();
function abort(why) {
	console.error(`mutate: ${why}`);
	process.exit(2);
}

const battery = JSON.parse(readFileSync(batteryPath, "utf8"));
if (dirty()) abort("the tree is not clean; commit first, so a leftover change is visible");

// Every anchor is checked before anything runs: an anchor that matches nothing
// reads exactly like a survivor.
for (const m of battery) {
	const count = readFileSync(m.file, "utf8").split(m.old).length - 1;
	if (count !== 1) abort(`${m.name}: the anchor occurs ${count} times in ${m.file}, not once`);
}

// The caller's shell, since cmd.exe on Windows may not resolve what Git Bash does.
const shell = process.env.SHELL || true;
const test = () => spawnSync(command.join(" "), { shell, encoding: "utf8", maxBuffer: 1 << 28 });
// Red before any mutation means red for a reason no mutation caused, and a
// command that never ran is one of those.
const baseline = test();
if (baseline.status !== 0) {
	const said = `${baseline.stdout ?? ""}${baseline.stderr ?? ""}`.trim().split("\n").slice(-20).join("\n");
	abort(`the test command is red on the unmutated tree:\n${said}`);
}

let restore = null;
for (const signal of ["SIGINT", "SIGTERM"]) {
	process.on(signal, () => {
		restore?.();
		process.exit(2);
	});
}

const rows = [];
for (const m of battery) {
	const original = readFileSync(m.file);
	const before = hash(original);
	restore = () => writeFileSync(m.file, original);
	writeFileSync(m.file, original.toString("utf8").replace(m.old, () => m.new));
	if (hash(readFileSync(m.file)) === before) abort(`${m.name}: the mutation left ${m.file} unchanged`);

	const run = test();
	restore();
	restore = null;
	if (hash(readFileSync(m.file)) !== before) abort(`${m.name}: ${m.file} did not come back byte for byte`);
	if (dirty()) abort(`${m.name}: the tree is not clean after the restore:\n${dirty()}`);

	const output = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;
	const failed = [...output.matchAll(/^test (\S+) \.\.\. FAILED/gm)].map((hit) => hit[1]);
	const killed = run.status !== 0;
	rows.push({ name: m.name, verdict: killed ? "KILLED" : "SURVIVED", by: failed.join(", ") });
}

for (const row of rows) console.log(`${row.verdict.padEnd(8)}  ${row.name}${row.by ? `  (${row.by})` : ""}`);
process.exit(rows.some((row) => row.verdict === "SURVIVED") ? 1 : 0);
