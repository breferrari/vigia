# take-next: unattended-loop

The reader invoked `/take-next unattended-loop`. That invocation is plan approval for every pass the loop takes. Run `SKILL.md` pass after pass, until the queue is dry. **Where this file and `SKILL.md` differ, this file wins.**

## What changes in a pass

- **Step 3 does not wait, and skips plan mode.** Write the plan as usual and post it on the issue, headed "Plan (pre-approved by the reader for this run)". Then build. This replaces "Step 3 is a real stop".
- **The four stops stop the item, not the loop.** A contradiction with `SPEC.md`, a task that is two tasks, a destructive action outside the branch, or a decline or narrowing of what the reader asked for: do not build past it. If a draft PR is open, leave it open with the question at the top of its body. Record the question for the final report and take the next item. A `SPEC.md` §6 dependency or stack change is the reader's, and goes to the report the same way. This replaces "ask the question in its own message, and wait".
- **A wait does not block.** After `gh pr ready`, start the next pass in a new worktree while CI runs. While review agents run, apply each report as it arrives, then build. Merging still waits for its green run.

## Merging

- Merge a PR when the `ci` run on its **head** is green. Then remove its worktree and local branch in the same step.
- A red run stops merging. Fix it before the next pass starts.
- When `main` moves under an open PR, rebase it. On a conflict in `crates/vigia/tests/package.rs`, take `main`'s side. On a `ROADMAP.md` conflict, keep both sides' rows. Then rerun `cargo test -p vigia --test register --test package`, unless the rebase changed only `ROADMAP.md` or `CHANGELOG.md`, which needs no re-review and no local suites, per step 7.

## The queue

1. `next.sh`, in its own order.
2. When it is dry, stop. A pass that concludes an item should be declined still stops that item and reports it, per stop 4 of `SKILL.md`.
3. Re-read an item's dated reason on its issue before taking it. A reason that still holds, or an item that needs a refactor across many call sites, goes to the report with one line, and the loop moves on.
4. Read the queue again when a pass ends and before calling it dry: issues are filed while the loop runs. When nothing is eligible, tell the reader, with one line per skipped item and its reason.

## Done when

`next.sh` is dry, every item it offered is merged, skipped with a reason or waiting on the reader, and every PR of the loop is merged or reported red. Report once, per `SKILL.md` step 9, with the counts: PRs opened, merged and red, items skipped, and the questions waiting on the reader.
