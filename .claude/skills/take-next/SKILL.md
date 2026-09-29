---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to merged. Not part of a task, not several, not a survey of possible work. This file lives in the repo so it is versioned; keep it here.

> [!IMPORTANT]
> **Run this to the end. Plan approval in step 3 is the one routine stop.**
>
> The skill is started and left alone, often overnight. If a step waits for an answer, the pass stalls until the reader replies, often hours later, with the work done and nothing merged. So split **what** from **how**. What gets built is settled at step 3, where asking costs nothing. Everything after it is execution, and this file answers execution questions:
>
> - **The tools are pre-authorized.** Invoking this skill is the request to run `/simplify`, `two-axis-review`, `/code-review` and the agents they spawn.
> - **Apply review findings without asking.** Fix every finding worth fixing. List any you skip, with a one-line reason, in the PR body. A finding that would decline or narrow what the reader asked for is stop 4 below, not a fix.
> - **A documented choice wins.** Take it and name it in the report. A documented refusal is a reason with a date: check the reason (step 3).
> - **An open choice goes to the branch that delivers what was asked.** Finish the pass and put the question in the report. Building less is the wrong caution. A missing feature leaves nothing to review. An extra one can be rejected in review.
>
> Four things stop the pass, and all four are about *what*:
>
> 1. a finding contradicts `SPEC.md` (step 4),
> 2. the task turns out to be two tasks (step 3),
> 3. an action is destructive outside this branch,
> 4. you conclude something the reader asked for should be declined or narrowed. Plan approval does not cover this: a decline inside a long plan is easy to miss, and approving the plan does not approve the decline. Ask it on its own, in one message, and wait.
>
> **An unattended session may add. It may not subtract.**
>
> Step 3 is a real stop. Present the plan and wait. Do not approve it yourself; nobody answering is not a yes. Never offer "ship it as is" as an option.

## 1. Find the task

```sh
sh .claude/skills/take-next/next.sh            # the milestone to take from, then its open issues
sh .claude/skills/take-next/next.sh --ranked   # every eligible milestone in take order
sh .claude/skills/take-next/preflight.sh       # does the spec still agree with the tracker
```

`next.sh` picks the earliest eligible milestone. Each rule stops a failure it has had:

- **Order is the phase number at the start of the title.** Every milestone's due date is null, and a sort on a null key returns whatever the API sent first. A title without `Phase <n>` sorts last rather than vanishing, so a renamed milestone loses its turn silently; pre-flight check 6 catches that.
- **A description starting `Shelf:` is never selected.** A shelf stays open and is never next. Take from it only as a deliberate choice, after re-reading its dated deferral reason.
- **A milestone with no open issues is skipped**, so a finished phase that is still open is never chosen. An empty answer means the rest is shelved, finished, or nothing: find out which before acting.

Run `sh .claude/skills/take-next/selftest.sh` after any edit to `next.sh`, `preflight.sh` or these rules. Close a milestone when you finish its last issue.

`ROADMAP.md` declares the next task and links every issue, open or closed. The tracker holds each issue's state. When the two disagree, fix the roadmap in the same pass.

### Pre-flight

`preflight.sh` reads `SPEC.md` and `ROADMAP.md` from `origin/main`, never the working tree, fetches the whole board, and exits non-zero on any hit. Fix every hit in this pass. It takes about twenty seconds. Before comparing anything it checks the board arrived whole: a truncated fetch makes check 1 report drift that is not there and checks 2, 4 and 7 miss drift that is. Then:

0. no soak test is running on this machine (advisory),
1. every spec invariant is named by an issue title,
2. no issue names an invariant the spec dropped,
3. every roadmap row's mark matches its issue's state,
4. every open issue has a milestone (without one, `next.sh` never sees it),
5. the open `SPEC.md` §10 bullets, printed for you to read: one with ordering words (*before*, *first*, *until*, *blocked*) and no issue is a blocker: file it, then decide before planning whether it is in scope or taken first,
6. `next.sh`'s answer matches the roadmap's section order, and every open milestone with work, except the Shelf, has a `## Phase <n>` section,
7. every issue, open or closed, has a roadmap row.

