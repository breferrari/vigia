---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** issue from the tracker and carry it to merged. This file lives in the repo so it is versioned.

## Running unattended

The skill is often started and left alone overnight. Plan approval in step 3 is the one routine stop. After it, everything is execution and you decide it yourself:

- **The tools are pre-authorized.** Invoking this skill is the request to run `/simplify`, `two-axis-review`, `/code-review` and the agents they spawn.
- **Apply review findings without asking.** Fix every finding you judge correct. Skip one only with a one-line reason in the PR body.
- **A documented choice wins.** Take it and name it in the report.
- **An open choice goes to the branch that delivers what was asked.** Finish the pass and put the question in the report. Building less than asked is the wrong kind of caution: a missing feature is invisible, an extra one is easy to reject.

Stop and wait for the reader only when:

1. a finding contradicts `SPEC.md` (step 4),
2. the task turns out to be two tasks (step 3),
3. an action is destructive outside this branch,
4. you conclude something the reader asked for should be declined or narrowed. Ask that on its own, in one message. Plan approval does not cover it.

Nobody answering is not approval. Never offer "ship it as is" as an option.

## 1. Find the task

```sh
sh .claude/skills/take-next/next.sh            # the milestone to take from, then its open issues
sh .claude/skills/take-next/next.sh --ranked   # every eligible milestone in take order
sh .claude/skills/take-next/preflight.sh       # does the spec still agree with the tracker
```

`next.sh` picks the earliest eligible milestone:

- **Order** is the phase number at the start of the title. A title without `Phase <n>` sorts last.
- **A description starting `Shelf:`** is never selected. Take from the shelf only as a deliberate choice, after re-reading the dated deferral reason.
- **A milestone with no open issues** is skipped. An empty answer means the rest is shelved or done: find out which before acting.

Run `sh .claude/skills/take-next/selftest.sh` after any edit to `next.sh`, `preflight.sh` or these rules. Close a milestone when you finish its last issue.

The issues are the truth and `ROADMAP.md` follows them. When they disagree, fix the roadmap in the same pass.

### Pre-flight

`preflight.sh` reads `SPEC.md` and `ROADMAP.md` from `origin/main`, fetches the whole board, and exits non-zero on any hit. Fix every hit in this pass. It checks:

0. no soak test is running on this machine (advisory),
1. every spec invariant is named by an issue title,
2. no issue names an invariant the spec dropped,
3. every roadmap row's mark matches its issue's state,
4. every open issue has a milestone (without one, `next.sh` never sees it),
5. the open `SPEC.md` §10 bullets, printed for you to read: one with ordering words (*before*, *first*, *until*, *blocked*) and no issue is a blocker to file first,
6. `next.sh`'s answer matches the roadmap's section order, and every open milestone with work has a `## Phase <n>` section,
7. every open issue has a roadmap row.

A false positive means the check is wrong: fix the check. A command in this file that no longer works is fixed in the pass that finds it.

Take the **topmost unstarted task** in the chosen phase. If a later task blocks it, say so and take the blocker. If a task is `🔨 in progress`, check `git status` and open PRs first: another session may own it.

### Declines

A decline costs more than a build, because a missing feature leaves nothing to review.

- Reach it early. Whether the reason holds is a question of fact. Once the reason is known, more time only adds justification.
- When the reader asked for the thing, build it. "Possible, affordable, and I would have designed it differently" is a preference, not a reason.
- Put the reason in `SPEC.md` in the fewest words that can be falsified. The evidence goes in `RULINGS.md`.

### A `decision` issue is ruled first and built second

The `decision` label is the reader's. Never apply it, infer it, or write a roadmap row that implies it. An unlabelled issue is a build: answer any question in it in the report, not in `SPEC.md`. `.claude/scripts/decision-authority.mjs` refuses a branch that both files a decision and rules on it.

For an issue labelled `decision`:

- The ruling goes in the `SPEC.md` section the issue names, and its §10 bullet is closed.
- The rejected option goes in `RULINGS.md`.
- A *yes* is half the pass. File the build as its own issue and take it next, in its own PR. If it is too big for this pass, the report's first line says "nothing visible has changed yet; the build is #N".
- If no ruling is possible, say what would settle it and leave the issue open.

## 2. Load the context

Read the issue, the `SPEC.md` sections it touches, and the commits that touched them. On a research or look-and-feel task, look at how other tools solve it first, then compare with the record, so the record does not set the ceiling.

Then query `vigil` for what the repo does not hold: the private context, lessons from other projects, and why the product is monitor-class. `search` the decision you are about to touch. `recall` is often empty, and that means nothing. Keep strategic context out of anything public.

## 3. Plan, and wait for approval

Enter plan mode. Write no code before a person approves the plan. The plan is what the shipped work is checked against in step 6.

The plan contains:

- **What it rests on.** Decisions by title, from `SPEC.md` and `vigil`, or "nothing recorded".
- **Premises.** What must be true, how it could be false, and the answer with its source: *measured*, *read in the dependency's source*, *checked against the world*, *recorded in `SPEC.md`*, or *assumed*. Settle every load-bearing premise yourself before presenting the plan: read the source in `~/.cargo/registry`, write a probe, measure, search the web. Check facts about other libraries and terminals against the world, never against memory. Only product decisions go to the reader.
- **Checks on the record.** Quote an invariant's own words and check they reach this case. Quote a refusal with its date and check its reason is still true today: a reason about something missing ("no API for this") goes stale fastest. Give a budget with its current headroom ("2.4ms of 16ms"), never on its own.
- **Promises you can diff.** Files, signatures, error codes, tests by name and what each asserts, deviations from `SPEC.md` with reasons, and what is out of scope.

