---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to merged. Do not take part of a task, several tasks, or a survey of possible work.

**`unattended-loop`** in the arguments: read [`unattended-loop.md`](unattended-loop.md) first.

> [!IMPORTANT]
> **Run this to the end. Plan approval in step 3 is the one routine stop.**
>
> Split **what** from **how**. Step 3 settles what gets built. Everything after step 3 is execution, and this file answers execution questions:
>
> - **The tools are pre-authorized.** The request to run this skill is also the request to run `/simplify`, `/two-axis-review`, `/code-review` and the agents that they start.
> - **Apply review findings without asking.** Fix each finding that is worth a fix. In the PR body, list each finding that you skip, with a one-line reason. In a core area (step 6), if you cannot name why a finding is wrong, fix it. A finding that declines or narrows what the reader asked for is stop 4 below. Do not apply it as a fix.
> - **A documented choice wins.** Take it and name it in the report. A documented refusal is a reason with a date. Make sure that the reason is still true (step 3).
> - **An open choice goes to the branch that delivers what was asked.** Finish the pass and put the question in the report. Do not build less to be safe.
>
> Four things stop the pass, and all four are about *what*:
>
> 1. A finding contradicts `SPEC.md` (step 4).
> 2. The task is two tasks (step 3).
> 3. An action is destructive outside this branch.
> 4. You conclude that something the reader asked for must be declined or narrowed. Plan approval does not cover this. Approval of the plan is not approval of the decline. Ask the question in its own message, and wait.
>
> **An unattended session may add. It may not subtract.**
>
> Step 3 is a real stop. Present the plan and wait. Do not approve it yourself. If nobody answers, that is not approval. Do not offer "ship it as is" as an option.

## 1. Find the task

```sh
sh .claude/skills/take-next/next.sh            # the roadmap section to take from, then its open issues
sh .claude/skills/take-next/next.sh --ranked   # every eligible section in take order
sh .claude/skills/take-next/preflight.sh       # does the spec still agree with the tracker
```

`next.sh` selects the earliest eligible section of `ROADMAP.md` on `origin/main`:

- **Order is the roadmap's section order**, from the first `## Phase` heading to `## Shelf`. A section need not be a phase. Milestones are not read.
- **A section is eligible when a row not marked done names an open issue.** An empty answer means all is done.
- **A heading naming a release ("right after 1.2.0") waits for its tag on origin**, and blocks the sections after it: the answer is `waiting:`.

After you edit `next.sh`, `preflight.sh` or these rules, run `sh .claude/skills/take-next/selftest.sh`. Close a milestone when it has no open issues.

`ROADMAP.md` declares the next task and links every issue, open or closed. The tracker holds the state of each issue. If the two disagree, fix the roadmap in the same pass.

### Pre-flight

`preflight.sh` reads `SPEC.md` and `ROADMAP.md` from `origin/main`, not from the working tree. It fetches the whole board and exits non-zero on any hit in checks 1 to 4 and 6 to 8. Fix each of those hits in this pass. Checks 0 and 5 are advisory: read them and act as they say.

First, the script makes sure that the whole board arrived. Then it runs these checks:

0. No soak test runs on this machine (advisory).
1. An issue title names each invariant in the spec.
2. No issue names an invariant that the spec removed.
3. The mark on each roadmap row agrees with the state of its issue, and the issue exists.
4. Each open issue has a milestone.
5. The open bullets in `SPEC.md` §10 are printed for you to read. A bullet with ordering words (*before*, *first*, *until*, *blocked*) and no issue is a blocker. File an issue for it. Then decide whether it is in scope or goes first. Do this before you plan.
6. The roadmap and the tracker agree on which phases exist: each open milestone with work has a `## Phase` section of its title, and each phase with open rows has an open milestone. It also reports an open issue no section offers.
7. Each issue, open or closed, has a roadmap row.
8. Given an issue number, no worktree, branch or plan comment names it.

A false positive means that the check is wrong: fix it, never skip it. A command in this file that stops doing what it says is fixed in the pass that finds it, as a correction, not under step 4's filing rule.

Take the **topmost unstarted row** in the `ROADMAP.md` section `next.sh` named. If a later task blocks it, say so and take the blocker. Then run `preflight.sh <n>`: another session can hold a ⬜ row.

### Declines

- Reach a decline early.
- A decline has a higher bar than a build: its reason must survive a check. "It costs a wake", "it needs a timer" and "no API reports that" are claims with answers.
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

Read the issue first. For a research or look-and-feel task, next find how other tools solve the problem. Then read the `SPEC.md` sections that the issue touches and the commits that changed those sections.

