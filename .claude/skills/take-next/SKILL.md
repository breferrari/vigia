---
name: take-next
description: Take the next task from ROADMAP.md and ship it end to end. Use when starting work with no specific task named, or when the user says "take next", "next task", "what's next and do it", or "keep going". Enforces one task per pass, a failing test per invariant, and a recorded trail.
---

# take-next

Take **one** task from `ROADMAP.md` and carry it to done. Not part of a task, not three tasks, not a survey of what could be done.

This is a reconstruction. The original lived in a global skills directory, was never version controlled, and was lost in a machine migration. It lives in the repo now for that reason. Do not move it out.

> [!IMPORTANT]
> **Run this to the end. After the plan is approved, do not stop to ask.**
>
> This skill is invoked and then left alone, frequently overnight. **A step that
> can only complete by asking a question does not complete** — it halts, and the
> answer arrives hours later with all the expensive work already done and
> nothing merged. That has happened: a pass finished 372 green tests, a clean
> plan diff, 17 killed mutants and before/after budgets, then stopped at the
> audit to ask permission for something this file already mandates.
>
> The line is **what** versus **how**. What gets built is settled at step 3 and
> the plan is where a question belongs, because it is free there. Everything
> after it is execution, and execution questions have answers in this file:
>
> - **The instruments are pre-authorized.** Invoking this skill *is* the request
>   to run them, including the parallel review agents `/simplify` and `/harden`
>   spawn. A standing "do not spawn agents unless asked" is satisfied by the
>   invocation; it is not a second gate to clear at step 6. See that step.
> - **Where a choice is documented, take the documented one** and say so in the
>   report. Do not offer it as a menu.
> - **Where a choice is genuinely open and nobody is there, take the more
>   conservative branch, finish the pass, and put the question in the report.**
>   A finished pass with a flagged decision beats an unfinished one with a
>   pending question, because the flag survives and the prompt does not.
>
> Three things still stop the pass, and they are all *what*-shaped: a finding
> that contradicts `SPEC.md` (step 4 says stop and decide which is wrong),
> discovering the task is really two tasks, and anything destructive or
> irreversible outside this branch. Those are worth waiting for. Nothing else is.
>
> **"Ship it as is" is never one of the options.** It is the self-authored
> triage step 6 refuses, wearing a question mark.

## 1. Find your place

Do not guess and do not scroll the code looking for where things stopped. One command finds the work:

```sh
# The earliest ELIGIBLE milestone that still HAS open issues, then its issues.
gh api "repos/{owner}/{repo}/milestones?state=open&per_page=100" --jq '
  [ .[]
    | select(.open_issues > 0)
    | select((.description // "") | startswith("Shelf:") | not)
    | { order: (((.title | [scan("^Phase +([0-9]+)")[]] | first) // "9999") | tonumber), title: .title }
  ] | sort_by(.order, .title) | .[0].title // empty'
gh issue list --state open --milestone "<that title>"
```

**Not "the earliest open milestone".** A finished phase leaves its milestone open until someone closes it, so the earliest *open* milestone can be one with zero open issues, and the query then returns nothing and the session has no work to take. That happened the first time this was tried: Phase 1 was complete, merged, and still open, so `take-next` found the plan and then found nothing in it.

