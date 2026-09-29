---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to merged. Do not take part of a task, several tasks, or a survey of possible work. This file lives in the repo so that it is versioned. Keep it here.

> [!IMPORTANT]
> **Run this to the end. Plan approval in step 3 is the one routine stop.**
>
> The reader often starts the skill and leaves it alone overnight. If a step waits for an answer, the pass stalls until the reader replies. The reply often comes hours later, with the work done and nothing merged. So split **what** from **how**. Step 3 settles what gets built, and a question costs nothing there. Everything after step 3 is execution, and this file answers execution questions:
>
> - **The tools are pre-authorized.** The request to run this skill is also the request to run `/simplify`, `two-axis-review`, `/code-review` and the agents that they start.
> - **Apply review findings without asking.** Fix each finding that is worth a fix. In the PR body, list each finding that you skip, with a one-line reason. In a core area (step 6), if you cannot name why a finding is wrong, fix it. A finding that declines or narrows what the reader asked for is stop 4 below. Do not apply it as a fix.
> - **A documented choice wins.** Take it and name it in the report. A documented refusal is a reason with a date. Make sure that the reason is still true (step 3).
> - **An open choice goes to the branch that delivers what was asked.** Finish the pass and put the question in the report. Do not build less to be safe. A missing feature gives the reader nothing to review. The reader can reject an extra feature in review.
>
> Four things stop the pass, and all four are about *what*:
>
> 1. A finding contradicts `SPEC.md` (step 4).
> 2. The task is two tasks (step 3).
> 3. An action is destructive outside this branch.
> 4. You conclude that something the reader asked for must be declined or narrowed. Plan approval does not cover this. A decline inside a long plan is easy to miss. Approval of the plan is not approval of the decline. Ask the question in its own message, and wait.
>
> **An unattended session may add. It may not subtract.**
>
> Step 3 is a real stop. Present the plan and wait. Do not approve it yourself. If nobody answers, that is not approval. Do not offer "ship it as is" as an option.

## 1. Find the task

```sh
sh .claude/skills/take-next/next.sh            # the milestone to take from, then its open issues
sh .claude/skills/take-next/next.sh --ranked   # every eligible milestone in take order
sh .claude/skills/take-next/preflight.sh       # does the spec still agree with the tracker
```

`next.sh` selects the earliest eligible milestone. Each rule prevents a failure that occurred before:

- **Order is the phase number at the start of the title.** The due date of every milestone is null. A sort on a null key returns the milestones in the order that the API sent them. A title without `Phase <n>` sorts last and does not disappear. Thus a renamed milestone goes after all the others, with no warning. Pre-flight check 6 finds this.
- **A description that starts with `Shelf:` is never selected.** A shelf stays open and is never next. Take from a shelf only as a deliberate choice. First, read its dated reason for the deferral again.
- **A milestone with no open issues is skipped.** Thus a finished phase that is still open is never selected. An empty answer means that the rest is shelved, finished, or empty. Find out which before you act.

After you edit `next.sh`, `preflight.sh` or these rules, run `sh .claude/skills/take-next/selftest.sh`. When you finish the last issue in a milestone, close the milestone.

`ROADMAP.md` declares the next task and links every issue, open or closed. The tracker holds the state of each issue. If the two disagree, fix the roadmap in the same pass.

### Pre-flight

`preflight.sh` reads `SPEC.md` and `ROADMAP.md` from `origin/main`, not from the working tree. It fetches the whole board and exits non-zero on any hit in checks 1 to 4, 6 and 7. Fix each of those hits in this pass. Checks 0 and 5 are advisory: read them and act as they say. The script takes about twenty seconds.

First, the script makes sure that the whole board arrived. A truncated fetch causes false drift in check 1. It also causes checks 2, 4 and 7 to miss real drift. Then it runs these checks:

0. No soak test runs on this machine (advisory).
1. An issue title names each invariant in the spec.
2. No issue names an invariant that the spec removed.
3. The mark on each roadmap row agrees with the state of its issue, and the issue exists.
4. Each open issue has a milestone. Without one, `next.sh` never sees the issue.
5. The open bullets in `SPEC.md` §10 are printed for you to read. A bullet with ordering words (*before*, *first*, *until*, *blocked*) and no issue is a blocker. File an issue for it. Then decide whether it is in scope or goes first. Do this before you plan.
6. The answer from `next.sh` agrees with the section order in the roadmap. Each open milestone with work, except the Shelf, has a `## Phase <n>` section.
7. Each issue, open or closed, has a roadmap row.

