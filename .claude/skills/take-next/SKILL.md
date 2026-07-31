---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to done. Not part of a task, not three tasks, not a survey of what could be done.

This is a reconstruction. The original lived in a global skills directory, was never version controlled, and was lost in a machine migration. It lives in the repo now for that reason. Do not move it out.

## 1. Find your place

Do not guess and do not scroll the code looking for where things stopped. One command finds the work:

```sh
# The earliest milestone that still HAS open issues, then its issues.
gh api "repos/{owner}/{repo}/milestones?state=open&sort=due_on&direction=asc" \
  --jq '[.[] | select(.open_issues > 0)][0].title'
gh issue list --state open --milestone "<that title>"
```

**Not "the earliest open milestone".** A finished phase leaves its milestone open until someone closes it, so the earliest *open* milestone can be one with zero open issues, and the query then returns nothing and the session has no work to take. That happened the first time this was tried: Phase 1 was complete, merged, and still open, so `take-next` found the plan and then found nothing in it.

**If you finish the last issue in a milestone, close the milestone.** It is the step that makes the next session's first command work.

`ROADMAP.md` is the plan; the issues are the truth. If they disagree, the issues win and the roadmap is stale, so fix the roadmap in the same pass.

### Pre-flight: does the spec still agree with the tracker?

That rule is right and nothing enforced it, so drift was only ever caught by someone happening to read both. It went wrong in **both** directions inside one hour: an issue described a design the spec had already moved past, and an issue sat open after the work it tracked had shipped.

Run this before taking a task. Three commands gather the state; you do the comparing.

```sh
git fetch -q origin

# Invariants the spec declares.
git show origin/main:SPEC.md \
  | grep -oE '^\| \*\*I[0-9]+[a-z]?\*\*' | grep -oE 'I[0-9]+[a-z]?' | sort -u

# What the tracker holds.
gh issue list --state all --limit 200 --json number,title,state,milestone

# Roadmap rows that claim a state, with the issue they claim it for.
git show origin/main:ROADMAP.md | grep -oE '^\| *(✅|🔨|⬜) *\|.*\[#[0-9]+\]'

# Open questions the spec has parked. Nothing above can see these: they are
# prose, and an unanswered one carries neither an `I<n>` token nor an issue.
git show origin/main:SPEC.md \
  | sed -n '/^## 10\./,/^## 11\./p' \
  | grep -E '^- \[ \]' | cut -c1-200

# Ordering language inside those bullets — the ones that are blockers.
git show origin/main:SPEC.md \
  | sed -n '/^## 10\./,/^## 11\./p' \
  | grep -E '^- \[ \]' \
  | grep -inE 'do (this|that|it) (before|first)|(before|after) the [a-z]+ (test|soak|work|pass)|must (happen|land|come|ship|be done)|blocked (by|on|until)|prerequisite|revisit (together|with)|stand or fall together|confirm against|until [^ ]+ (lands|ships|merges|closes)'
```