Measure to find out, never to support a no.

**One fresh context must hold it**: the issue, the spec sections, the changed files and the new tests, with room left to think. If not, split the issue into children that each have a full path through spec, code and gates, and mark the blocker. A wide mechanical refactor goes expand then contract: add the new form, migrate call sites in batches, delete the old form.

Post the approved plan as a comment on the issue before writing code. Write down any deviation and its reason when you take it. A reason written at review time does not count.

## 4. Build

- **One issue, one branch, one worktree, one PR.** Work in a worktree, never the main checkout: `git worktree add ../vigia.<n> -b issue-<n>-<slug> origin/main`, or reuse a free `../vigia.*` worktree with `git -C <dir> checkout -B issue-<n>-<slug> origin/main`. Check it is not another session's first.
- **If the reader is present,** run `vigia` in a side pane on the worktree.
- **A run that outlives the pass** gets a comment on its issue when it starts: what runs, where output goes, when it ends.
- **Open a draft PR early** with `gh pr create --draft`. CI and Copilot skip drafts, so pushes cost nothing.
- **Failing test first** for every invariant. Watch it fail, then make it pass.
- **Frame-path changes run the budget gate.**
- **Add no dependency** that `SPEC.md` does not name. Propose it into the spec in its own commit first.
- **If reality contradicts the spec, stop.** Decide which is wrong and change that one in its own commit.
- **In-scope findings are fixed here.** An out-of-scope one gets an issue with a milestone and a roadmap row: `gh issue create --title "..." --body-file f.md --milestone "Shelf"`. A defect in a gate, check, skill or workflow goes to the Shelf. Take that kind of work only when it blocks a product pass.

## 5. Scope the checks

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

Empty output means docs-only: skip `cargo test`, benches and budget gates, and run `cargo test --test register --test package`. `Cargo.toml`, `Cargo.lock`, `.github/workflows`, `.github/scripts` and `.claude/scripts` always count as code.

## 6. Review and prove

Run these in order and apply what they find:

1. `/simplify`.
2. `two-axis-review` against `origin/main`. The Spec axis checks the diff against the issue and the plan comment. Its Standards axis mostly overlaps `/simplify`, so act only on what `/simplify` did not already cover.
3. `/code-review high`.
4. **Mutation check.** For each gate added, remove its fix and confirm the gate fails. Mutate a copy or a stash, never uncommitted work.

Run each once. A fresh review always finds something new, so rerunning until clean never ends. Run `/harden` only when the reader asks for it.

Brief every agent with: *Read the code. Do not run builds, benchmarks or tests. If you need a measurement, name it and I will run it. Judge comments by the comment rule in `CLAUDE.md`.* Run review agents on Sonnet.

Then prove it:

- `cargo test` green with the count, and budget gates with numbers against budgets. State failures plainly.
- **Look-and-feel changes** get a screenshot in the PR.
- **Diff the result against the plan.** Mark each promise delivered or not. Fix any quietly narrowed scope, dropped case or unused definition in this pass.

## 7. Mark ready and merge

Marking ready starts the three-platform matrix and Copilot's review, and Copilot has a quota. Mark ready once, when the suite is green locally and the plan diff is clean. A draft shows a green `ci complete` that ran nothing.

```sh
gh pr ready <n>
gh run list --branch <branch> --workflow ci --limit 1 --json databaseId,headSha --jq '.[0]'
gh run watch <id> --exit-status
gh run view <id> --json conclusion --jq .conclusion
```

Watch the run, not `gh pr checks`. The run's `headSha` must equal the PR head. Gate on `conclusion`, because `gh run watch` can exit 0 on a failed run.

Then read Copilot's comments:

```sh
gh api repos/{owner}/{repo}/pulls/<n>/comments --jq '.[] | select(.user.login == "Copilot") | "\(.path):\(.line)\n\(.body)\n"'
```

Never request a review on top of the automatic one. Wait up to fifteen minutes after the run settles, then proceed and note it. Answer every comment: fix it, or reply with the spec section or invariant it would break. Batch fixes into one push. For longer iteration, `gh pr ready <n> --undo`.

Merge when the run is green on the ready revision and every comment is answered: `gh pr merge <n> --squash --delete-branch`. Under a worktree the local branch delete fails after the merge lands, so check the PR state before retrying.

## 8. Close the loop

1. **Issue**: close it with the commit, test count and numbers.
2. **`ROADMAP.md`**: flip the row's status.
3. **`SPEC.md`**: only if the contract changed, in its own commit.
4. **Vault**: `record_work` for what happened. `remember` for a lesson that helps another project.

If `record_work` fails after one smaller retry, write the note by hand under `projects/vigia/notes/`, say so in the report, and comment on breferrari/obsidian-mind#244. Read every write back. A Stop hook blocks once if a merged pass has no note naming its issue.

## Writing

The PR body, the issue comments, commits and the report follow `CLAUDE.md`'s house rules: plain words, the fact first, one paragraph per line. Write the PR body with `/pr`. It says what is now true and links the plan comment. It does not replay the review. Review outcomes take one line per tool.

## 9. Report

1. **First line:** what the reader can now do that they could not before, or "nothing yet" and the issue that will change it.
2. **Second line:** the latest release tag and how many merged PRs sit after it. Say if this pass is not released.

Then, briefly: the issue taken, what shipped with numbers, the next task (named, not started), and:

- **Review:** what each tool and Copilot found, and what was applied or skipped.
- **Plan diff:** every promise delivered, or the deviations.
- **Decisions taken without asking:** one line each, the branch taken and the one not taken.
- **Pane:** what `vigia` showed that read wrong, `none`, or `not open`.
