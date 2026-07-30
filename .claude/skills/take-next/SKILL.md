---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to done. Not part of a task, not
three tasks, not a survey of what could be done.

This is a reconstruction. The original lived in a global skills directory, was
never version controlled, and was lost in a machine migration. It lives in the
repo now for that reason. Do not move it out.

## 1. Find your place

Do not guess and do not scroll the code looking for where things stopped.

```
gh issue list --state open --milestone "<earliest open milestone>"
```

`ROADMAP.md` is the plan; the issues are the truth. If they disagree, the issues
win and the roadmap is stale, so fix the roadmap in the same pass.

Take the **topmost unstarted task in the earliest open phase**. Do not skip ahead
to something more interesting. If a later task genuinely blocks the current one,
say so and take the blocker, but say it out loud first.

If a task is already `🔨 in progress`, check `git status` and the open PRs before
starting anything: another session may be mid-flight, and two sessions on one
task is worse than one session idle.

## 2. Load the why before touching code

The issue carries acceptance criteria. `SPEC.md` carries the contract. The
reasoning behind both is outside the repo, through the `vigil` MCP server:

- `search` for the decision you are about to touch
- `recall` for accumulated constraints, remembering it is empty early and that
  empty is not evidence of no record

Read before deciding. A choice re-derived from scratch that contradicts a
recorded one is the single most expensive mistake available here.

## 3. Ship it

- **The unit is the issue.** One issue, one branch, one PR. Splitting one issue
  across several PRs fragments review and reads as progress theatre. If the issue
  is genuinely two things, say so and split the *issue* first.
- **Never defer a finding into a new issue to get the PR closed.** If work
  surfaces something inside the scope of the task, fix it here. A new issue is
  for something genuinely out of scope, and it goes on the deferral shelf in
  `ROADMAP.md` with its reason.
- **An invariant is not landed until a test fails when it is violated.** Write
  the failing test first, watch it fail, then make it pass. A test that passes
  against broken code is worse than no test.
- **Budgets are tests.** If the task touches the frame path, the budget gate runs.
- **Do not add a dependency `SPEC.md` does not name.** Propose it into the spec,
  in its own commit, then use it.
- If reality contradicts the spec, **stop**. Decide which is wrong, change that
  one deliberately, in its own commit, and say which you changed.

## 4. Scope the checks to the diff

Running the full suite on a docs-only change wastes minutes and proves nothing.

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

Empty means docs-only: skip `cargo test`, `cargo bench` and the budget gates.

**Caveats, because this is where corner-cutting hides:** `Cargo.toml`,
`Cargo.lock`, `*.yml` and anything under `.github/workflows` are **never** docs,
even when changed alongside markdown. A README tweak plus a "tiny" manifest edit
is a code diff. Log the scope decision in the PR body so a reviewer can challenge
it.

## 5. Prove it, then say so honestly

- `cargo test` green, and name the count
- budget gates green, and quote the numbers against the budgets
- state failures plainly; a green summary over a skipped check is a lie with good
  manners

Then run `/harden` **until dry** if the change is foundational. Do not accept your
own "not worth fixing" on a first pass: that dismissal has a one-pushback
half-life here, and three out of four have historically been wrong.

## 6. Close the loop, all four places

Skipping any of these is how the next session loses time.

1. **The issue.** Close it, with the evidence: commit, test count, numbers.
2. **`ROADMAP.md`.** Flip the status. Add to the deferral shelf or pull-forward
   log if anything moved.
3. **`SPEC.md`.** Only if the contract actually changed. Own commit.
4. **The vault**, through the MCP:
   - `record_work` for what happened here: changes, decisions, what was learned,
     what is still open, how it was verified.
   - `remember` for anything that would help someone on a **different** project.
     A `gix` limitation that would bite any Rust project is a `remember`. "Landed
     the watch engine" is a `record_work`. Both, when both are true.

## 7. Report

What was taken, what shipped, the numbers, what moved on the roadmap, and what
the next task is. Then stop. Do not start it.

## Anti-patterns

- Surveying the whole roadmap instead of taking one task
- Taking a later task because it looks more interesting
- Closing an issue whose invariant has no failing test
- Filing a follow-up issue to avoid fixing something in scope
- Running the full suite on a markdown diff, or skipping it on a manifest diff
- Reporting green without naming what ran
- Finishing without `record_work`