Then query `vigil` for the three things that the repo does not hold:

- Private context, for example the competitive read.
- Lessons from other projects, for example a `gix` limitation or a measurement trap.
- History from before the code, for example why the product is monitor-class.

Know which of the three you need. Use `search` for the decision you will change. `recall` is often empty, and an empty result means nothing. Keep strategic context out of public text: the commit guard misses it.

## 3. Plan, and wait for approval

Enter plan mode. Do not write code before a person approves the plan. Step 6 compares the shipped work with this plan.

**A pass is time-boxed.** Plan to merge within 3 hours of approval. The clock runs from plan approval and does not restart after a pause; a pass past 3 hours stops, lists what is left, and merges or reports. Mutation is scoped proof, not a completion target (step 4). If the pass is not converging at 2 hours, send the coordinator a status with what is left. *Ruled 2026-10-20, reader.* User-facing: none.

The plan contains these parts:

- **What it rests on.** List the decisions by title, from `SPEC.md` and `vigil`. List each fact that argues against the approach, and why you continue. If the record is empty, write "nothing recorded". That empty result is a finding.
- **Prior art.** Before the plan, write the questions it rests on: how others solved this, what breaks it, and what the tools it touches do at the version in use. Answer each one:
  - **From a primary source:** the project's own docs at the version in use and at its latest, its source at a tag or commit, its issues and PRs, its maintainers' writing. Quote the fact, link it, pin its version. A blog, a forum answer, a search summary or an AI-written page is a lead: trace it to the original. Never memory or an older page; a difference between the version in use and the latest is itself a finding.
  - **With the case against:** for each design choice, search for its known failures (issues describing the symptom, reverts, "doesn't work with"). A design question isn't answered until a source that could have contradicted it was looked for.
  - **Tied to the plan:** each answer names the part of the plan it confirms, changes or kills.
  - **Or open, visibly:** an unanswered question is marked *unverified*, with the queries run and the sources checked, and nothing built depends on it until it is answered.

  A section that names sources without answering written questions fails. Searching is cheaper than defending a wrong design later.
- **Premises.** For each premise, write what must be true, how it can be false, and the answer with its source. The source is one of: *measured*, *read in the dependency's source*, *checked against the world*, *recorded in `SPEC.md`*, or *assumed*. A premise that the plan depends on must not stay *assumed*. Settle it before you present the plan. Read the source in `~/.cargo/registry`, write a probe, measure, or search the web. For facts about other libraries and terminals, use the world as the source, not memory. Settle premises in dependency order. Finding facts is your job, not the reader's job. Only product decisions go to the reader, in the plan. A decline is the exception, because it is stop 4.
- **Checks on the record.** Quote the words of each invariant that you cite. Make sure that they apply to this case: I1's budget is *0 wakeups while idle*, and a reader who uses the pane is not idle. Quote each refusal that you cite, with its date, and mark it checked or not checked. Make sure that its reason is still true. A reason about something missing ("no API for this") becomes false fastest. If the reason is false, the question is open again. Do not find a new reason for the same conclusion. Give each budget with its current headroom ("2.4ms of 16ms"), not alone.
- **Promises you can diff.** List the files, signatures and error codes. List the tests by name, with what each test asserts. List each deviation from `SPEC.md` with its reason, and list what is out of scope. Size the list to the diff.

Measure to learn the answer. Do not run a measurement that can only support a no.

The same holds after the plan. A claim about how something outside the repo behaves, in a question to the reader, a ruling request, a review answer or a report, carries its source, or is marked *unverified* and settled before anything is built on it. *Ruled 2026-10-20, reader.* User-facing: none.

**One fresh context must hold the work.** The context must hold the issue, the spec sections, the changed files and the new tests. It must also have room to think. If it cannot, split the issue into child issues. Each child needs a full path through spec, code and gates. Mark which child blocks which. A split is stop 2. Show the split in the plan, and file the child issues after the reader approves. For a wide mechanical refactor, use expand then contract: add the new form, move the call sites in batches, then delete the old form.

Before you write code, post the approved plan as a comment on the issue. When you take a deviation, write it and its reason at that time. A reason that you write at review time does not count.

## 4. Build

