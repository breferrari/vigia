---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to done. Not part of a task, not three tasks, not a survey of what could be done. It lives in the repo so that it is version controlled. Do not move it out.

> [!IMPORTANT]
> **Run this to the end. Plan approval at step 3 is the one sanctioned stop.**
>
> The skill is invoked and left alone, often overnight. A step that can only complete by asking a question does not complete: the answer arrives hours later with the expensive work done and nothing merged. So the line is **what** versus **how**. What gets built is settled at step 3, where a question is free. Everything after it is execution, and execution questions have answers in this file:
>
> - **The instruments are pre-authorized.** Invoking this skill is the request to run them, including the review agents `/simplify` and `/harden` spawn. Do not ask again at step 6.
> - **Where a choice is documented, take the documented one** and say so in the report. That governs *how*. A documented refusal is a conclusion from a reason, and the reason gets checked (step 3).
> - **Where a choice is open and nobody is there, take the branch that delivers what was asked, finish the pass, and put the question in the report.** "Conservative" never means "build less": under-building costs the reader the thing they asked for and leaves nothing on screen to say so, where over-building costs an afternoon they can see and reject.
>
> Four things still stop the pass, and all four are *what*-shaped: a finding that contradicts `SPEC.md` (step 4), discovering the task is two tasks (step 3), anything destructive outside this branch, and **reaching a conclusion that declines or narrows something the reader asked for**. That last one is not settled by plan approval: a decline riding inside a plan is one line in two thousand words, and approving a plan reads as *proceed*, not as *I agree not to get the thing I asked for* (#177). Ask it on its own, in one message, and wait. An unattended session may add. It may not subtract.
>
> Step 3 is a real stop. An unattended pass that reaches it presents the plan and waits. It does not self-approve, and nobody being awake is not a yes. **"Ship it as is" is never one of the options.**

## 1. Find your place

```sh
sh .claude/skills/take-next/next.sh            # the milestone to take from, then its open issues
sh .claude/skills/take-next/next.sh --ranked   # the whole take order the answer came from
sh .claude/skills/take-next/preflight.sh       # does the spec still agree with the tracker
```

The script picks the earliest **eligible** milestone. Three rules, each with the failure it exists to stop:

1. **Order is the phase number that begins the title**, because the milestone due date is null on every row here and a sort on a null key returns whatever the API sent first. A title not beginning `Phase <n>` sorts last rather than vanishing. That is cheaper than vanishing, not safe: a milestone renamed off the pattern skips its turn silently, which comparison 6 catches.
2. **A description beginning `Shelf:` is never selected.** A shelf is permanently open and never next. Mark a shelf that way and nothing else.
3. **A milestone with no open issues is not a place to look**, so a finished phase left open cannot answer. An empty answer is not an error: it means what is left is shelved, exhausted, or nothing at all. Read which before acting. Only the first leads anywhere, and taking from the shelf is a deliberate choice made after re-reading the deferral reason, which is a dated claim (#76).

`sh .claude/skills/take-next/selftest.sh` asserts all three offline in about three seconds. Run it after any edit to `next.sh`, `preflight.sh` or these rules. **If you finish the last issue in a milestone, close the milestone.**

`ROADMAP.md` is the plan; the issues are the truth. If they disagree, the issues win and the roadmap is fixed in the same pass.

### Pre-flight

`preflight.sh` reads `SPEC.md` and `ROADMAP.md` from `origin/main`, never the working tree, fetches the whole board, and exits non-zero on any mechanical hit. **Every hit is fixed in this pass**, not noted. It takes about twenty seconds, most of it two shell loops (#371). First it checks the board arrived whole, because a truncated fetch makes comparison 1 cry wolf while 2, 4 and 7 under-report. Then seven comparisons:

1. **Untracked.** An invariant the spec declares that no issue title names.
2. **Orphan.** An issue naming an `I<n>` the spec no longer declares.
3. **State.** A row marked `✅` whose issue is open, a row not marked done whose issue is closed, or a row citing an issue the tracker does not have.
4. **Unfiled.** An open issue with no milestone. It is not deprioritised, it is **invisible**: `next.sh` filters by milestone and will never return it.
5. **Untracked prerequisite.** The open `SPEC.md` §10 bullets are printed for judgement. One whose text orders work (*before*, *first*, *until*, *blocked*) with no issue behind it is a blocker no token match can see. File it, then decide whether it is in scope or is taken first, before planning.
6. **Milestone drift.** `next.sh`'s answer must be the phase `ROADMAP.md`'s section order would choose, and every open milestone with work must have a `## Phase <n>` section. This is the only comparison that checks the first command's own answer.
7. **Missing row.** An issue, any state, that the roadmap never mentions.

If a finding is a false positive, fix the check rather than learning to skip it. The same holds for this file: **a command here that no longer does what it says is fixed the moment it is found**, in the pass that found it. That is a correction, not instrument work, and the shelf rule in step 4 does not reach it.

Then take the **topmost unstarted task in the eligible phase**. Do not skip ahead. If a later task genuinely blocks the current one, say so first, then take the blocker. If a task is `🔨 in progress`, check `git status` and the open PRs before starting anything: another session may be mid-flight.

### A decline is the most expensive thing this skill can produce

Four Phase 8 passes each spent a full session and delivered a "no", and one has since been reopened because both of its reasons were false. Three rules:

- **A decline is reached early or it is not a decline.** The reason either holds under checking or it does not, and that is a question about facts, which is cheap. Hours spent after the reason is known are spent justifying, and the tell is prose getting longer while the argument does not get stronger.
- **A decline carries a higher bar than a build.** A bad build is loud; a bad refusal is silent, and no gate can see the feature that does not exist. *"It would cost a wake"*, *"it needs a timer"* and *"no API reports that"* are claims with answers, and all three have been wrong here.
- **When the reader asked for the thing, the default is build.** If the honest summary is *"possible, affordable, and I would have designed it differently"*, that is a preference. Build it.

A ruling's prose is sized to the ruling, not to the effort: the reason in the fewest words that can be falsified goes to `SPEC.md`, and the evidence trail goes to `RULINGS.md`.

### A `decision` issue is ruled first and built second, in the same pass

**The `decision` label is the reader's. A session may not apply it, infer it, or write the roadmap row that implies it.** An issue arrives labelled `decision` or it is a build, and a build that reads like a decision is a build with a question in it: answer the question in the report, not in `SPEC.md`. A branch that both files a decision and rules on it is refused at publish by `.claude/scripts/decision-authority.mjs`.

When the issue is labelled `decision`:

- **The deliverable is the ruling**, in the `SPEC.md` section the issue names, plus its §10 bullet closed. A ruling filed only in the issue is not filed.
- **The road not taken goes to `RULINGS.md`**, so the contract does not grow on a refusal.
- **A ruling of *yes* is the first half of the pass.** The build gets its own issue and its own PR, and this pass takes it next. Stopping at the ruling shipped 0.11.1 with a feature the tool had been told about and did not have (#167, #206).
- **Size is the only reason to stop after the ruling**, and then the report's first line says *"nothing the reader can see has changed yet; the build is #N"*.
- **A ruling that cannot be made is a finding.** Say what would settle it and leave the issue open with that written down.

## 2. Load the why before touching code

**Read the repo first.** The issue carries acceptance criteria, `SPEC.md` carries the contract and, unusually, most of the reasoning: §10's open questions carry their measurements and the commit messages argue rather than announce. **On a research or look-and-feel row, the world comes first**: survey outside, form the view, then diff it against the record. Reading the rulings first anchors the survey to what was already decided, which is how a document recording yesterday's ceiling becomes tomorrow's (#318, and `SPEC.md` §0 says the same).

Then the `vigil` MCP server, for the three things the repo deliberately does not hold: what cannot be public (the competitive read, the market position), what generalises past this repo (a `gix` limitation, a measurement trap), and what predates the code (why monitor-class rather than review-class).

```
search   the decision you are about to touch
recall   accumulated constraints; empty early, and empty is not evidence of none
```

Consulting the vault is deliberate, not reflexive: know which of the three you are asking for. Strategic context loaded while writing a public commit message is how it leaks, and the commit guard catches session artifacts (URLs, trailers, local paths), not strategy.

## 3. Plan it, in plan mode, before touching code

**Enter plan mode and write the plan. No code before an approved plan, and approved means a person answered.** The plan is the only artifact the finished work can be audited against: `/harden`'s plan-fidelity phase skips when no written plan exists, and a session once passed five clean audit rounds with 501 tests green while three promises short.

### The plan states what it stands on

Name, in the plan: the decisions it rests on, by title, from `search`, `recall` and `SPEC.md`; anything found that argues against the approach, and why you proceed regardless; and an explicit *"nothing recorded on this"* when the record is empty, which is a finding rather than a blank.

### The record is evidence, not authority

`SPEC.md` says what must hold now. That is not the same as every sentence in it being beyond question, and three kinds of sentence are not equally solid:

- **An invariant with a measurement behind it** is load-bearing. **Whether it reaches the case in front of you is a fresh question every time**: quote the row's own words. I1's budget is *0 wakeups while idle*, and nothing a reader's hand is doing is idle. Two features were refused on I1 and its gate could not have caught either.
- **A refusal** is a conclusion from a moment: a reason and a date, and both expire. **Re-read the reason and check whether it is still true today.** A reason naming an absence (*"no API for this"*, *"the takeover does not enable X"*) expires fastest, because dependencies add things and nobody re-reads a refusal when they do; B10 was declined for a year on a line this repo had simply not written. When the reason collapses, the question reopens. It does not get a fresh reason for the same conclusion (#123).
- **A budget** is a number chosen against a workload. **Invoked as a reason, it arrives with its current headroom**: not "this costs a wake" but "2.4ms of a 16ms frame". A budget at 19% is a budget with room, and saying so is what stops the number being used as a mood.

**Do not run a measurement whose only possible use is to justify a no.** Measuring to find out is the most valuable thing in this repo's history: 442.71ms to 8.76ms, found by checking a premise. Measuring to build a case against something a reader asked for produces a number that was never going to change the answer.

**A refusal cited in a plan is quoted, dated, and marked checked or not.** Relayed without those three, it is an unnamed premise.

### The plan names its premises, and settles the load-bearing ones itself

A premise is what everyone took for granted. Write them as a short ledger before the plan body: what must be true, how it would be falsified, and the answer with its source, one of *measured*, *read in the dependency's source*, *checked against the world*, *recorded in `SPEC.md`*, or *assumed*. **A load-bearing premise is not allowed to stay assumed.** Finding facts is this session's job and never Brenno's: read the source in `~/.cargo/registry`, write the throwaway probe, take the measurement, search the web. A premise about the outside world (*does this library expose that, do terminals honour it, what do the toolkits that solved this already use*) is checked against the outside world and never against memory, because a wrong fact about someone else's library produces a plan that is internally consistent and wrong. Work premises in dependency order.

Only a genuinely open **decision**, a judgement about what the product should be that no probe can settle, goes to Brenno, and it goes in the plan, where it is free.

### The plan must be diffable

Every promise has to be checkable later by reading: modules and files touched, signatures and types, error codes and what emits them, tests by name and by what they assert, deviations from `SPEC.md` named upfront with the reason, and what is out of scope. "Fix the thing" passes any fidelity check because it promised nothing. Scale it to the diff.

### The work has to fit one fresh context

**Could a fresh session hold this whole issue, the spec sections it touches, the files it changes and the tests it adds, and still have room to reason about it?** If not, the issue is two issues: split the **issue**, give each child a complete path through spec, code and gates, and name the one it is blocked by. The day #77 was split in three, eleven PRs merged at a sixth of the size under the same gates. The audit is a bad place to learn the scope was wrong, because by then the rounds are paid for.

The exception is a wide mechanical refactor whose blast radius fans across the codebase. Sequence it **expand then contract**: add the new form beside the old so nothing breaks, migrate call sites in batches each their own issue, delete the old form last. Do not force that shape onto ordinary work.

### The plan has to outlive the session

**Comment it on the issue before implementing**, then carry it into the PR body. A plan that lives only in the conversation dies at the next compaction.

### Deviations

Reality contradicts plans; deviating quietly is the failure. **Every deviation is a defect unless its justification was written down when it was taken.** A reason produced at audit time for a choice made an hour earlier is rationalisation. A plan-versus-reality conflict routes through step 4's rule: stop, decide which side is wrong, change that one in its own commit, and say which.

## 4. Ship it

- **The unit is the issue.** One issue, one branch, one worktree, one PR. If the issue is two things, split the issue first.
- **Work in a worktree, never the main checkout.** `../vigia.a` and `../vigia.b` exist for passes and keep their `target/` warm. Take one with `git -C ../vigia.a checkout -B issue-<n>-<slug> origin/main`. The main checkout stays parked on `main`, so "read from `origin/main`" and "what the tree shows" never become different questions.
- **If Brenno is present, `vigia` runs in a side pane on this worktree for the pass.** Six defects were found by a reader looking at the screen while eleven green gates sat over one of them. It costs nothing unattended, and step 9 says which kind of pass this was.
- **An instrument run that outlives the pass gets a start-comment on its issue** the moment it starts: what is running, where its output lands, when it ends.
- **Open the PR as a draft, early.** `gh pr create --draft` as soon as there is a branch worth pushing, with the plan in the body. `ci.yml` skips every job on a draft and Copilot does not review one, so pushes are free. Marking ready is the metered event, and step 7 owns it.
- **Never defer a finding into a new issue to get the PR closed.** In scope means fixed here. Out of scope needs all three of a milestone, a `ROADMAP.md` row and a shelf entry with the reason, filed in one command: `gh issue create --title "..." --body-file f.md --milestone "Shelf"`. Without a milestone the issue is invisible to `next.sh`.
- **A finding about the process files to the Shelf.** A defect in a gate, a check, a skill or a workflow goes there with its dated reason, never into a phase. Instrument work is taken only when a product pass is blocked by it, sized to the blockage. A wrong command in this file is the exception step 1 names.
- **An invariant is not landed until a test fails when it is violated.** Failing test first, watch it fail, make it pass.
- **Budgets are tests.** If the task touches the frame path, the budget gate runs.
- **No dependency `SPEC.md` does not name.** Propose it into the spec in its own commit, then use it.
- **If reality contradicts the spec, stop.** Decide which is wrong, change that one deliberately, in its own commit, and say which you changed.

## 5. Scope the checks to the diff

```sh
git diff --name-only <base>..HEAD | grep -vE '\.md$|^\.github/ISSUE|^LICENSE'
```

Empty means docs-only: skip `cargo test`, `cargo bench` and the budget gates. `Cargo.toml`, `Cargo.lock`, anything under `.github/workflows`, `.github/scripts` or `.claude/scripts` is **never** docs, whatever it is changed alongside. Log the scope decision in the PR body.

## 6. Prove it, then say so honestly

- `cargo test` green, with the count. Budget gates green, with the numbers against the budgets. Failures stated plainly: a green summary over a skipped check is a lie with good manners.
- **Diff the shipment against the plan.** Walk every promise and mark it delivered or not, and say the result out loud even when it is clean. Three shapes shipped behind five clean audit rounds once: **quietly narrowed** (per-file counts promised, paths shipped), **quietly collapsed** (a three-value union became two), **promised and absent** (an error code defined and never emitted). A deviation without a contemporaneous justification is corrected in this pass, not noted.

Then polish, and let the **diff** pick the instrument:

- **Under ~200 lines across ≤3 files:** `/simplify` alone. `/harden` states this floor itself and wins if the two disagree.
- **Larger, or anything the system stands on:** `/harden` until dry. It runs `/simplify` as one of its phases and carries its own plan-fidelity phase, so tell it the plan diff above is done.
- **The surface picks the bar.** Engine and invariant work hardens until dry. Look-and-feel work (layout, colour, keys, chrome) is `/simplify` plus a screenshot in the PR, because the judge of feel is a human eye. The escalation is one-way: feel work that touches the frame path, the watch or an invariant takes the engine bar for that part.
- **"Foundational" is not a self-assessment you get to lower.** The frame path, the watch engine, the diff oracle and the budget gates are the whole system. A first-pass "not worth fixing" has a one-pushback half-life here.

Both instruments spawn parallel review agents, and this invocation authorised them. **Run the agents on Sonnet.** The reviewer personas are pinned to it and the orchestrator keeps the session's model, because the fan-out is what exhausts a session's limit and a Sonnet round found the last publish blocker. Pass two things into every brief:

> Documentation is in scope for `/simplify` and is judged by the same rule as code: a comment exists where the code cannot explain itself. Keep why the obvious approach is wrong, an invariant a caller must hold, and a cost invisible at the call site. Delete restatements of the code, issue numbers, ruling ids, and any account of the change rather than the thing.

> Read the code. Do not run builds, benchmarks, test suites, or anything else that consumes the machine. Every measurement you need is in this brief. If one is missing, name it and say what it would change, and I will run it and hand it back.

Running is the orchestrator's job in both directions: one run, one consistent picture. Four agents building at once saturated this machine before anything in the loop noticed.

## 7. Mark it ready, and wait for both reviewers

**Marking ready is the one expensive action here.** `gh pr ready` fires `ready_for_review`, which wakes the matrix on three platforms and Copilot's automatic review, and Copilot is quota-limited. Mark ready once, when the work is finished, the suite is green locally and the plan diff is clean.

> [!WARNING]
> **A draft's checks prove nothing.** The jobs skip on a draft, and `ci complete` passes a draft that skipped everything, so a draft shows one green check that ran no tests. That is a gate that looks settled and proves nothing. The local suite is the only evidence until the run below has actually happened.

```sh
gh pr ready <n>
gh run list --branch <branch> --workflow ci --limit 1 --json databaseId,headSha --jq '.[0]'
gh run watch <id> --exit-status
```

Watch the **run**, not the check list: `gh pr checks --watch` has returned before the matrix started, and a PR reached mergeable without one check having run (#301). The run's `headSha` must equal the PR's head, or the checks are about an earlier revision.

**Then Copilot, which nothing watches for you.** It arrives as a review from `copilot-pull-request-reviewer[bot]`, usually `COMMENTED`, and the substance is in the line comments, which carry the login `Copilot`:

```sh
gh api repos/{owner}/{repo}/pulls/<n>/reviews --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]") | .state'
gh api repos/{owner}/{repo}/pulls/<n>/comments --jq '.[] | select(.user.login == "Copilot") | "\(.path):\(.line)\n\(.body)\n"'
```

Do not request a review before checking whether one is coming: an explicit request on top of the automatic one spends a second unit of quota. **Wait at most fifteen minutes after the run settles** (a session's number, 2026-09-03; move it freely). If nothing has arrived, proceed and say so in the report. Quota has run out before, and a wait with no exit condition is a pass that never ends.

**Every Copilot comment gets one of two visible outcomes:** fixed in the diff, or declined in a reply naming the spec section or invariant it would violate. Silence reads as agreement. Copilot is not authoritative and not dismissible.

**Iterating after ready is the expensive shape.** Batch fixes into one push. For real iteration, `gh pr ready <n> --undo` returns to draft, where CI is quiet.

**Merge** when the run is green on the ready revision and every comment is answered: `gh pr merge <n> --squash --delete-branch`. Under a worktree the local branch delete fails after the merge has landed, so check the PR state before retrying.

## 8. Close the loop, all four places

1. **The issue.** Close it with the evidence: commit, test count, numbers.
2. **`ROADMAP.md`.** Flip the status; add to the shelf or the pull-forward log if anything moved.
3. **`SPEC.md`.** Only if the contract changed. Own commit.
4. **The vault.** `record_work` for what happened here; `remember` for anything that would help a different project. Both when both are true.

The vault write is the loop's least reliable step. If `record_work` refuses after one narrower retry (title and summary first, the rest in a second call), **file the note by hand** under `projects/vigia/notes/` and say so in the report, then comment the date and the field list on breferrari/obsidian-mind#244, because a documented workaround suppresses the bug report. After any success, read the note back: a success return is not evidence of a clean write. A Stop hook refuses, once, to end a session whose merged pass has no note naming its issue.

## 9. Report

**The first line is what a reader can now do that they could not before**, in the tool's own terms, or *nothing yet* naming the issue that will change it. No gate can check this: a pass once ended green, complete against its plan, and released a version in which nothing on screen had changed.

**The second line is the release.** The latest tag, and how many merged PRs sit after it. If this pass's work is not in a release, say so: the reader has believed a feature shipped when it sat 22 commits behind the last tag.

Then what was taken, what shipped, the numbers, what moved on the roadmap, and the next task, named and not started. And, each under its own heading:

- **Review outcome.** Whether Copilot commented, how many, and what happened to each.
- **Plan fidelity.** "Every promise delivered", or the deviations and what was done about them.
- **Decisions taken without asking.** One line each, the branch chosen and the one not taken. This is the half that makes not stopping safe.
- **What the record gave.** The recorded decisions the work stood on, or that there were none.
- **`vigia observations`.** What the pane showed that read wrong, or `none`, or `pane not open, unattended pass` (#72).