A false positive means the check is wrong: fix the check, never learn to skip it. A command in this file that no longer does what it says is fixed in the pass that finds it. That is a correction, not instrument work, so the Shelf rule in step 4 does not apply to it.

Take the **topmost unstarted row** in that phase's `ROADMAP.md` section. If a later task blocks it, say so and take the blocker. If a task is `🔨 in progress`, check `git status` and open PRs first: another session may own it.

### Declines

A decline costs more than a build, because a missing feature leaves nothing to review.

- Reach it early. Whether the reason holds is a question of fact. Once the reason is known, more time only adds justification, not argument.
- A decline has a higher bar than a build. A bad build shows up in review. A bad refusal does not, and no gate can check a feature that was never built. "It would cost a wake", "it needs a timer" and "no API reports that" are claims with answers, and all three have been wrong here.
- When the reader asked for the thing, build it. "Possible, affordable, and I would have designed it differently" is a preference, not a reason.
- Put the reason in `SPEC.md` in the fewest words that can be falsified. The evidence goes in `RULINGS.md`.

### A `decision` issue is ruled first and built second

The `decision` label is the reader's. Never apply it, infer it, or write a roadmap row that implies it. An unlabelled issue is a build: answer any question in it in the report, not in `SPEC.md`. `.claude/scripts/decision-authority.mjs` refuses a branch that both files a decision and rules on it.

For an issue labelled `decision`:

- The ruling goes in the `SPEC.md` section the issue names, with its `Ruled <date>, reader|session` line, and its §10 bullet is closed. A ruling filed only in the issue is not filed.
- The rejected option goes in `RULINGS.md`.
- A *yes* is half the pass. File the build as its own issue and take it next, in its own PR. That is not stop 2: the build is a separate issue, not a split one. Size is the only reason to stop after the ruling. If it is too big for this pass, the report's first line says "nothing visible has changed yet; the build is #N".
- If no ruling is possible, say what would settle it and leave the issue open.

## 2. Load the context

Read the issue, the `SPEC.md` sections it touches, and the commits that touched them. On a research or look-and-feel task, look at how other tools solve it first, then compare with the record, so the record does not limit the options.

Then query `vigil` for the three things the repo does not hold: private context (the competitive read), lessons from other projects (a `gix` limitation, a measurement trap), and what predates the code (why monitor-class). Know which of the three you are asking for. `search` the decision you are about to touch. `recall` is often empty, and that means nothing. The commit guard catches session artifacts, not strategy, so strategic context loaded while writing public text is how it leaks.

## 3. Plan, and wait for approval

Enter plan mode. Write no code before a person approves the plan. The plan is what the shipped work is checked against in step 6.

The plan contains:

- **What it rests on.** Decisions by title, from `SPEC.md` and `vigil`. Anything found that argues against the approach, and why you proceed anyway. "Nothing recorded" when the record is empty: that is a finding, not a blank.
- **Premises.** What must be true, how it could be false, and the answer with its source: *measured*, *read in the dependency's source*, *checked against the world*, *recorded in `SPEC.md`*, or *assumed*. Settle every load-bearing premise yourself before presenting the plan: read the source in `~/.cargo/registry`, write a probe, measure, search the web. Check facts about other libraries and terminals against the world, never against memory. Work premises in dependency order. Finding facts is your job, never the reader's. Only product decisions go to the reader, in the plan.
- **Checks on the record.** Quote an invariant's own words and check they reach this case, every time: I1's budget is *0 wakeups while idle*, and nothing a reader's hand is doing is idle. Two features were refused on I1 when it did not apply. Quote a refusal with its date, mark it checked or not, and check its reason is still true today: a reason about something missing ("no API for this") goes stale fastest. When the reason collapses, the question reopens; do not find a fresh reason for the same conclusion. Give a budget with its current headroom ("2.4ms of 16ms"), never on its own.
- **Promises you can diff.** Files, signatures, error codes, tests by name and what each asserts, deviations from `SPEC.md` with reasons, and what is out of scope. "Fix the thing" promises nothing and passes any check. Scale it to the diff.