- **Runners: CONTRIBUTING.md, Running things.** A suite runs under `cargo nextest run --profile ci`, installed if missing, not `cargo test`.
- **One issue, one branch, one worktree, one PR.** Work in a worktree, not in the main checkout. The main checkout stays on `main`, so `origin/main` and the tree agree. First take a free `../vigia.*` worktree no other session uses (its `target/` is warm), and run `git -C <dir> checkout -B issue-<n>-<slug> origin/main`. If no worktree is free, add one with `git worktree add ../vigia.<n> -b issue-<n>-<slug> origin/main`. After the merge, remove the worktree that you added.
- **If the reader is present,** run `vigia` in a side pane on the worktree.
- **A run that lasts longer than the pass** needs a comment on its issue. When the run starts, write what runs, where the output goes, and when it ends.
- **Open a draft PR early** with `gh pr create --draft`.
- **Write the failing test first** for each invariant. Watch it fail, then make it pass.
- **A change to the frame path runs the budget gate.**
- **Add no dependency that `SPEC.md` does not name.** First, propose it in the spec, in its own commit.
- **If reality contradicts the spec, stop (stop 1).** Say which one you think is wrong, and wait. After the reader answers, change that one in its own commit. If reality contradicts the plan, decide which is wrong. If the plan is wrong, write the deviation and its reason on the issue at that time. Then correct the plan comment. Step 6 then compares against the plan as it is now.
- **Each issue that you file gets a milestone and a roadmap row.** Fix in-scope findings in this PR. Do not move an in-scope finding to a new issue to close the PR. An out-of-scope finding goes to the phase it belongs in. Give it a row in that phase's table, its dated reason on the issue, and `gh issue create --title "..." --body-file f.md --milestone "<phase title>"`. When none fits, add a phase after the last, with a milestone and a section. A defect in a gate, a check, a skill or a workflow goes the same way. There is no shelf. If that work blocks a product pass, do it, and make it as small as the blockage.

## 5. Scope the checks

```sh
git diff --name-only --no-renames <base>..HEAD | sh .github/scripts/change-class.sh | tail -1
```

`docs` means the diff is docs-only. Then skip the suite, the benches and the budget gates, and run the suites that CI's documents job runs: `cargo nextest run -p vigia --test package --test register --test sheet --test theme_docs` and `cargo nextest run -p vigia-core --test coverage`. `full` is code. In the PR body, write which scope you chose.

## 6. Review and prove

**The kind of change decides the review.** Look-and-feel work (layout, colour, keys, chrome) runs `/simplify` and puts a screenshot in the PR. All other work runs the full sequence below. Docs-only diffs and small code diffs are the exceptions, at the end of this step. The core areas are the frame path, the watch engine, the diff oracle, the budget gates and the invariants. If look-and-feel work touches a core area, that part runs the full sequence.

**Before the review, diff the result against the plan.** Mark each promise delivered or not delivered. In this pass, fix each quietly narrowed scope, each dropped case and each unused definition. If a deviation has no reason written when you took it, remove the deviation in this pass: make the code match the plan.

The full sequence, in order. Apply what each step finds:

1. `/simplify`.
2. `/two-axis-review` against `origin/main`. The Spec axis compares the diff with the issue and the plan comment. It also checks the plan's Prior art section: every question answered from a pinned primary source or visibly open, a case-against search for each design choice, and each answer tied to the plan. The Standards axis mostly repeats `/simplify`. Act only on what `/simplify` did not find.
3. `/code-review high`.
4. **Mutation check.** One run of about 10 mutants on the PR's own decisions, in 15 minutes or less: commit, then `.claude/scripts/mutate.mjs` with `VIGIA_TEST_CEILING=15`, a narrow command per entry, and `--limit 15`. A survivor gets a cheap test or a known-limits line, never an issue; an equivalent mutant on the PR's code gets the code deleted. No second round. On a PR that risks the engine (watch, status, renames, decode limits), the PO may ask for the `mutants` workflow before the proof packet: `gh workflow run mutants -f ref=<branch>`. It is not run by default.

Run each tool once, not until clean. Docs-only diffs run `/simplify` and `/two-axis-review`, at any size. A small code diff runs `/simplify` and the mutation check. Small means under ~200 lines in 3 files or fewer, outside the core areas. Run `/harden` only when the reader asks. A PR opened beside the pass, such as a rule change asked for mid-run, runs this step for its class too. One exception to running once: a code change committed after `/code-review` gets one `/code-review` of that change.

Give each agent a brief with every measurement it needs. The brief also says: *Read the code. Do not run builds, benchmarks or tests. If a measurement is missing, name it and I will run it. Judge comments by the comment rule in `CLAUDE.md`.* Review agents run on Sonnet.

Then prove the result:

