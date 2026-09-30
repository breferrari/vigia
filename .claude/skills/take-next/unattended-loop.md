# take-next: unattended-loop

The reader invoked `/take-next unattended-loop`. That invocation is plan approval for every pass the loop takes. Run `SKILL.md` pass after pass, until the queue is dry. **Where this file and `SKILL.md` differ, this file wins.**

## What changes in a pass

- **Step 3 does not wait, and skips plan mode.** Write the plan as usual and post it on the issue, headed "Plan (pre-approved by the reader for this run)". Then build. This replaces "Step 3 is a real stop".
- **The four stops stop the item, not the loop.** A contradiction with `SPEC.md`, a task that is two tasks, a destructive action outside the branch, or a decline or narrowing of what the reader asked for: do not build past it. If a draft PR is open, leave it open with the question at the top of its body. Record the question for the final report and take the next item. A `SPEC.md` §6 dependency or stack change is the reader's, and goes to the report the same way. This replaces "ask the question in its own message, and wait".
- **No step blocks.** After `gh pr ready`, start the next pass in a new worktree while CI runs. While a review agent or CI runs, work: apply findings already in, build the next item, or read the queue again.

## Merging

- Merge a PR when the `ci` run on its **head** is green and step 7's Copilot comments each have a reply. Then remove its worktree and local branch in the same step.
- A red run stops merging. Fix it before the next pass starts.
- When `main` moves under an open PR, rebase it. On a conflict in `crates/vigia/tests/package.rs`, take `main`'s side. On a `ROADMAP.md` conflict, keep both sides' rows. Then rerun `cargo test -p vigia --test register --test package`.

## The queue

1. `next.sh`, in its own order.
2. When it is dry, the Shelf, product items only: work that changes what the pane does or costs. Skip items labelled `decision`, items labelled `good first issue`, items with an open PR by someone else, and gate, CI, skill and workflow work. The reader chose this scope on 2026-09-30. One exception: an item that blocks releasing work this loop merged is taken whatever its kind. Each pull gets a line in the Pull-forward log.
3. Re-read an item's dated deferral reason before taking it. A reason that still holds, or an item that needs a refactor across many call sites, goes to the report with one line, and the loop moves on.


Read the queue again before each wait and before calling it dry: issues are filed while the loop runs. When nothing is eligible, say so in one line with the reason per item, then go on with the PRs still open.

## Done when

`next.sh` is dry, every product item left on the Shelf is skipped with a reason or waiting on the reader, and every PR of the loop is merged or reported red. Report once, per `SKILL.md` step 9, with the counts: PRs opened, merged and red, items skipped, and the questions waiting on the reader.