A false positive means that the check is wrong. Fix the check. Do not learn to skip it. A command in this file can stop doing what the file says. Fix it in the pass that finds it. That is a correction, not instrument work, so the Shelf rule in step 4 does not apply.

Take the **topmost unstarted row** in the `ROADMAP.md` section of that phase. If a later task blocks it, say so and take the blocker. If a task is `🔨 in progress`, read `git status` and the open PRs first. Another session can own that task.

### Declines

A decline costs more than a build, because a missing feature gives the reader nothing to review.

- Reach a decline early. Whether the reason holds is a question of fact. After you know the reason, more time only adds words that defend the decline.
- A decline has a higher bar than a build: its reason must survive a check. A bad build is visible in review. A bad refusal is not visible, and no gate can find a feature that nobody built. "It costs a wake", "it needs a timer" and "no API reports that" are claims with answers. All three were wrong in this repo before.
- If the reader asked for the thing, build it. "Possible, affordable, but I prefer another design" is a preference, not a reason.
- Put the reason in `SPEC.md` in the fewest words that evidence can prove false. Put the evidence in `RULINGS.md`.

### A `decision` issue is ruled first and built second

The `decision` label belongs to the reader. Do not apply it, infer it, or write a roadmap row that implies it. An issue without the label is a build. If a build issue contains a question, answer the question in the report, not in `SPEC.md`. `.claude/scripts/decision-authority.mjs` refuses a branch that both files a decision and rules on it.

For an issue with the `decision` label:

- Put the ruling in the `SPEC.md` section that the issue names, with its `Ruled <date>, reader|session` line. Close its §10 bullet. A ruling that is only in the issue is not filed.
- Put the rejected option in `RULINGS.md`.
- A *yes* is half the pass. File the build as its own issue. Take it next, in its own PR. This is not stop 2, because the build is a separate issue, not a split. Size is the only reason to stop after the ruling. If the build is too big for this pass, start the report with "nothing yet" and the build issue (step 9).
- If you cannot make a ruling, write what can settle it. Leave the issue open.

## 2. Load the context

Read the issue first. For a research or look-and-feel task, next find how other tools solve the problem. Then read the `SPEC.md` sections that the issue touches and the commits that changed those sections. This order keeps the record from limiting the options.

Then query `vigil` for the three things that the repo does not hold:

- Private context, for example the competitive read.
- Lessons from other projects, for example a `gix` limitation or a measurement trap.
- History from before the code, for example why the product is monitor-class.

Know which of the three you need. Use `search` for the decision that you will change. `recall` is often empty, and an empty result means nothing. Strategic context can leak into public text that you write with it in mind. The commit guard does not find that leak.

## 3. Plan, and wait for approval

Enter plan mode. Do not write code before a person approves the plan. Step 6 compares the shipped work with this plan.

The plan contains these parts:

- **What it rests on.** List the decisions by title, from `SPEC.md` and `vigil`. List each fact that argues against the approach, and why you continue. If the record is empty, write "nothing recorded". That empty result is a finding.
- **Premises.** For each premise, write what must be true, how it can be false, and the answer with its source. The source is one of: *measured*, *read in the dependency's source*, *checked against the world*, *recorded in `SPEC.md`*, or *assumed*. A premise that the plan depends on must not stay *assumed*. Settle it before you present the plan. Read the source in `~/.cargo/registry`, write a probe, measure, or search the web. For facts about other libraries and terminals, use the world as the source, not memory. Settle premises in dependency order. Finding facts is your job, not the reader's job. Only product decisions go to the reader, in the plan. A decline is the exception, because it is stop 4.
- **Checks on the record.** Quote the words of each invariant that you cite. Make sure that they apply to this case. For example, I1's budget is *0 wakeups while idle*, and a reader who uses the pane is not idle. Two features were refused on I1, and I1 did not apply to either. Quote each refusal that you cite, with its date, and mark it checked or not checked. Make sure that its reason is still true. A reason about something missing ("no API for this") becomes false fastest. If the reason is false, the question is open again. Do not find a new reason for the same conclusion. Give each budget with its current headroom ("2.4ms of 16ms"), not alone.
- **Promises you can diff.** List the files, signatures and error codes. List the tests by name, with what each test asserts. List each deviation from `SPEC.md` with its reason, and list what is out of scope. "Fix the thing" promises nothing and passes every check. Size the list to the diff.