- For a code diff, report `cargo nextest run --workspace` and `cargo test --workspace --doc` green with the counts. Report the budget gates with numbers against the budgets. For a docs-only diff, report `register` and `package` green. State each failure plainly.

## 7. Mark ready and merge

Before `gh pr ready`, put exactly one of the labels `release` or `internal` on the PR with `gh pr edit <n> --add-label <label>`. The label follows the changed paths, not what a user can see. Any path under `crates/*/src/` or `crates/*/assets/`, a dependency the binary builds with in `Cargo.toml` or `Cargo.lock`, or install (the `dist` metadata in `Cargo.toml`, `release.yml`) makes the PR `release`, a comment or a refactor included, because the binary changed. Everything else, which is process, CI, skills, `SPEC.md`, `ROADMAP.md`, tests and gates, is `internal`. Ruled 2026-09-30, reader. User-facing: none. A `release` PR carries `Release-note: <one line the user can read>` in a commit body. An `internal` PR carries `Release-note: none`. Start an `internal` title with its kind: `ci:`, `docs:`, `spec:`, `roadmap:`, `skill:`, `test:` or `chore:`. Start a `release` title with `fix:` for a defect or `feat:` for a new feature.

Before `gh pr ready`, name the commit each step 6 review read; a later code commit is reviewed first. `gh pr ready` starts the matrix, so mark the PR ready once, after the local suite is green and the plan diff is clean.

```sh
t=$(date -u +%Y-%m-%dT%H:%M:%SZ)
gh pr ready <n>
gh run list --branch <branch> --workflow ci --created ">=$t" --limit 1 --json databaseId,headSha --jq '.[0]'
gh run watch <id> --exit-status
gh run view <id> --json conclusion --jq .conclusion
```

Watch the run, not `gh pr checks`, and list only runs started after the ready call: the draft's run has the same `headSha`, a green conclusion and no jobs. An empty list means the ready run is not queued yet. The `headSha` of the run must be the head of the PR. Use `conclusion` as the gate, because `gh run watch` can exit 0 on a failed run. After each fix push, watch the new run the same way.

**After a rebase that changed only `ROADMAP.md` or `CHANGELOG.md`, with `git range-diff` showing the PR's own commits otherwise unchanged: no re-review and no local suites.** Merge as soon as `ci complete` is green on the new head. Ruled 2026-10-19, reader. User-facing: none.

No external review is configured: read and answer any review comment a person left, then, once the post-ready run is green on the head, merge with `gh pr merge <n> --squash --delete-branch`. Ruled 2026-10-19, PO for the reader: Copilot review is off on his account. User-facing: none. Under a worktree the local branch delete fails after the merge, so read the PR state before retrying.

## 8. Close the loop

Do items 2 and 3 on the branch before step 7, so that they merge with the PR. Do items 1 and 4 after the merge.

1. **Issue.** Close it with the commit, the test count and the numbers.
2. **`ROADMAP.md`.** Change the status of the row. Add a row for each issue that this pass filed or closed, in its phase. If it took an issue into an earlier phase than planned, add a line to the Pull-forward log.
3. **`SPEC.md`.** If the contract changed, change it in its own commit on the branch.
4. **Vault.** Use `record_work` for what happened. Use `remember` for a lesson that helps another project.

If `record_work` fails after one smaller retry, write the note by hand under `projects/vigia/notes/`. Say so in the report. Then comment the date and the dropped fields on breferrari/obsidian-mind#244. Read each write back. If a merged pass has no note that names its issue, a Stop hook blocks the session end once.

## 9. Report

1. **First line.** Write what the reader can do now that was not possible before. If nothing changed, write "nothing yet" and the issue that will change it.
2. **Second line.** Write the latest release tag and the number of merged PRs after it. If this pass is not in a release, say so.

Then write briefly: the issue that you took, what shipped with numbers, and the next task (named, not started). Add these parts:

- **Review.** What each tool found, and what you applied or skipped.
- **Plan diff.** Every promise delivered, or the deviations.
- **What the record gave.** The recorded decisions that the work used, or none.
- **Decisions taken without asking.** One line each: the branch that you took and the branch that you did not take.
- **Pane.** What `vigia` showed that looked wrong, `none`, or `not open`.

## Writing

The PR body, the issue comments, the commits and the report follow the house rules in `CLAUDE.md`: plain words, the fact first, one paragraph per line. Write the PR body the way that `/pr` does, on the sections of `.github/PULL_REQUEST_TEMPLATE.md`, and set it without a wait for approval. The body says what is true now and links the plan comment. It does not repeat the review. It has one line for each tool, then one line for each skipped finding with its reason.