**And not "the earliest" by whatever the API hands back, either.** That query used to sort on `due_on`, and **every milestone here has none** — sorting a set on a key that is null for every member does not define an order, so `[0]` was whichever one the API happened to return first. It gave the right answer by luck while there were two open milestones and the lower number was the one to take. The Phase 4 re-housing ended that: number order and execution order came apart, and the shelf, which must **never** be selected, stopped being separable from the phases by number alone — it sat between Phase 4 and Phase 6 while all three had work, and it is below both survivors now that Phase 4 is closed. **What it actually cost is one occurrence and one near miss**, which is worth stating precisely because the case for the fix does not need more: the [#66](https://github.com/breferrari/vigia/issues/66) session recorded that step 1 returned the wrong phase and shipped Phase 4 work regardless, so the wrong answer was caught; the pass that fixed this was handed `Phase 5` outright and caught it only by reading the roadmap prose the query cannot see. Neither was caught by anything automatic, and that is the finding — not a tally.

Three rules, so that the next milestone added inherits them rather than the accident:

1. **The order is the phase number in the title.** It is deterministic and it needs no metadata, where `due_on` was the same shape `SPEC.md` §7 keeps finding one domain over: an instrument that looks settled and proves nothing, because a sort key that is null on every row reads exactly like an order and defines none. A title not *beginning* `Phase <n>` sorts **last** rather than being dropped, and the thing holding that line is `// "9999"`, not the choice of `scan`: with the fallback in place `capture` behaves identically, and it is only with the fallback removed that the two come apart. That is the reason `scan` is still the right one to write. Delete the fallback and `scan` fails **loudly** (`null cannot be parsed as a number`) while `capture` silently keeps 2 milestones of 3 — so the form that survives a future edit badly is the one to leave behind. Ties break on the title, so two milestones sharing a number still have a defined order. **Sorting last is a cheaper failure than vanishing, not a safe one:** a milestone renamed off `Phase <n>` still skips its turn silently, which is why comparison 6 below exists.
2. **A milestone whose description begins with `Shelf:` is never selected.** A shelf is permanently open and never "next", and until #83 that was a fact only prose knew, so no query could act on it. Mark one by starting its description with `Shelf:` — [Phase 5](https://github.com/breferrari/vigia/milestone/5) reads `Shelf: never next. …` — and leave the marker off anything meant to be taken in sequence.
3. **An empty result is not an error, and it is not the paragraph above either.** Line 56's "returns nothing" is a milestone left open with zero open issues, which `select(.open_issues > 0)` now discards on its own. Empty *here* means every eligible milestone was discarded that way and what is left is shelved, exhausted, or nothing at all — so read which before acting, because the three want different things and only the first leads anywhere. Shelved work remaining is the point at which taking from the shelf becomes a deliberate choice rather than a default, and the deferral reason is re-read first, because a reason is a dated claim ([#76](https://github.com/breferrari/vigia/issues/76)). Drop the trailing `| .[0].title // empty` to see the ranked list the answer came from; one bare title is the answer without any of its evidence.

**All three rules are asserted by `sh .claude/skills/take-next/selftest.sh`**, which is offline, needs only `jq`, and takes a second. Run it after any edit to the query or to the rules beside it. It exists because the first two edits to this command were verified by hand into a GitHub comment, which is verification that dies with the shell it ran in, and because the rationale in rule 1 was **wrong** in its first version and nothing would have caught that. It also fails if the filter here and the filter it tests drift apart, in either direction.

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

> [!NOTE]
> **Why that pattern is phrase-shaped and not a word list**
>
> The obvious version greps bare `before|first|until`, and on this spec it is
> **80% false positives**: §10 is full of "first paint", "the first frame that
> draws deep", "the lines before it". A check that nags four times for every
> real hit gets skipped exactly like one that never fires, which is the failure
> this whole comparison exists to fix. Tested both directions before landing:
> against `origin/main` it returns three bullets and all three are genuinely
> actionable, and against `dbc97aa` — the last commit before [#32](https://github.com/breferrari/vigia/issues/32)
> closed — it returns the settle-margin bullet, the exact prerequisite that hid
> for two phases. Widen it only with the same two runs.

Then six comparisons. Any hit is a finding to fix **in this pass**, not a note:

1. **Untracked** — an invariant the spec declares that no issue title names.
2. **Orphan** — an issue naming an `I<n>` token the spec no longer declares. This is what catches a rename or a split that left the tracker behind.
3. **State** — a roadmap row marked `✅` whose issue is open, or a row not marked done whose issue is closed.
4. **Unfiled** — an *open* issue with **no milestone**. This looks least like drift and matters most: the query above filters *by* milestone, so an unmilestoned issue is not deprioritised, it is **invisible** and will never be returned however long it sits. Seven had accumulated before anyone noticed.
5. **Untracked prerequisite** — an open `SPEC.md` §10 bullet that **no issue names**, and above all one whose text orders work: *before*, *first*, *until*, *blocked*, *prerequisite*. Unlike the four above this wants judgement rather than a token match, so read the five to ten bullets the commands print and say which have nothing behind them. **A §10 bullet that says another task must happen first is a blocker with no tracker entry, and the task it blocks will be taken anyway** — every check above will run clean, because prose carries no `I<n>` and no `#<n>`. That is strictly worse than the unfiled case: an unmilestoned issue is at least *in* the tracker. §10 said *"narrow the settle margin… do this before the soak test"* and nothing tracked it, so [#5](https://github.com/breferrari/vigia/issues/5) sat blocked by name for **two phases** and was only caught by a session happening to read §10 while loading context. File the blocker, then decide whether it is in scope for this pass or a prerequisite to take first — but decide it before planning, not after.
6. **Milestone drift** — the phase step 1 chose must be the phase `ROADMAP.md` would have chosen, and every open milestone with open issues must have a `## Phase <n>` section. This is the only comparison that checks *this file's own first command*, and it exists because every other one ran clean on the pass that [#83](https://github.com/breferrari/vigia/issues/83) fixed while step 1 was handing that session the shelf. Step 1's answer is now correct **by construction**; comparison 6 is what makes it correct **by evidence**, and the two are not the same claim, which is the distinction §10 draws between a gate that fires and a budget met at its own window.

```sh
git show origin/main:ROADMAP.md | grep -oE '^## Phase [0-9]+.*' | sed 's/^## //' | tr -d '\r' > /tmp/order.txt
gh api "repos/{owner}/{repo}/milestones?state=open&per_page=100" > /tmp/ms.json
jq -r '.[] | select(.open_issues > 0) | .title' /tmp/ms.json | tr -d '\r' > /tmp/withwork.txt
jq -r '.[] | select(.open_issues > 0) | select((.description // "") | startswith("Shelf:")) | .title' /tmp/ms.json | tr -d '\r' > /tmp/shelved.txt

# What ROADMAP.md's section order says, which is the authority on sequence.
while IFS= read -r s; do
  grep -qxF "$s" /tmp/withwork.txt || continue
  grep -qxF "$s" /tmp/shelved.txt && continue
  echo "roadmap says: $s"; break
done < /tmp/order.txt

# Open milestones with work that no ROADMAP section names.
LC_ALL=C sort /tmp/withwork.txt > /tmp/withwork.sorted
LC_ALL=C sort /tmp/order.txt > /tmp/order.sorted
LC_ALL=C comm -23 /tmp/withwork.sorted /tmp/order.sorted
```

**Two ways it fires, and each catches a different silent failure.** A *disagreement* between step 1's answer and the roadmap's means the `Shelf:` marker was edited off, or gained a leading space, or a phase was renumbered — the marker is remote free text with none of this repo's controls over it, and it is the load-bearing input the diff cannot show you. An *orphan* means a milestone was renamed off `Phase <n>` and now sorts to the 9999 bucket, so its turn is skipped without anything saying so, or a new milestone was created with no roadmap section at all. Mutation-tested on all four before landing, plus the unchanged case, because a check that cannot report "no drift" has not been tested.

**`tr -d '\r'` is not optional here**, and it is the trap two bullets below rather than a new one: `jq` emits CRLF on Windows, `grep -qxF` then fails against every line, and the comparison reports the entire board as orphaned. It did exactly that on the first run of this check.

> [!WARNING]
> **Read `SPEC.md` and `ROADMAP.md` from `origin/main`, never the working tree**
>
> The first run of this check read the checkout, which had a feature branch
> active, and compared branch state against the live tracker. It reported the
> roadmap as ahead of an issue when on `main` the two agreed — and **#2 was closed
> on the strength of a line that was not merged.** A check whose answer depends on
> which branch happens to be checked out is worse than no check.
>
> Comparing uncommitted state is occasionally what you want. It has to be asked
> for out loud, never the default.

Only the first two directions are cheap to eyeball; run all six anyway. A check that nags forever gets ignored exactly like one that stays silent, so if a finding is a false positive, fix the *check* here rather than learning to skip it.

**Three traps if you match these with `jq`. The first two made the first run report every invariant as drifting in both directions at once; the third silently halves a comparison rather than breaking it:**

- **`\b` does not work.** In a `jq` string, `\b` is the *backspace* character, so `test("\\bI1\\b")` compiles a regex containing two backspaces and matches nothing — silently, and in the "everything is broken" direction. Use an explicit class instead of escaping harder:
  ```sh
  B='(^|[^A-Za-z0-9])'; A='([^A-Za-z0-9]|$)'
  jq -e --arg i "$inv" --arg b "$B" --arg a "$A" 'any(.[]; .title | test($b+$i+$a))'
  ```
- **`jq` emits CRLF on Windows.** Its output into a shell loop carries `\r`, so `grep -qxF "$token"` fails against a clean list for every token. Pipe through `tr -d '\r'` before comparing. Comparison 6 hit this on its own first run and reported every milestone as orphaned, which is what the bullet above predicts and is still what it looks like from the outside: a catastrophic finding.
- **`--paginate` does not compose with `--jq`.** `gh api --paginate --jq` runs the filter **once per page** and concatenates the outputs, so a filter that sorts and takes `.[0]` emits one answer per page rather than one answer. Verified against this repository at `per_page=2`: two lines came back, `Phase 6` and `Phase 7`, which is the ambiguity step 1 exists to remove, reintroduced by the flag that looks like the careful choice. `--slurp` is refused outright alongside `--jq`. So step 1 uses `per_page=100` and no `--paginate`, and it stays that way until something needs more than 100 milestones, at which point the fix is to page and sort in the shell, not to add the flag.

The first two together produce a check that is *100% false positive* while looking like a catastrophic finding. Mutate it once before you trust it: point a comparison at a token you know is tracked and confirm it comes back **clean**. A drift check that cannot report "no drift" has not been tested.

Take the **topmost unstarted task in the earliest *eligible* phase** — eligible in the sense the three rules above give it, not "the earliest open milestone", which is the phrasing this section opens by rejecting twice. Do not skip ahead to something more interesting. If a later task genuinely blocks the current one, say so and take the blocker, but say it out loud first.

If a task is already `🔨 in progress`, check `git status` and the open PRs before starting anything: another session may be mid-flight, and two sessions on one task is worse than one session idle.

### A `decision` issue is done when the ruling is written, not when code lands

Check the label before planning. An issue labelled **`decision`** is one whose acceptance is **a ruling recorded in `SPEC.md`**, not a diff. Three exist today ([#74](https://github.com/breferrari/vigia/issues/74), [#50](https://github.com/breferrari/vigia/issues/50), [#89](https://github.com/breferrari/vigia/issues/89)) and they were already written that way — #74's exit criteria read *"a ruling: build the seam, or record that direct consumption is accepted"*, and *"if declined: a line in `SPEC.md` §6 saying so, so the next reviewer finds a decision instead of an omission."*

**They had no route, which is the actual defect.** Every step below assumes a diff: plan a build, ship it, scope the checks to the changed files, prove it with gates. Handed a `decision` issue, this skill plans a build for something whose answer might be *"do not build it"* — and #50 sits in a phase that gets **taken in sequence**, so this is reachable rather than hypothetical.

When the issue is labelled `decision`:

- **The deliverable is the ruling and its reasoning**, written where the next reader will hit it — the `SPEC.md` section the issue names, plus its §10 bullet closed. A ruling filed only in the issue is not filed: the issue closes and `SPEC.md` still reads as an omission.
- **Both branches get written.** *"Declined"* is a result, and the case for the road not taken is recorded rather than dropped, because it will be raised again. #50 says this in its own acceptance and it is the general rule.
- **Code is allowed but it is not the point.** If the ruling is *build the seam*, the build is a **separate issue** the ruling unblocks. Do not fold it in: the ruling is what the next session needs and it should not wait behind an implementation.
- **The gate is different.** There is no diff to scope checks to and nothing for `/harden` to audit, so step 5's docs-vs-code split resolves to docs and the fidelity check in step 6 runs against the *plan's promises about the ruling*: every question the issue asked is answered, and each answer names what it rests on.
- **A ruling that cannot be made is a finding, not a failure.** If it needs a measurement nobody has run or a week of use nobody has had, say so, record what would settle it, and leave the issue open with that written down. #50 needs exactly that, and pretending otherwise produces a guess wearing a ruling's clothes.

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

### The plan names its premises, and settles the load-bearing ones itself

The section above records what the plan **stands on**: decisions already written down. This one is about what it **assumes**, which is the part nothing has ever checked.

A premise is a claim that must be true for this to be the right plan at all. It is not a decision, because nobody made it — it is what everyone took for granted. Write them as a short ledger before the plan body:

- **What must be true** for this approach to be correct.
- **How it would be falsified** — the observation that would end the plan.
- **The answer and where it came from**: measured, read in the dependency's source, recorded in `SPEC.md`, or *assumed*.

**A premise the plan is load-bearing on is not allowed to stay `assumed`. Go and find out.** Finding facts is this session's job and never Brenno's: read the source, write the throwaway probe, take the measurement. A question a probe can answer is not a question for the report, and it is certainly not one to leave for an empty room at 3am.

Work them in dependency order. A premise whose answer depends on another still-open one is a *later* question, not a parallel one — settle the first, then ask the second with the answer in hand.

**Why this exists**, three occasions from this repo's own notes:

- A row-exact scrollbar was refused as unaffordable, citing I4 and a prior issue as precedent. Brenno pushed back twice and asked for it to be investigated anyway. The invariant was real and had been applied to the wrong operation — counting is not building — and the correct path measured **442.71ms to 8.76ms**. The premise, *"a total requires every file diffed"*, was never written down as a premise, so nothing was in a position to challenge it.
- An issue's entire premise was wrong in a useful direction, and the note records it as **the third time** an expensive-looking property turned out to be cheap.
- The measurement that mattered most turned out to be for the implementation that was **not** chosen. Nothing asked for it; it surfaced by accident.

Each was cheap to check before the work and expensive after. **A plan that cannot be wrong about anything has not named its premises.**

Only a genuinely open **decision** — a judgement about what the product should be, which no probe can settle — goes to Brenno, and it goes *in the plan*, where it is free. That is the same line the header already draws between *what* and *how*.

### The plan must be diffable

Length is not the bar; **concreteness** is. Every promise has to be checkable later by reading, so write nouns, not intentions:

- modules and files touched, function signatures, types
- error codes emitted, and what emits them
- the tests that will exist, by name and by what they assert
- any deviation from `SPEC.md`, named upfront with the reason
- anything explicitly **out** of scope

"Fix the thing" is not a plan. It survives any audit precisely because it promised nothing — a plan that cannot be diffed passes the fidelity check while proving nothing, which is worse than having no plan at all, because it *looks* gated.

Scale it to the diff. A two-line fix gets a short plan; it still names the file, the assertion, and the test. A phase gets a long one.

### The work has to fit one fresh context, and this is where you find out

The section above sizes the *plan*. This one sizes the *work*, and it is the check this skill was missing.

**Before the plan is approved, ask: could a fresh session hold this whole issue — the spec sections it touches, the files it changes, the tests it adds — and still have room left to reason about it?** Not "can it be described in one plan", which is always yes. Can it be *held*.

If the honest answer is no, the issue is two issues. Say so now and split the **issue**. *The unit is the issue* still holds — one issue, one branch, one PR — because you are splitting the unit, not fragmenting one unit across PRs. Give each child a complete path through spec, code and gates that is verifiable on its own, and name the one it is blocked by.

**This is not a style preference.** One week in this repo:

| Day | PRs merged | avg additions |
|---|---|---|
| 08-01 | 2 | 2394 |
| 08-02 | 2 | 3379 |
| 08-03 — the day [#77](https://github.com/breferrari/vigia/issues/77) was split in three | **11** | **531** |

Five times the throughput at a sixth of the size, same standard, same gates. And #77 is what finding out late costs: its audit went 22 findings then 23, flat, which `/harden` reads as *the scope is wrong, not the rigor*. The split was correct, and it came after two full rounds had been spent auditing a diff that was never going to converge. **The audit is a bad place to learn the scope was wrong, because by then the rounds are already paid for.**

**A wide refactor is the exception.** One mechanical change whose blast radius fans across the codebase — retyping a shared symbol, renaming a field every module names — cannot be cut into slices that each land green, because the first slice breaks every call site. Sequence it **expand then contract**: add the new form beside the old so nothing breaks, migrate call sites in batches sized by blast radius (per crate, per module) with each batch its own issue blocked by the expand, then delete the old form in a final issue blocked by every batch. CI stays green batch to batch because the old form still exists. Do not force that shape onto ordinary work, and do not use it as a licence to skip the test above.

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
- **Open the PR as a draft, and open it early.** `gh pr create --draft` the moment there is a branch worth pushing, and carry the step 3 plan into the body. A draft is the cheap place to work: `ci.yml` skips every job while `draft == true`, so pushes cost nothing, and Copilot does not review one. Marking it ready is the expensive event and step 7 owns it. Iterating on a non-draft PR is how one branch here spent **nine CI runs** converging.
- **Never defer a finding into a new issue to get the PR closed.** If work surfaces something inside the scope of the task, fix it here. A new issue is for something genuinely out of scope, and it needs **all three** of a milestone, a `ROADMAP.md` row, and a shelf entry giving the reason it moved. Miss the milestone and the issue is **invisible**, not deprioritised: the query in step 1 filters by milestone and will never return it. Seven accumulated that way before anyone noticed, so file it in one command rather than intending to come back:

  ```sh
  gh issue create --title "..." --body-file f.md --milestone "Phase 5 — deferred findings"
  ```

  That milestone is named literally because it is the shelf that exists today. **The shelf is a class, not a name** — step 1's rule 2 identifies one by a description beginning `Shelf:` — so if a second is ever added, file against the right one rather than against this string.
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

Then polish, and let the **diff** pick the instrument rather than your appetite.

> [!IMPORTANT]
> **Both instruments spawn parallel review agents, and this step authorizes them.**
>
> `/simplify` runs four — reuse, simplification, efficiency, altitude. `/harden`
> runs three per round — adversarial, correctness, docs — and repeats until a
> whole round finds nothing new. That fan-out is the instrument, not an
> optimisation of it: the skill's own reason is that a solo reviewer
> tunnel-visions and disjoint remits do not.
>
> So a standing "do not spawn agents unless asked" **has been satisfied** — by
> the invocation that started this pass. Do not stop here to ask for it a second
> time. A session that halts at this step has paid for the whole diff, the plan
> fidelity check and the mutation run, and banked none of it.
>
> If the environment refuses the spawn outright rather than asking, that is a
> different thing and it is reportable: run the audit single-threaded, say in
> the report that it ran without fan-out, and name what that costs. Degrading
> loudly is fine. Stopping is not.

- **Under ~200 lines across ≤3 files** — `/simplify` alone. A full audit workflow on a small diff is theatre. (`/harden` states this floor itself and will decline; the number lives there, so if the two ever disagree, harden wins.)
- **Anything larger, or anything the rest of the system stands on** — `/harden` **until dry**. It runs `/simplify` as one of its own phases, so do not run one first and then the other. It also carries its own plan-fidelity phase: tell it the diff above was already done, so it records the result instead of repeating it.

Whichever runs, pass the docs carve-out into the invocation, because `/simplify` reduces and the comments explaining *why* something works are the ones nobody can reconstruct from the code:

> Documentation is non-negotiable during `/simplify`. Do not shorten, remove, or
> fold module-header docblocks, function doc comments, or `Why:` / invariant
> notes. If a simplification would delete context about why something works, skip
> the simplification.

**"Foundational" is not a self-assessment you get to lower.** If the change touches the frame path, the watch engine, the diff oracle, or the budget gates, it is foundational — that is the whole system. Do not accept your own "not worth fixing" on a first pass either: that dismissal has a one-pushback half-life here, and three out of four have historically been wrong.

## 7. Mark it ready, and wait for both reviewers

Everything until now happened inside a draft, where pushes are free. **Marking ready is the one expensive action in this skill, it is metered twice, and it should happen once.** `gh pr ready` fires `ready_for_review`, which wakes the full matrix on three platforms *and* Copilot's automatic review, and Copilot is quota-limited rather than merely slow.

So do not mark ready to "see what CI says". Mark it ready when the work is finished, the suite is green locally, and step 6's plan diff is clean. Everything else belongs in the draft.

> [!WARNING]
> **A draft shows no checks, and no checks is not green**
>
> The jobs are skipped, so the PR page shows an empty check list rather than a
> passing one. That is the exact shape `SPEC.md` §7 keeps finding — a gate that
> proves nothing while looking settled — and here it is on the review surface
> instead of in a test. Nothing in a draft has been verified by CI. The local
> suite is your only evidence until the checks below have actually run.

```sh
gh pr ready <n>                              # the metered event: CI + Copilot
gh pr checks <n> --watch --fail-fast         # blocks until the matrix settles
```

**Then wait for Copilot, which nothing watches for you.** It arrives as a review from `copilot-pull-request-reviewer[bot]`, usually in state `COMMENTED`, and the substance is in the **line comments** rather than the review body — reading the body alone is how you conclude it had nothing to say:

```sh
gh api repos/{owner}/{repo}/pulls/<n>/reviews \
  --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]") | .state'

gh api repos/{owner}/{repo}/pulls/<n>/comments \
  --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]")
        | "\(.path):\(.line)\n\(.body)\n"'
```

**Do not request a review before checking whether one is coming.** Automatic review fires on `ready_for_review`; an explicit request on top of it spends a second unit of quota on a review already in flight. Poll first, and only request explicitly if nothing has arrived after the checks have settled.

### Answering it

Copilot is **not authoritative and not dismissible**, and the two failure modes are symmetric. Applying a wrong suggestion because a reviewer said it is how a considered decision gets undone by a machine that never read `SPEC.md`; waving comments away as noise is the self-authored triage this skill refuses in step 6, with the same one-pushback half-life.

Every comment gets one of two outcomes, and both are visible:

- **Fixed**, in the diff.
- **Declined**, in a reply saying why — most usefully by naming the spec section or invariant the suggestion would violate, since that is the thing Copilot cannot see.

Silence on a comment is neither, and it reads to the next person as agreement.

### Iterating after ready is the expensive shape

Every push to a ready PR re-runs the matrix. **Batch the fixes into one push.** If the review turns up something that needs real iteration rather than a couple of edits, go back to the cheap surface instead of converging in public:

```sh
gh pr ready <n> --undo     # back to draft; CI goes quiet again
```

Fix there, then mark ready once more. Two metered events beat six.

### Merge

When the checks are green and every Copilot comment is fixed or answered:

```sh
gh pr merge <n> --squash --delete-branch
```

Squash, because the history here is one commit per PR with the number in the subject. **Green means the run that CI actually performed** — check that the matrix ran on the ready revision rather than reading a check list that is empty because the PR was still a draft when you last looked.

## 8. Close the loop, all four places

Skipping any of these is how the next session loses time.

1. **The issue.** Close it, with the evidence: commit, test count, numbers.
2. **`ROADMAP.md`.** Flip the status. Add to the deferral shelf or pull-forward log if anything moved.
3. **`SPEC.md`.** Only if the contract actually changed. Own commit.
4. **The vault**, through the MCP:
   - `record_work` for what happened here: changes, decisions, what was learned, what is still open, how it was verified.
   - `remember` for anything that would help someone on a **different** project. A `gix` limitation that would bite any Rust project is a `remember`. "Landed the watch engine" is a `record_work`. Both, when both are true.

## 9. Report

What was taken, what shipped, the numbers, what moved on the roadmap, and what the next task is. Then stop. Do not start it.

Name the **review outcome** too: whether Copilot commented, how many, and what happened to each. A review whose result nobody states is one nobody can tell you skipped — the same reason step 6's plan diff has to be said out loud.

And list **every decision taken without asking**, under its own heading, with the branch chosen and the one not taken. That is where the questions this pass did not stop for go, and it is the half that makes not stopping safe rather than merely faster: an unattended pass that finishes silently has decided things nobody can see. One line each is enough — it is a list to disagree with, not a justification.

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
- Halting on a question this file already answers, above all at the audit
- Offering "ship it as is" as an option
- Asking permission for the review agents, which the invocation already gave
- Leaving a pass unfinished with a pending prompt instead of finished with a flagged decision
- Opening the PR ready, or marking it ready to see what CI says
- Reading a draft's empty check list as a passing one
- Requesting a Copilot review that automatic review was already sending
- Converging on a ready PR one push at a time instead of returning it to draft
- Ignoring a Copilot comment, or applying one that contradicts `SPEC.md` because a reviewer said it
- Merging on checks that ran against an earlier revision
- Finishing without `record_work`