Measure to learn the answer. Do not run a measurement that can only support a no.

**One fresh context must hold the work.** The context must hold the issue, the spec sections, the changed files and the new tests. It must also have room to think. If it cannot, split the issue into child issues. Each child needs a full path through spec, code and gates. Mark which child blocks which. A split is stop 2. Show the split in the plan, and file the child issues after the reader approves. For a wide mechanical refactor, use expand then contract: add the new form, move the call sites in batches, then delete the old form.

Before you write code, post the approved plan as a comment on the issue. When you take a deviation, write it and its reason at that time. A reason that you write at review time does not count.

## 4. Build

- **One issue, one branch, one worktree, one PR.** Work in a worktree, not in the main checkout. The main checkout stays on `main`, so `origin/main` and the tree agree. First, find a free `../vigia.*` worktree, because its `target/` is warm. Make sure that no other session uses it. Then run `git -C <dir> checkout -B issue-<n>-<slug> origin/main`. If no worktree is free, add one with `git worktree add ../vigia.<n> -b issue-<n>-<slug> origin/main`. After the merge, remove the worktree that you added.
- **If the reader is present,** run `vigia` in a side pane on the worktree. A reader who looked at the pane found defects that eleven green gates missed.
- **A run that lasts longer than the pass** needs a comment on its issue. When the run starts, write what runs, where the output goes, and when it ends.
- **Open a draft PR early** with `gh pr create --draft`. CI and Copilot skip drafts, so a push costs nothing.
- **Write the failing test first** for each invariant. Watch it fail, then make it pass.
- **A change to the frame path runs the budget gate.**
- **Add no dependency that `SPEC.md` does not name.** First, propose it in the spec, in its own commit.
- **If reality contradicts the spec, stop (stop 1).** Say which one you think is wrong, and wait. After the reader answers, change that one in its own commit. If reality contradicts the plan, decide which is wrong. If the plan is wrong, write the deviation and its reason on the issue at that time. Then correct the plan comment. Step 6 then compares against the plan as it is now.
- **Each issue that you file gets a milestone and a roadmap row.** Fix in-scope findings in this PR. Do not move an in-scope finding to a new issue to close the PR. An out-of-scope finding goes to the Shelf. Give it a row in the Shelf table of `ROADMAP.md`. Put its dated reason in the Deferral shelf table. Use `gh issue create --title "..." --body-file f.md --milestone "Shelf"`. A defect in a gate, a check, a skill or a workflow also goes to the Shelf. If that work blocks a product pass, do it, and make it as small as the blockage. Otherwise, leave it on the Shelf.

## 5. Scope the checks

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

If the output is empty, the diff is docs-only. Then skip `cargo test`, the benches and the budget gates, and run `cargo test --test register --test package`. `Cargo.toml`, `Cargo.lock`, `.github/workflows`, `.github/scripts` and `.claude/scripts` are always code. In the PR body, write which scope you chose.

## 6. Review and prove

**The kind of change decides the review.** A human eye judges feel. Thus look-and-feel work (layout, colour, keys, chrome) runs `/simplify` and puts a screenshot in the PR. All other work runs the full sequence below. Docs-only diffs and small code diffs are the exceptions, at the end of this step. The rule goes one way only. The core areas are the frame path, the watch engine, the diff oracle, the budget gates and the invariants. If look-and-feel work touches a core area, that part runs the full sequence. Do not use the lighter review on a core area.

**Before the review, diff the result against the plan.** Mark each promise delivered or not delivered. In this pass, fix each quietly narrowed scope, each dropped case and each unused definition. If a deviation has no reason written when you took it, remove the deviation in this pass: make the code match the plan. A reason that you write now does not count.

The full sequence, in order. Apply what each step finds:

1. `/simplify`.
2. `two-axis-review` against `origin/main`. The Spec axis compares the diff with the issue and the plan comment. The Standards axis mostly repeats `/simplify`. Act only on what `/simplify` did not find.
3. `/code-review high`.
4. **Mutation check.** For each new gate, remove its fix and make sure that the gate fails. Change a copy of the file, not the worktree.

Run each tool once. A new review always finds something new, so a loop until clean never ends. Docs-only diffs run `/simplify` and `two-axis-review`, at any size. A small code diff runs `/simplify` and the mutation check. Small means under ~200 lines in 3 files or fewer, outside the core areas. If the reader asks for `/harden`, run it. Do not run it otherwise.