> [!note] Why that pattern is phrase-shaped and not a word list
> The obvious version greps bare `before|first|until`, and on this spec it is
> **80% false positives**: §10 is full of "first paint", "the first frame that
> draws deep", "the lines before it". A check that nags four times for every
> real hit gets skipped exactly like one that never fires, which is the failure
> this whole comparison exists to fix. Tested both directions before landing:
> against `origin/main` it returns three bullets and all three are genuinely
> actionable, and against `dbc97aa` — the last commit before [#32](https://github.com/breferrari/vigia/issues/32)
> closed — it returns the settle-margin bullet, the exact prerequisite that hid
> for two phases. Widen it only with the same two runs.

Then five comparisons. Any hit is a finding to fix **in this pass**, not a note:

1. **Untracked** — an invariant the spec declares that no issue title names.
2. **Orphan** — an issue naming an `I<n>` token the spec no longer declares. This is what catches a rename or a split that left the tracker behind.
3. **State** — a roadmap row marked `✅` whose issue is open, or a row not marked done whose issue is closed.
4. **Unfiled** — an *open* issue with **no milestone**. This looks least like drift and matters most: the query above filters *by* milestone, so an unmilestoned issue is not deprioritised, it is **invisible** and will never be returned however long it sits. Seven had accumulated before anyone noticed.
5. **Untracked prerequisite** — an open `SPEC.md` §10 bullet that **no issue names**, and above all one whose text orders work: *before*, *first*, *until*, *blocked*, *prerequisite*. Unlike the four above this wants judgement rather than a token match, so read the five to ten bullets the commands print and say which have nothing behind them. **A §10 bullet that says another task must happen first is a blocker with no tracker entry, and the task it blocks will be taken anyway** — every check above will run clean, because prose carries no `I<n>` and no `#<n>`. That is strictly worse than the unfiled case: an unmilestoned issue is at least *in* the tracker. §10 said *"narrow the settle margin… do this before the soak test"* and nothing tracked it, so [#5](https://github.com/breferrari/vigia/issues/5) sat blocked by name for **two phases** and was only caught by a session happening to read §10 while loading context. File the blocker, then decide whether it is in scope for this pass or a prerequisite to take first — but decide it before planning, not after.

> [!warning] Read `SPEC.md` and `ROADMAP.md` from `origin/main`, never the working tree
> The first run of this check read the checkout, which had a feature branch
> active, and compared branch state against the live tracker. It reported the
> roadmap as ahead of an issue when on `main` the two agreed — and **#2 was closed
> on the strength of a line that was not merged.** A check whose answer depends on
> which branch happens to be checked out is worse than no check.
>
> Comparing uncommitted state is occasionally what you want. It has to be asked
> for out loud, never the default.

Only the first two directions are cheap to eyeball; run all four anyway. A check that nags forever gets ignored exactly like one that stays silent, so if a finding is a false positive, fix the *check* here rather than learning to skip it.

**Two traps if you match these with `jq`, both of which made the first run report every invariant as drifting in both directions at once:**

- **`\b` does not work.** In a `jq` string, `\b` is the *backspace* character, so `test("\\bI1\\b")` compiles a regex containing two backspaces and matches nothing — silently, and in the "everything is broken" direction. Use an explicit class instead of escaping harder:
  ```sh
  B='(^|[^A-Za-z0-9])'; A='([^A-Za-z0-9]|$)'
  jq -e --arg i "$inv" --arg b "$B" --arg a "$A" 'any(.[]; .title | test($b+$i+$a))'
  ```
- **`jq` emits CRLF on Windows.** Its output into a shell loop carries `\r`, so `grep -qxF "$token"` fails against a clean list for every token. Pipe through `tr -d '\r'` before comparing.

The two together produce a check that is *100% false positive* while looking like a catastrophic finding. Mutate it once before you trust it: point a comparison at a token you know is tracked and confirm it comes back **clean**. A drift check that cannot report "no drift" has not been tested.

Take the **topmost unstarted task in the earliest open phase**. Do not skip ahead to something more interesting. If a later task genuinely blocks the current one, say so and take the blocker, but say it out loud first.

If a task is already `🔨 in progress`, check `git status` and the open PRs before starting anything: another session may be mid-flight, and two sessions on one task is worse than one session idle.

## 2. Load the why before touching code

**Read the repo first.** The issue carries acceptance criteria, `SPEC.md` carries the contract, and — unusually for a repo — a great deal of the *reasoning* is here too: §10's open questions carry their measurements, the invariant callouts explain why they split, and the commit messages argue rather than announce. Most "why did we do it this way" questions are answered inside the checkout.

Then reach outside it, through the `vigil` MCP server, for the three things the repo deliberately does **not** hold:

- **What cannot be public.** This repo is public. The competitive read, the market position, the objection this project was started against — none of that belongs in a file anyone can clone, and it is the reason the vault exists at all.
- **What generalises past this repo.** A `gix` limitation or a measurement trap is true for every Rust project, so it lives where other projects can reach it.
- **What predates or outlives the code.** Why this is monitor-class rather than review-class was decided before the first commit, and re-deriving it is the most expensive mistake available here.

```
search   the decision you are about to touch
recall   accumulated constraints — empty early, and empty is not evidence of none
```

**Consulting the vault is deliberate, not reflexive.** Reaching for it when the answer is in `SPEC.md` wastes a call; reaching for it out of habit while writing a *public* commit message loads strategic context that must never land in one. That direction is the one with a hook guarding it, because a squash commit merged to a public `main` stays reachable by SHA forever. Know which of the three you are asking for before you ask.

## 3. Plan it, in plan mode, before touching code

**Enter plan mode and write the plan. No code before an approved plan.** The plan is not ceremony and it is not for you — it is the only artifact the finished work can be *audited against*, and code cannot audit itself.

Without it the completeness check downstream has nothing to compare to. `/harden` carries a plan-fidelity phase that explicitly skips when no written plan exists, so a session that skips planning silently disables the one gate designed to catch under-delivery. That gate exists because a session once passed five clean audit rounds with 501 tests green and had still quietly shipped three promises short.

### The plan states what it stands on

Step 2 sends you to the record before touching code. **The plan is where you show what came back**, and it is the last point at which a contradiction is still free to fix.

Name, in the plan itself:

- the decisions this plan rests on, by title, from `search` / `recall` / `SPEC.md`
- anything you found that argues **against** the approach, and why you are proceeding regardless
- an explicit "nothing recorded on this" when the record is empty — a real finding, not a blank to skip past

Consultation at step 2 alone fires exactly once, at the moment you know least about what you will need. Naming the result here moves it to the moment you commit. A choice re-derived from scratch that contradicts a recorded one is the most expensive mistake available in this repo, and the plan is the only place it is visible while it still costs nothing.

### The plan must be diffable

Length is not the bar; **concreteness** is. Every promise has to be checkable later by reading, so write nouns, not intentions:

- modules and files touched, function signatures, types
- error codes emitted, and what emits them
- the tests that will exist, by name and by what they assert
- any deviation from `SPEC.md`, named upfront with the reason
- anything explicitly **out** of scope

"Fix the thing" is not a plan. It survives any audit precisely because it promised nothing — a plan that cannot be diffed passes the fidelity check while proving nothing, which is worse than having no plan at all, because it *looks* gated.

Scale it to the diff. A two-line fix gets a short plan; it still names the file, the assertion, and the test. A phase gets a long one.

### The plan has to outlive the session

**Comment it on the issue before implementing.** The issue is the only durable surface that exists at planning time — the PR does not exist yet, since there are no commits to open it from, so "put it in the PR body" is unexecutable here and quietly becomes "keep it in my head". Carry it into the PR body at PR time as well, where the reviewer and `/harden`'s fidelity phase will both look for it.

A plan that lives only in the conversation dies at the next compaction, and nobody but this session can ever check the work against it.

### Deviations

Reality will contradict the plan sometimes; that is normal and not a failure. The failure is deviating quietly.

**Every deviation is a defect unless its justification was written down at the moment it was taken**, naming what the plan got wrong. A reason produced at audit time, about a choice made an hour earlier, is rationalisation — the same self-serving triage this skill refuses everywhere else, and it has a one-pushback half-life. Genuine plan-vs-reality conflicts route through the rule in the next section: stop, decide which side is wrong, change *that* one deliberately, in its own commit, and say which you changed.

At the end of the pass, diff the shipment against the plan and report the result (step 6). Any deviation without a contemporaneous justification gets **corrected in this session** — not logged, not deferred, not carried into the PR as a note.

---

## 4. Ship it

- **The unit is the issue.** One issue, one branch, one PR. Splitting one issue across several PRs fragments review and reads as progress theatre. If the issue is genuinely two things, say so and split the *issue* first.
- **Never defer a finding into a new issue to get the PR closed.** If work surfaces something inside the scope of the task, fix it here. A new issue is for something genuinely out of scope, and it needs **all three** of a milestone, a `ROADMAP.md` row, and a shelf entry giving the reason it moved. Miss the milestone and the issue is **invisible**, not deprioritised: the query in step 1 filters by milestone and will never return it. Seven accumulated that way before anyone noticed, so file it in one command rather than intending to come back:

  ```sh
  gh issue create --title "..." --body-file f.md --milestone "Phase 5 — deferred findings"
  ```
- **An invariant is not landed until a test fails when it is violated.** Write the failing test first, watch it fail, then make it pass. A test that passes against broken code is worse than no test.
- **Budgets are tests.** If the task touches the frame path, the budget gate runs.
- **Do not add a dependency `SPEC.md` does not name.** Propose it into the spec, in its own commit, then use it.
- If reality contradicts the spec, **stop**. Decide which is wrong, change that one deliberately, in its own commit, and say which you changed.

## 5. Scope the checks to the diff

Running the full suite on a docs-only change wastes minutes and proves nothing.

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

Empty means docs-only: skip `cargo test`, `cargo bench` and the budget gates.

**Caveats, because this is where corner-cutting hides:** `Cargo.toml`, `Cargo.lock`, `*.yml` and anything under `.github/workflows` are **never** docs, even when changed alongside markdown. A README tweak plus a "tiny" manifest edit is a code diff. Log the scope decision in the PR body so a reviewer can challenge it.

## 6. Prove it, then say so honestly

- `cargo test` green, and name the count
- budget gates green, and quote the numbers against the budgets
- state failures plainly; a green summary over a skipped check is a lie with good manners

### Then diff the shipment against the plan

Tests prove the code does what it does. They cannot prove it does what step 3 **said** it would — the plan lives outside the code, so no test run reaches it. This is a completeness check, done by reading both sides.

Walk every promise the plan made — each module, signature, type, error code, named test, declared scope boundary — and mark it delivered or not. Then report the result out loud, including when it is clean.

Three shapes to look for, because these are the three that actually shipped once behind five clean audit rounds and 501 green tests:

- **Quietly narrowed** — the plan promised per-file `+N/−M` counts, the shipment showed paths only.
- **Quietly collapsed** — a three-value discriminated union became two.
- **Promised and absent** — an error code defined in the plan, never emitted, leaving dead code where it should have been.

Any deviation lacking a justification **written when it was taken** is a defect, and it gets fixed in this pass. Not noted in the PR, not filed as a follow-up, not explained in the report. Correcting it is the requirement; reporting it is not a substitute. A reason invented now, for a choice made an hour ago, is rationalisation — and this skill does not accept self-authored triage anywhere else either.

Then polish, and let the **diff** pick the instrument rather than your appetite:

- **Under ~200 lines across ≤3 files** — `/simplify` alone. A full audit workflow on a small diff is theatre. (`/harden` states this floor itself and will decline; the number lives there, so if the two ever disagree, harden wins.)
- **Anything larger, or anything the rest of the system stands on** — `/harden` **until dry**. It runs `/simplify` as one of its own phases, so do not run one first and then the other. It also carries its own plan-fidelity phase: tell it the diff above was already done, so it records the result instead of repeating it.

Whichever runs, pass the docs carve-out into the invocation, because `/simplify` reduces and the comments explaining *why* something works are the ones nobody can reconstruct from the code:

> Documentation is non-negotiable during `/simplify`. Do not shorten, remove, or
> fold module-header docblocks, function doc comments, or `Why:` / invariant
> notes. If a simplification would delete context about why something works, skip
> the simplification.

**"Foundational" is not a self-assessment you get to lower.** If the change touches the frame path, the watch engine, the diff oracle, or the budget gates, it is foundational — that is the whole system. Do not accept your own "not worth fixing" on a first pass either: that dismissal has a one-pushback half-life here, and three out of four have historically been wrong.

## 7. Close the loop, all four places

Skipping any of these is how the next session loses time.

1. **The issue.** Close it, with the evidence: commit, test count, numbers.
2. **`ROADMAP.md`.** Flip the status. Add to the deferral shelf or pull-forward log if anything moved.
3. **`SPEC.md`.** Only if the contract actually changed. Own commit.
4. **The vault**, through the MCP:
   - `record_work` for what happened here: changes, decisions, what was learned, what is still open, how it was verified.
   - `remember` for anything that would help someone on a **different** project. A `gix` limitation that would bite any Rust project is a `remember`. "Landed the watch engine" is a `record_work`. Both, when both are true.

## 8. Report

What was taken, what shipped, the numbers, what moved on the roadmap, and what the next task is. Then stop. Do not start it.

Include the **plan-fidelity result** explicitly — "every promise delivered", or the deviations and what was done about them. Silence here reads as clean, and a step whose absence is indistinguishable from success is a step that stops happening.

Say what the record gave you, too: which recorded decisions the work stood on, or that there were none. The same reasoning applies — a consultation nobody reports is a consultation nobody can tell you skipped.

## Anti-patterns

- Surveying the whole roadmap instead of taking one task
- Taking a later task because it looks more interesting
- Writing code before an approved plan exists
- A plan too vague to diff — it passes the fidelity check by promising nothing
- A plan that lives only in the conversation, so it dies at the next compaction
- Justifying a deviation at audit time instead of when it was taken
- Reporting a deviation instead of correcting it
- Taking a task while an open `SPEC.md` §10 bullet says something else comes first
- Closing an issue whose invariant has no failing test
- Filing a follow-up issue to avoid fixing something in scope
- Running the full suite on a markdown diff, or skipping it on a manifest diff
- Reporting green without naming what ran
- Finishing without `record_work`
