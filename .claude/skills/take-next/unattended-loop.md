# take-next: unattended-loop

The reader invoked `/take-next unattended-loop`. That invocation is plan approval for every pass the loop takes. Run `SKILL.md` pass after pass with the changes below, until the queue is dry.

## What changes in a pass

- **Step 3 does not wait.** Write the plan as usual and post it on the issue, headed "Plan (pre-approved by the reader for this run)". Then build.
- **The four stops still stop the item, not the loop.** A contradiction with `SPEC.md`, a task that is two tasks, a destructive action outside the branch, or a decline or narrowing of what the reader asked for: do not build it. Record the question for the final report and take the next item. A `SPEC.md` §6 dependency or stack change is the reader's, and goes to the report the same way.
- **Step 7 does not block.** After `gh pr ready`, start the next pass in a new worktree while CI runs.

## Merging

- Merge a PR when the `ci` run on its **head** is green, with `gh pr merge --squash --delete-branch`. Then remove its worktree and local branch in the same step.
- Merge with a background loop per batch of open PRs: wait for the head's run, merge on success, stop on a red run or a conflict. A red run is fixed before new work starts.
- When `main` moves under an open PR, rebase it. On a conflict in `crates/vigia/tests/package.rs`, take `main`'s side, then rerun `cargo test -p vigia --test register --test package`.
- A mutation copy that shares a worktree's `target/` leaves its build behind. `touch` every mutated file in the real worktree before its next test run.

## The queue

1. `next.sh`, in its own order.
2. When it is dry, the Shelf, product items only: work that changes what the pane does or costs. Skip items labelled `decision`, items labelled `good first issue`, items with an open PR by someone else, and gate, CI, skill and workflow work. Each pull gets a line in the Pull-forward log.
3. Re-read an item's dated deferral reason before taking it. A reason that still holds, or an item that needs a refactor across many call sites, goes to the report with one line, and the loop moves on.

## Done when

`next.sh` is dry, and every product item left on the Shelf is either skipped with a reason or waiting on the reader, and every open PR of the loop is merged or reported red. Report per `SKILL.md` step 9 once, covering every PR: what shipped, what was skipped and why, and the questions waiting on the reader.