Measure to learn the answer, not to justify a refusal.

**One fresh context must hold it**: the issue, the spec sections, the changed files and the new tests, with room left to think. If not, split the issue into children that each have a full path through spec, code and gates, and mark the blocker. A wide mechanical refactor goes expand then contract: add the new form, migrate call sites in batches, delete the old form.

Post the approved plan as a comment on the issue before writing code. Write down any deviation and its reason when you take it. A reason written at review time does not count.

## 4. Build

- **One issue, one branch, one worktree, one PR.** Work in a worktree, never the main checkout. The main checkout stays on `main`, so reading `origin/main` and reading the tree never differ. Reuse a free `../vigia.*` worktree first, since its `target/` is warm: `git -C <dir> checkout -B issue-<n>-<slug> origin/main`, after checking no other session is using it. Add one only when none is free (`git worktree add ../vigia.<n> -b issue-<n>-<slug> origin/main`), and remove it after the merge.
- **If the reader is present,** run `vigia` in a side pane on the worktree. A reader looking at the pane has found defects that eleven green gates missed.
- **A run that outlives the pass** gets a comment on its issue when it starts: what runs, where output goes, when it ends.
- **Open a draft PR early** with `gh pr create --draft`. CI and Copilot skip drafts, so pushes cost nothing.
- **Failing test first** for every invariant. Watch it fail, then make it pass.
- **Frame-path changes run the budget gate.**
- **Add no dependency** that `SPEC.md` does not name. Propose it into the spec in its own commit first.
- **If reality contradicts the spec, stop.** Decide which is wrong and change that one in its own commit.
- **Every issue you file gets a milestone and a roadmap row.** In-scope findings are fixed here. Never defer an in-scope finding to a new issue to close the PR. An out-of-scope one goes to the Shelf: a row in `ROADMAP.md`'s Shelf table, and its dated reason in the Deferral shelf table: `gh issue create --title "..." --body-file f.md --milestone "Shelf"`. A defect in a gate, check, skill or workflow goes to the Shelf. Take that kind of work only when it blocks a product pass.

## 5. Scope the checks

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

Empty output means docs-only: skip `cargo test`, benches and budget gates, and run `cargo test --test register --test package`. `Cargo.toml`, `Cargo.lock`, `.github/workflows`, `.github/scripts` and `.claude/scripts` always count as code. Say in the PR body which scope you chose.

## 6. Review and prove

**The kind of change decides the review.** Look-and-feel work (layout, colour, keys, chrome) runs `/simplify` and puts a screenshot in the PR, because the judge of feel is a human eye. Everything else runs the full sequence below, except docs-only diffs (end of this step). The escalation is one way: look-and-feel work that touches the frame path, the watch engine, the diff oracle, the budget gates or an invariant takes the full sequence for that part. These are the core of the product. Do not use the lighter review on them.

The full sequence, in order, applying what each finds:

1. `/simplify`.
2. `two-axis-review` against `origin/main`. The Spec axis checks the diff against the issue and the plan comment. Its Standards axis mostly overlaps `/simplify`, so act only on what `/simplify` did not already cover.
3. `/code-review high`.
4. **Mutation check.** For each gate added, remove its fix and confirm the gate fails. Mutate a copy of the file, never the worktree.

Run each once. Docs-only diffs run `/simplify` and `two-axis-review`. A fresh review always finds something new, so rerunning until clean never ends. Run `/harden` only when the reader asks for it.