Give each agent a brief. Add every measurement that the reviewer needs to the brief. The brief also says: *Read the code. Do not run builds, benchmarks or tests. If a measurement is missing, name it and I will run it. Judge comments by the comment rule in `CLAUDE.md`.* Run the review agents on Sonnet.

Then prove the result:

- For a code diff, report `cargo test` green with the count. Report the budget gates with numbers against the budgets. For a docs-only diff, report `register` and `package` green. State each failure plainly.

## 7. Mark ready and merge

`gh pr ready` starts the matrix on three platforms and the Copilot review. Copilot has a quota. Mark the PR ready once, after the local suite is green and the plan diff is clean. A draft shows a green `ci complete` that ran nothing.

```sh
t=$(date -u +%Y-%m-%dT%H:%M:%SZ)
gh pr ready <n>
gh run list --branch <branch> --workflow ci --created ">=$t" --limit 1 --json databaseId,headSha --jq '.[0]'
gh run watch <id> --exit-status
gh run view <id> --json conclusion --jq .conclusion
```

Watch the run, not `gh pr checks`. List only the runs that started after the ready call. The run of the draft has the same `headSha` and a green conclusion, and it skipped every job. If the list is empty, the ready run is not in the queue yet. List again. The `headSha` of the run must be the head of the PR. Use `conclusion` as the gate, because `gh run watch` can exit 0 on a failed run. After each fix push, watch the new run the same way.

Then read the Copilot review, which nothing else watches. Find out whether the review arrived. Then read its line comments:

```sh
gh api repos/{owner}/{repo}/pulls/<n>/reviews --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]") | "\(.state)\n\(.body)"'
gh api repos/{owner}/{repo}/pulls/<n>/comments --jq '.[] | select(.user.login == "Copilot") | "\(.path):\(.line)\n\(.body)\n"'
```

When the PR becomes ready, Copilot reviews it automatically. A manual request spends a second unit of quota. After the first ready run ends, wait up to fifteen minutes for the Copilot review. If no review arrives, continue and write that in the report. Reply to every comment. Fix it, or reply with the spec section or invariant that the fix breaks. A comment without a reply reads as agreement. Copilot comments do not bind you, and you must not ignore any of them. Put all fixes in one push. If you must do more than fix review comments, run `gh pr ready <n> --undo` first. The next ready call starts the matrix again and spends Copilot quota again.

When the latest run is green on the PR head and every comment has a reply, merge with `gh pr merge <n> --squash --delete-branch`. Under a worktree, the local branch delete fails after the merge. Thus read the PR state before you try again.

## 8. Close the loop

Do items 2 and 3 on the branch before step 7, so that they merge with the PR. Do items 1 and 4 after the merge.

1. **Issue.** Close it with the commit, the test count and the numbers.
2. **`ROADMAP.md`.** Change the status of the row. Add a row for each issue that this pass filed or closed. If this pass moved an issue to the Shelf, add its rows to the Shelf and Deferral shelf tables. If it took an issue from the Shelf, add a line to the Pull-forward log.
3. **`SPEC.md`.** If the contract changed, change it in its own commit on the branch.
4. **Vault.** Use `record_work` for what happened. Use `remember` for a lesson that helps another project.

If `record_work` fails after one smaller retry, write the note by hand under `projects/vigia/notes/`. Say so in the report. Then comment the date and the dropped fields on breferrari/obsidian-mind#244. Read each write back. If a merged pass has no note that names its issue, a Stop hook blocks the session end once.

## 9. Report

1. **First line.** Write what the reader can do now that was not possible before. If nothing changed, write "nothing yet" and the issue that will change it.
2. **Second line.** Write the latest release tag and the number of merged PRs after it. If this pass is not in a release, say so.

Then write briefly: the issue that you took, what shipped with numbers, and the next task (named, not started). Add these parts:

- **Review.** What each tool and Copilot found, and what you applied or skipped.
- **Plan diff.** Every promise delivered, or the deviations.
- **What the record gave.** The recorded decisions that the work used, or none.
- **Decisions taken without asking.** One line each: the branch that you took and the branch that you did not take.
- **Pane.** What `vigia` showed that looked wrong, `none`, or `not open`.

## Writing

The PR body, the issue comments, the commits and the report follow the house rules in `CLAUDE.md`: plain words, the fact first, one paragraph per line. Write the PR body the way that `/pr` does, and set it without a wait for approval. The body says what is true now and links the plan comment. It does not repeat the review. It has one line for each tool, then one line for each skipped finding with its reason.