Brief every agent with: *Read the code. Do not run builds, benchmarks or tests. If you need a measurement, name it and I will run it. Judge comments by the comment rule in `CLAUDE.md`.* Run review agents on Sonnet.

Then prove it:

- `cargo test` green with the count, and budget gates with numbers against budgets. State failures plainly.
- **Diff the result against the plan.** Mark each promise delivered or not. Fix any quietly narrowed scope, dropped case or unused definition in this pass.

## 7. Mark ready and merge

Marking ready starts the three-platform matrix and Copilot's review, and Copilot has a quota. Mark ready once, when the suite is green locally and the plan diff is clean. A draft shows a green `ci complete` that ran nothing.

```sh
t=$(date -u +%Y-%m-%dT%H:%M:%SZ)
gh pr ready <n>
gh run list --branch <branch> --workflow ci --created ">=$t" --limit 1 --json databaseId,headSha --jq '.[0]'
gh run watch <id> --exit-status
gh run view <id> --json conclusion --jq .conclusion
```

Watch the run, not `gh pr checks`. List only runs created after the ready call: the draft's run has the same `headSha`, a green conclusion, and skipped every job. If the list is empty, the ready run is not queued yet; list again. The run's `headSha` must equal the PR head. Gate on `conclusion`, because `gh run watch` can exit 0 on a failed run.

Then Copilot, which nothing watches for you. Check whether its review has arrived, then read its line comments:

```sh
gh api repos/{owner}/{repo}/pulls/<n>/reviews --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]") | "\(.state)\n\(.body)"'
gh api repos/{owner}/{repo}/pulls/<n>/comments --jq '.[] | select(.user.login == "Copilot") | "\(.path):\(.line)\n\(.body)\n"'
```

Copilot reviews automatically on ready; a manual request spends a second unit of quota. Wait up to fifteen minutes after the run settles, then proceed and note it. Answer every comment: fix it, or reply with the spec section or invariant it would break. No reply counts as agreement. Copilot's comments are not binding, and none may be ignored. Batch fixes into one push. For longer iteration, `gh pr ready <n> --undo`.

Merge when the run is green on the ready revision and every comment is answered: `gh pr merge <n> --squash --delete-branch`. Under a worktree the local branch delete fails after the merge lands, so check the PR state before retrying.

## 8. Close the loop

1. **Issue**: close it with the commit, test count and numbers.
2. **`ROADMAP.md`**: flip the row's status, add a row for any issue this pass filed or closed, and add to the shelf or the pull-forward log if anything moved.
3. **`SPEC.md`**: only if the contract changed, in its own commit.
4. **Vault**: `record_work` for what happened. `remember` for a lesson that helps another project.

If `record_work` fails after one smaller retry, write the note by hand under `projects/vigia/notes/`, say so in the report, and comment on breferrari/obsidian-mind#244. Read every write back. A Stop hook blocks once if a merged pass has no note naming its issue.

## 9. Report

1. **First line:** what the reader can now do that they could not before, or "nothing yet" and the issue that will change it.
2. **Second line:** the latest release tag and how many merged PRs sit after it. Say if this pass is not released.

Then, briefly: the issue taken, what shipped with numbers, the next task (named, not started), and:

- **Review:** what each tool and Copilot found, and what was applied or skipped.
- **Plan diff:** every promise delivered, or the deviations.
- **What the record gave:** the recorded decisions the work stood on, or none.
- **Decisions taken without asking:** one line each, the branch taken and the one not taken.
- **Pane:** what `vigia` showed that read wrong, `none`, or `not open`.

## Writing

The PR body, the issue comments, commits and the report follow `CLAUDE.md`'s house rules: plain words, the fact first, one paragraph per line. Write the PR body the way `/pr` does, and set it without waiting for approval. It says what is now true and links the plan comment. It does not replay the review: one line per tool, then one line per skipped finding with its reason.
