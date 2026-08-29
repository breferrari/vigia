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
> This skill is invoked and then left alone, frequently overnight. **A step that can only complete by asking a question does not complete** — it halts, and the answer arrives hours later with all the expensive work already done and nothing merged. That has happened: a pass finished 372 green tests, a clean plan diff, 17 killed mutants and before/after budgets, then stopped at the audit to ask permission for something this file already mandates.
>
> **Step 3 is the one exception, and it is not negotiable.** Plan approval is the single sanctioned stop in this skill — "run to the end" starts *after* it. An unattended pass that reaches step 3 **stops and waits**. It does not self-approve, and it does not proceed on the grounds that nobody is awake. **Not wanting to disturb someone is not approval**, and a plan nobody answered is not an approved plan. Halting at step 3 costs a night. Skipping it ships *what* nobody chose and silently disables the plan-fidelity gate downstream, which is the one gate that would have caught it.
>
> The line is **what** versus **how**. What gets built is settled at step 3 and the plan is where a question belongs, because it is free there. Everything after it is execution, and execution questions have answers in this file:
>
> - **The instruments are pre-authorized.** Invoking this skill *is* the request
> to run them, including the parallel review agents `/simplify` and `/harden` spawn. A standing "do not spawn agents unless asked" is satisfied by the invocation; it is not a second gate to clear at step 6. See that step.
> - **Where a choice is documented, take the documented one** and say so in the
> report. Do not offer it as a menu. This governs *how* to build a thing, and it is not a licence to inherit a **refusal** unexamined: a documented "no" is a conclusion someone reached from a reason, and the reason is checkable. See "The record is evidence, not authority" at step 3.
> - **Where a choice is genuinely open and nobody is there, take the more
> conservative branch, finish the pass, and put the question in the report.** A finished pass with a flagged decision beats an unfinished one with a pending question, because the flag survives and the prompt does not.
>
> Three things still stop the pass, and they are all *what*-shaped: a finding that contradicts `SPEC.md` (step 4 says stop and decide which is wrong), discovering the task is really two tasks, and anything destructive or irreversible outside this branch. Those are worth waiting for. Nothing else is.
>
> **"Ship it as is" is never one of the options.** It is the self-authored triage step 6 refuses, wearing a question mark.

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
2. **A milestone whose description begins with `Shelf:` is never selected.** A shelf is permanently open and never "next", and until #83 that was a fact only prose knew, so no query could act on it. Mark one by starting its description with `Shelf:` — the [Shelf milestone](https://github.com/breferrari/vigia/milestone/5) reads `Shelf: never next. …` — and leave the marker off anything meant to be taken in sequence. (It was titled "Phase 5" until 2026-08-06; older records citing that name mean this shelf, and a title with no phase number also sorts last by rule 1's own fallback, so the description prefix and the sort now agree.)
3. **An empty result is not an error, and it is not the paragraph above either.** Line 56's "returns nothing" is a milestone left open with zero open issues, which `select(.open_issues > 0)` now discards on its own. Empty *here* means every eligible milestone was discarded that way and what is left is shelved, exhausted, or nothing at all — so read which before acting, because the three want different things and only the first leads anywhere. Shelved work remaining is the point at which taking from the shelf becomes a deliberate choice rather than a default, and the deferral reason is re-read first, because a reason is a dated claim ([#76](https://github.com/breferrari/vigia/issues/76)). Drop the trailing `| .[0].title // empty` to see the ranked list the answer came from; one bare title is the answer without any of its evidence.

**All three rules are asserted by `sh .claude/skills/take-next/selftest.sh`**, which is offline, needs only `jq`, and takes about two and a half seconds, most of it the three cases that run `preflight.sh` end to end against a fixture. Run it after any edit to the query or to the rules beside it. It exists because the first two edits to this command were verified by hand into a GitHub comment, which is verification that dies with the shell it ran in, and because the rationale in rule 1 was **wrong** in its first version and nothing would have caught that. It also fails if the filter here and the filter it tests drift apart, in either direction.

**If you finish the last issue in a milestone, close the milestone.** It is the step that makes the next session's first command work.

`ROADMAP.md` is the plan; the issues are the truth. If they disagree, the issues win and the roadmap is stale, so fix the roadmap in the same pass.

### Pre-flight: does the spec still agree with the tracker?

That rule is right and nothing enforced it, so drift was only ever caught by someone happening to read both. It went wrong in **both** directions inside one hour: an issue described a design the spec had already moved past, and an issue sat open after the work it tracked had shipped.

**One command runs it: `sh .claude/skills/take-next/preflight.sh`.** It performs comparisons 1–4, 6 and 7 mechanically, prints §10's open bullets for the judgment in 5, and exits non-zero on any mechanical hit. Before any of them it checks **the board itself**, which is a precondition rather than an eighth comparison: the seven weigh two records against each other, and this one asks whether the tracker record is all here, because a fetch that stops short makes comparison 1 cry wolf while 2, 4 and 7 quietly under-report. Every session used to do this by hand from the blocks below, which is the intention-triggered shape that decays; the blocks stay because the **why** is what a session needs when a finding fires, and the script carries the same traps in code (explicit ref, `tr -d '\r'`, character-class boundaries, no `--paginate`). Mutation-tested in both directions before it was trusted — including one mutation that survived because the *mutation* was wrong (it flipped a row with no issue link, which comparison 3 rightly ignores), which is itself the lesson §7 teaches about instruments.

The seven, by hand if the script is unavailable — three commands gather the state; you do the comparing:

```sh
git fetch -q origin

# Invariants the spec declares.
git show origin/main:SPEC.md \
  | grep -oE '^\| \*\*I[0-9]+[a-z]?\*\*' | grep -oE 'I[0-9]+[a-z]?' | sort -u

# What the tracker holds. The limit is above the whole board on purpose: a
# fetch that stops short is invisible from inside a comparison, and at 200
# against 202 issues it dropped #2 and made comparison 1 report I2a untracked.
gh issue list --state all --limit 1000 --json number,title,state,milestone

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
> The obvious version greps bare `before|first|until`, and on this spec it is **80% false positives**: §10 is full of "first paint", "the first frame that draws deep", "the lines before it". A check that nags four times for every real hit gets skipped exactly like one that never fires, which is the failure this whole comparison exists to fix. Tested both directions before landing: against `origin/main` it returns three bullets and all three are genuinely actionable, and against `dbc97aa` — the last commit before [#32](https://github.com/breferrari/vigia/issues/32) closed — it returns the settle-margin bullet, the exact prerequisite that hid for two phases. Widen it only with the same two runs.

Then seven comparisons. Any hit is a finding to fix **in this pass**, not a note:

1. **Untracked** — an invariant the spec declares that no issue title names.
2. **Orphan** — an issue naming an `I<n>` token the spec no longer declares. This is what catches a rename or a split that left the tracker behind.
3. **State** — a roadmap row marked `✅` whose issue is open, or a row not marked done whose issue is closed. And a row citing an issue the tracker does not have at all, which for a long time this skipped in silence: a deleted issue, a transferred one and a mistyped `#N` all arrive here looking identical, and none of them is fixed by fetching more.
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
> The first run of this check read the checkout, which had a feature branch active, and compared branch state against the live tracker. It reported the roadmap as ahead of an issue when on `main` the two agreed — and **#2 was closed on the strength of a line that was not merged.** A check whose answer depends on which branch happens to be checked out is worse than no check.
>
> Comparing uncommitted state is occasionally what you want. It has to be asked for out loud, never the default.

7. **Missing row** — an issue, any state, that `ROADMAP.md` never mentions at all. The 2026-08-03 sweep found four gaps in exactly this direction while the five comparisons then existing ran clean over the same data: a drift check has a direction, chosen by whichever collection it iterates, and this is the one that iterates the tracker against the roadmap rather than the reverse.

Only the first two directions are cheap to eyeball; run all seven anyway — which is what `preflight.sh` is for. A check that nags forever gets ignored exactly like one that stays silent, so if a finding is a false positive, fix the *check* here rather than learning to skip it.

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

### A decline is the most expensive thing this skill can produce, so reach it early or not at all

**Four passes in Phase 8 each spent a full session and delivered a "no"** ([#149](https://github.com/breferrari/vigia/issues/149), [#120](https://github.com/breferrari/vigia/issues/120) twice, [#123](https://github.com/breferrari/vigia/issues/123), [#124](https://github.com/breferrari/vigia/issues/124)), and one of them has since been reopened because both of its reasons were false. That is the worst output shape available here: maximum cost, nothing on screen, and a reader who asked for something getting an essay about why they cannot have it.

Three rules, and they are about *when* and *how hard*, not about being agreeable:

- **A decline is reached in the first part of the pass or it is not a decline.** The reason a thing should not be built either holds under checking or it does not, and that is a question about facts, which is cheap. Hours spent after the reason is known are not spent deciding, they are spent **justifying**, and the tell is that the writing gets longer while the argument does not get stronger. If you have been at it a while and are still assembling the case, the honest reading is that the case is weak.
- **A decline carries a higher bar than a build**, for the asymmetry step 3 records: a bad build is loud and a bad refusal is silent. The reason has to be a fact that survives being checked, not a consideration that sounds sound. **"It would cost a wake", "it needs a timer", "no API reports that"** are all claims with answers, and all three have been wrong here.
- **When the reader has asked for the thing, the default is build.** A refusal overrides a direct request from the person whose product it is, so it needs a reason that would still convince them after they have read it. If the honest summary is *"it is possible, it is affordable, and I would have designed it differently"*, that is a preference and the answer is build it.

**And the ruling's prose is sized to the ruling, not to the effort.** A decline that took an afternoon does not earn four paragraphs in `SPEC.md` because it took an afternoon. Long justification of a "no" is the same failure one layer over: it makes the refusal harder to revisit, because the next reader has to argue with a wall of text instead of with one checkable sentence. State the reason in the fewest words that can be falsified, and put the evidence trail in `RULINGS.md` where it belongs.

### A `decision` issue is ruled first and built second, in the same pass

Check the label before planning. An issue labelled **`decision`** is one whose acceptance is **a ruling recorded in `SPEC.md`** rather than a diff — and, when the ruling is *yes*, the build that makes the ruling true, which is the correction 2026-08-17 forced into this section after a ruling shipped in a release that changed nothing a reader could see. Three exist today ([#74](https://github.com/breferrari/vigia/issues/74), [#50](https://github.com/breferrari/vigia/issues/50), [#89](https://github.com/breferrari/vigia/issues/89)) and they were already written that way — #74's exit criteria read *"a ruling: build the seam, or record that direct consumption is accepted"*, and *"if declined: a line in `SPEC.md` §6 saying so, so the next reviewer finds a decision instead of an omission."*

**They had no route, which is the actual defect.** Every step below assumes a diff: plan a build, ship it, scope the checks to the changed files, prove it with gates. Handed a `decision` issue, this skill plans a build for something whose answer might be *"do not build it"* — and #50 sits in a phase that gets **taken in sequence**, so this is reachable rather than hypothetical.

When the issue is labelled `decision`:

- **The deliverable is the ruling and its reasoning**, written where the next reader will hit it — the `SPEC.md` section the issue names, plus its §10 bullet closed. A ruling filed only in the issue is not filed: the issue closes and `SPEC.md` still reads as an omission.
- **The ruling goes in the contract. The road not taken goes in `RULINGS.md`.** *"Declined"* is a result and the case for it is worth keeping, but keeping both branches in `SPEC.md` means the contract grows on a refusal as surely as on a build, so no outcome ever fails to add to it. `SPEC.md` gets the ruling in the fewest words that can be falsified; the rejected branch and its evidence go to the ledger, under its budget, or nowhere.
- **A ruling of *yes* is the first half of the pass, not the end of it.** The build gets its **own issue and its own PR**, so the ruling is never blocked by an implementation, and then **this same pass takes that issue and ships it**. Stopping at the ruling is what this bullet used to say and it was wrong: #167 ruled that `?` opens a gestures sheet, filed #206 for the build, and released **0.11.1 with nothing on screen**. The reader pressed `?`, got nothing, and had to go and find out that the tool had been *told* about a feature it did not have. Two PRs, one pass, in that order.
- **The only reason to stop after the ruling is size, and it is a claim about the build rather than about the rules.** If the build genuinely will not fit one fresh context (the test three sections down), stop there and say so — but say it in the **first line of the report**, in the form *"nothing the reader can see has changed yet; the build is #N"*. A pass that ends with the tool doing exactly what it did before is allowed. A pass that ends that way quietly is not.
- **The report opens with what a reader can now do that they could not before.** If the honest answer is *nothing*, that is the first sentence, not a detail three paragraphs down. This is the line that would have caught the case above, and no gate can see it: the suite was green, the plan was delivered in full, and the feature did not exist.
- **The gate is different.** There is no diff to scope checks to and nothing for `/harden` to audit, so step 5's docs-vs-code split resolves to docs and the fidelity check in step 6 runs against the *plan's promises about the ruling*: every question the issue asked is answered, and each answer names what it rests on.
- **A ruling that cannot be made is a finding, not a failure.** If it needs a measurement nobody has run or a week of use nobody has had, say so, record what would settle it, and leave the issue open with that written down. #50 needs exactly that, and pretending otherwise produces a guess wearing a ruling's clothes.

## 2. Load the why before touching code

**Except on a research or look-and-feel row, where the world comes first.** Reading the rulings before surveying anchors the survey to what was already decided, and a session that starts from the spec proposes what the spec already permits, which is how a document recording yesterday's ceiling becomes tomorrow's. It happened on #318: the first read of the field was filtered through three rulings before any survey ran, and the reader had to say *"the specs hold us down too much"* before the aperture opened. On those rows: survey the world, form the outside view, then diff it against the record and reconcile. `SPEC.md` §0 states the same rule from the document's side.

**Read the repo first.** The issue carries acceptance criteria, `SPEC.md` carries the contract, and — unusually for a repo — a great deal of the *reasoning* is here too: §10's open questions carry their measurements, the invariant callouts explain why they split, and the commit messages argue rather than announce. Most "why did we do it this way" questions are answered inside the checkout.

Then reach outside it, through the `vigil` MCP server, for the three things the repo deliberately does **not** hold:

- **What cannot be public.** This repo is public. The competitive read, the market position, the objection this project was started against — none of that belongs in a file anyone can clone, and it is the reason the vault exists at all.
- **What generalises past this repo.** A `gix` limitation or a measurement trap is true for every Rust project, so it lives where other projects can reach it.
- **What predates or outlives the code.** Why this is monitor-class rather than review-class was decided before the first commit, and re-deriving it is the most expensive mistake available here.

```
search   the decision you are about to touch
recall   accumulated constraints — empty early, and empty is not evidence of none
```

**Consulting the vault is deliberate, not reflexive.** Reaching for it when the answer is in `SPEC.md` wastes a call; reaching for it out of habit while writing a *public* commit message loads strategic context that must never land in one. This repo's own PreToolUse hook (`.claude/scripts/leak-guard.mjs`) blocks a `gh` publish whose body carries a session artifact, but **commit messages have no automatic guard here** — that discipline is yours, and a squash commit merged to a public `main` stays reachable by SHA forever. Know which of the three you are asking for before you ask.

## 3. Plan it, in plan mode, before touching code

**Enter plan mode and write the plan. No code before an approved plan.**

**Approved means a person answered.** Not "the plan is written", not "the plan is obviously right", not "it was late and asking would have woken someone". If this pass is unattended, step 3 is where it ends for the night: present the plan and wait. That is the designed outcome, not a failed pass — step 3 settles *what* gets built, and that was never the session's call to make.

The plan is not ceremony and it is not for you — it is the only artifact the finished work can be *audited against*, and code cannot audit itself.

Without it the completeness check downstream has nothing to compare to. `/harden` carries a plan-fidelity phase that explicitly skips when no written plan exists, so a session that skips planning silently disables the one gate designed to catch under-delivery. That gate exists because a session once passed five clean audit rounds with 501 tests green and had still quietly shipped three promises short.

### The plan states what it stands on

Step 2 sends you to the record before touching code. **The plan is where you show what came back**, and it is the last point at which a contradiction is still free to fix.

Name, in the plan itself:

- the decisions this plan rests on, by title, from `search` / `recall` / `SPEC.md`
- anything you found that argues **against** the approach, and why you are proceeding regardless
- an explicit "nothing recorded on this" when the record is empty — a real finding, not a blank to skip past

Consultation at step 2 alone fires exactly once, at the moment you know least about what you will need. Naming the result here moves it to the moment you commit. A choice re-derived from scratch that contradicts a recorded one is the most expensive mistake available in this repo, and the plan is the only place it is visible while it still costs nothing.

### The record is evidence, not authority

`SPEC.md` is the source of truth for **what must hold now**. That is not the same as every sentence in it being beyond question, and the difference is where this skill has gone wrong more than once.

**Try to break the assumption before you build on it. Look at it from the angles the original ruling did not.** A written reason is the best evidence available about what someone knew *at the time they wrote it*, which is exactly as durable as the facts it rested on. Treating it as scripture is how a repo inherits a constraint nobody has believed in for months.

Three distinctions worth keeping straight, because they are not equally solid:

- **An invariant with a measurement behind it** (I1's idle cost, I4's streaming bound) is load bearing and is not relitigated casually. But **whether it reaches the case in front of you is a fresh question every single time**, and that is the check that actually gets skipped. Quote the row's own words and see whether they describe your case. Twice in one week here, an invariant was cited to refuse something its budget could never have measured.
- **A refusal** is a conclusion from a moment. It carries a date and a reason, and both expire. Challenge it by attacking the reason, not by re-arguing taste.
- **A budget or a threshold** is a number someone chose against a workload. If your workload is not that workload, the number is not evidence about you.

**A budget invoked as a reason to refuse something must be quoted with its current headroom.** Not the limit: the limit *and* where the tool actually sits against it. "This costs a wake" is not an argument when the last measurement says the frame path uses **2.4ms of a 16ms budget**, which is five times over. A budget at 19% is a budget with room, and saying so out loud is what stops the number from being used as a mood.

That is not a licence to spend the headroom carelessly, and it is not an argument against the budgets, which exist because the thesis is a measurable claim. It is a rule about **honesty in citation**: the same paragraph that names the ceiling names the floor the tool is standing on, and then the trade is visible instead of implied. If the honest sentence is *"this would take us from 2.4ms to 2.6ms against a 16ms budget"*, that sentence usually settles the question in favour of building it, which is exactly why it has to be written rather than skipped.

**And do not run a measurement whose only possible use is to justify a no.** Measuring to find out is the most valuable thing in this repo's history: counting instead of building took a fixture from 442.71ms to 8.76ms, and it was found by someone checking a premise rather than defending one. Measuring to build a case against a feature a reader has asked for is the same activity pointed backwards, it costs real time and attention, and it produces a number that was never going to change the answer. If you already know what you want the measurement to show, you are not measuring.

**Refusals deserve more scrutiny than builds, not less, and the reason is asymmetry.** A bad build is loud: tests fail, gates redden, someone reports it within the hour. A bad refusal is silent. Nothing breaks, no gate fires, the feature simply does not exist and no one can see the hole where it should be. So the failure mode that survives longest in a well-gated repo is precisely the one the gates cannot reach, and every instrument in this skill points at code that was written rather than at code that was refused.

**A refusal cited in a plan is quoted, dated, and marked checked or not.** The plan names the refusal's reason in its own words, the date it was ruled, and whether this session re-checked the reason against the world. A refusal relayed without those three is not evidence, and the fidelity gate should read it as an unnamed premise.

**The burden sits on the reason, not on the person asking for the thing.** "It is written down" is not an argument, and neither is "we ruled on this." Both are pointers to an argument, and the argument is what gets checked. When a reader asks for something the record refuses, the first move is to go and re-read *why* it was refused, not to relay the refusal.

**And a refusal that turns out to rest on a false premise is not a decision to defend, it is a bug in the record.** Fix it the way any other defect gets fixed: say what was wrong, reopen the question, and leave both versions visible so the next reader can see the correction rather than only its result.

### The plan names its premises, and settles the load-bearing ones itself

The section above records what the plan **stands on**: decisions already written down. This one is about what it **assumes**, which is the part nothing has ever checked.

A premise is a claim that must be true for this to be the right plan at all. It is not a decision, because nobody made it — it is what everyone took for granted. Write them as a short ledger before the plan body:

- **What must be true** for this approach to be correct.
- **How it would be falsified** — the observation that would end the plan.
- **The answer and where it came from**: measured, read in the dependency's source, **checked against the world outside this repo**, recorded in `SPEC.md`, or *assumed*.

**A premise the plan is load-bearing on is not allowed to stay `assumed`. Go and find out.** Finding facts is this session's job and never Brenno's: read the source, write the throwaway probe, take the measurement, **search the web**. A question a probe can answer is not a question for the report, and it is certainly not one to leave for an empty room at 3am.

#### A premise about the outside world is checked against the outside world, not against memory

Some premises are not about this code at all. *Does this library expose that?* *Does the protocol carry it?* *Do terminals implement it?* *What do the toolkits that solved this already use?* Those have answers, the answers change, and the one place they are never reliably stored is a model's recollection of them.

**So read the dependency's source in `~/.cargo/registry`, and search the web.** Both, when both apply: the source says what the API is, the web says whether the world it talks to actually honours it. A number taken from two independent implementations is a number nobody has to defend; a number chosen because it felt right is one that gets re-argued every time it is looked at.

Three from one afternoon, all of which changed what shipped:

- *"No mouse protocol reports a held button"* was **confirmed** by reading `crossterm`'s `MouseEventKind`, which is what made a hold-to-repeat clock a design problem rather than a lookup.
- *"The repeat cadence should be about 400ms then 40ms"* was a guess. Qt's `qscrollbar.cpp` uses `initialDelay = 500` and a `50` repeat, and GTK's `gtk-timeout-repeat` defaults to 50ms. **500/50 shipped**, and it is defensible in a way the guess never was.
- *"The takeover does not enable focus reporting"* had been written into `SPEC.md` as the mechanism that made a hover highlight impossible. `crossterm` has shipped `EnableFocusChange` and `Event::FocusLost` for years, `?1004h` is implemented by xterm, iTerm2 and kitty, and on Windows the console API delivers focus events unasked. **A feature had been declined for a year-old absence that was never there.**

**The cost of not checking is asymmetric.** A search that confirms what you thought costs a minute. A premise about the outside world that is quietly out of date costs a feature, and it costs it silently, because a wrong fact about someone else's library produces a plan that is internally consistent and wrong.

#### A recorded decision's *reason* is a premise, and it is the one most likely to be stale

The section above says the plan names the decisions it stands on. This says what to do when one of them is a **refusal** you are about to inherit.

**Re-read the reason it was declined for, and check whether it is still true.** Not whether the decision was reasonable, and not whether you would make it again: whether the specific sentence it rests on is a fact today. A decline is a dated claim exactly the way a deferral is ([#76](https://github.com/breferrari/vigia/issues/76)), and it is worse than a deferral in one way, because a deferral advertises that it expires and a ruling reads as settled.

Two shapes to look for, and this repo has produced both inside one week:

- **The reason names an absence.** *"No API for this", "the takeover does not enable X", "nothing would tell it to turn off."* An absence is the claim most likely to have stopped being true, because dependencies add things and nobody re-reads a refusal when they do. §11.2 **B10** declined a hover highlight on *"the takeover does not enable focus reporting"*, which was a description of one line this repo had not written, phrased as though it were physics.
- **The reason cites an invariant.** Check that the invariant's **letter** reaches the case, not just its spirit. §11.2 B10 and [#166](https://github.com/breferrari/vigia/issues/166) were both refused on I1, and in both the citation was wrong: I1's budget is *0 wakeups while idle*, and neither pointer motion nor a held button is idle, so the gate could never have caught either. **Two refusals, one invariant, zero applicability.**

**And when a reason collapses, the decision reopens.** It does not acquire a new reason. B10's first reason was measured and found false during its own ruling, a second was substituted, and the conclusion never moved: the decline outlived both of its bases. A conclusion that survives while its stated basis is swapped underneath it is motivated reasoning wearing a ruling's clothes, and it is invisible from the inside, because each individual step looks like diligence. If the sentence a refusal rests on turns out not to be true, say so and put the question back on the table, even when that means reversing something written down hours earlier.

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

- **The tool watches its own build when anyone is at the keyboard.** If Brenno is present, `vigia` runs in a side pane on this worktree for the duration of the pass — the workload [#72](https://github.com/breferrari/vigia/issues/72) says has never been measured is exactly this session, and six of this repo's defects were found by a reader looking at the screen within the hour while eleven green gates sat over one of them. Costs nothing on an unattended overnight pass; the report line in step 9 says which kind this was.
- **The unit is the issue.** One issue, one branch, one worktree, one PR. Splitting one issue across several PRs fragments review and reads as progress theatre. If the issue is genuinely two things, say so and split the *issue* first.
- **Work in a worktree, never in the main checkout.** Two long-lived worktrees exist for passes — `../vigia.a` and `../vigia.b`, reused rather than created per issue so their `target/` stays warm — and the main checkout stays parked on `main`, which is what keeps "read from `origin/main`" and "what the working tree shows" from ever being different questions. The rule was earned in one evening, twice: an uncommitted edit sat in a tree another session was committing from, and a branch checkout in the shared tree meant a second session's commit would have landed on the first one's branch. A worktree costs one cold build the first time and nothing after; the shared-checkout failure costs someone else's work.
- **An instrument run that outlives the pass gets a start-comment on its issue** — what is running, where its output lands, and when it ends — at the moment it starts, not only a report when it finishes. A 24-hour soak recorded only in a commit message was ruled "unauthorised" by a parallel session planning from the machine it was running on; the comment on the window's issue is what reached that session, and pre-flight comparison 0 is the mechanical half that does not rely on being read.
- **Open the PR as a draft, and open it early.** `gh pr create --draft` the moment there is a branch worth pushing, and carry the step 3 plan into the body. A draft is the cheap place to work: `ci.yml` skips every job while `draft == true`, so pushes cost nothing, and Copilot does not review one. Marking it ready is the expensive event and step 7 owns it. Iterating on a non-draft PR is how one branch here spent **nine CI runs** converging.
- **Never defer a finding into a new issue to get the PR closed.** If work surfaces something inside the scope of the task, fix it here. A new issue is for something genuinely out of scope, and it needs **all three** of a milestone, a `ROADMAP.md` row, and a shelf entry giving the reason it moved. Miss the milestone and the issue is **invisible**, not deprioritised: the query in step 1 filters by milestone and will never return it. Seven accumulated that way before anyone noticed, so file it in one command rather than intending to come back:

  ```sh
  gh issue create --title "..." --body-file f.md --milestone "Shelf"
  ```

That milestone is named literally because it is the shelf that exists today. **The shelf is a class, not a name** — step 1's rule 2 identifies one by a description beginning `Shelf:` — so if a second is ever added, file against the right one rather than against this string.
- **A finding about the process files to the Shelf, and instrument work is takeable only when a product pass is blocked by it.** An audit that surfaces a defect in a gate, a check, a skill, or a workflow has found something real and not something *next*: it goes to the Shelf with its dated reason like any deferral, never into a phase. The instruments generate findings faster than the product generates users, and a queue that serves both equally serves the mirror — adopted 2026-08-07, the morning after a night of the machine's best work went to a memory meter's baseline while seven look-and-feel rows sat unbuilt. A product pass genuinely blocked by an instrument defect takes the unblock inside its own pass, sized to the blockage, and says so in the report.
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
> `/simplify` runs four — reuse, simplification, efficiency, altitude. `/harden` runs three per round — adversarial, correctness, docs — and repeats until a whole round finds nothing new. That fan-out is the instrument, not an optimisation of it: the skill's own reason is that a solo reviewer tunnel-visions and disjoint remits do not.
>
> So a standing "do not spawn agents unless asked" **has been satisfied** — by the invocation that started this pass. Do not stop here to ask for it a second time. A session that halts at this step has paid for the whole diff, the plan fidelity check and the mutation run, and banked none of it.
>
> If the environment refuses the spawn outright rather than asking, that is a different thing and it is reportable: run the audit single-threaded, say in the report that it ran without fan-out, and name what that costs. Degrading loudly is fine. Stopping is not.

- **Under ~200 lines across ≤3 files** — `/simplify` alone. A full audit workflow on a small diff is theatre. (`/harden` states this floor itself and will decline; the number lives there, so if the two ever disagree, harden wins.)
- **Anything larger, or anything the rest of the system stands on** — `/harden` **until dry**. It runs `/simplify` as one of its own phases, so do not run one first and then the other. It also carries its own plan-fidelity phase: tell it the diff above was already done, so it records the result instead of repeating it.
- **The surface picks the bar, the way the diff picks the instrument.** Engine and invariant work hardens until dry — that rigor earned its keep in the frame path and it stays. Look-and-feel work (the Phase 8 class: layout, colour, keys, chrome) defaults to `/simplify` plus snapshot review plus **a screenshot in the PR**, because the judge of feel is a human eye and it rules in five seconds; a three-agent audit loop on an inset spends a night where a look decides. The escalation is one-way: feel work that touches the frame path, the watch, or any invariant's surface takes the engine bar for that part.

Whichever runs, pass the docs rule into the invocation. `/simplify` is the only instrument that reduces, and freezing docs against it is what took the comments to 60% of the shell crate: the ratio could only ever climb. What actually needs protecting is a class, not a volume, so protect the class:

> Documentation is in scope for `/simplify` and is judged by the same rule as code: a comment exists where the code cannot explain itself. Keep why the obvious approach is wrong, an invariant a caller must hold, and a cost invisible at the call site. Where a comment exists because the code is unclear, the fix is clearer code and the comment goes with it. Delete restatements of the code, issue numbers, ruling ids, and any account of the change rather than the thing.

**And pass the read-only carve-out, because an agent that builds competes with you for the machine.** On 2026-08-18 four review agents were told to measure and each ran an optimised build (`lto = "thin"`, `codegen-units = 1`) concurrently with the session's own, one against a probe worktree carrying a 3.4G `CARGO_TARGET_DIR`. The machine was audibly saturated before anything in the loop noticed, because a session's picture of the machine is its own actions only. It costs nothing in review quality to prevent: round 2 of #245's audit ran read-only with the numbers pasted into the brief and was just as sharp as the round that built.

> Read the code. Do not run builds, benchmarks, test suites, or anything else that consumes the machine: `cargo build`, `cargo test`, `cargo bench`, `cargo clippy` and the soak are all mine to run, not yours. Every measurement you need is in this brief. If one you need is missing, name it and say what it would change about your finding, and I will run it and hand it back.

Running is the orchestrator's job in both directions: the numbers an agent reasons from come from **one** run, so the audit reads a single consistent picture rather than four builds' worth of contention. This is the same rule as the concurrency clause the soak workflow already carries, applied to review instead of to instruments.

**"Foundational" is not a self-assessment you get to lower.** If the change touches the frame path, the watch engine, the diff oracle, or the budget gates, it is foundational — that is the whole system. Do not accept your own "not worth fixing" on a first pass either: that dismissal has a one-pushback half-life here, and three out of four have historically been wrong.

## 7. Mark it ready, and wait for both reviewers

Everything until now happened inside a draft, where pushes are free. **Marking ready is the one expensive action in this skill, it is metered twice, and it should happen once.** `gh pr ready` fires `ready_for_review`, which wakes the full matrix on three platforms *and* Copilot's automatic review, and Copilot is quota-limited rather than merely slow.

So do not mark ready to "see what CI says". Mark it ready when the work is finished, the suite is green locally, and step 6's plan diff is clean. Everything else belongs in the draft.

> [!WARNING]
> **A draft shows no checks, and no checks is not green**
>
> The jobs are skipped, so the PR page shows an empty check list rather than a passing one. That is the exact shape `SPEC.md` §7 keeps finding — a gate that proves nothing while looking settled — and here it is on the review surface instead of in a test. Nothing in a draft has been verified by CI. The local suite is your only evidence until the checks below have actually run.

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

**This is the loop's least reliable step, so it carries a fallback and a check.** The write guard has refused a legitimate record nine calls running (the #15 record was filed by hand), and one record returned success while writing its sections into the note as raw tool markup, which sat corrupted for two days. So: if `record_work` refuses or errors after a couple of honest rephrasings, **file the note by hand** in the vault (`projects/vigia/notes/`, matching the dated-note shape) and say so in the report — the record is the requirement, the tool is only the route. And after any success, **read the note back** (`search` for it) and confirm the sections landed as markdown; a success return is not evidence of a clean write.

**Then file the recurrence — this fallback has an exit condition.** Try the narrow shape first: `title` and `summary` alone, then the remaining sections in a second call. If it still refuses, comment the date and the received field list on [breferrari/obsidian-mind#244](https://github.com/breferrari/obsidian-mind/issues/244) rather than only noting it in the report. **A documented workaround suppresses the bug report**: once the failure has a prescribed response it reads as a handled step rather than a defect, which is exactly how this one ran ten days across nine consecutive passes while every session followed the procedure correctly. No session can see the rate from inside its own pass — nine here each saw two or three refusals and concluded it was local. A fallback with no exit condition is a permanent bug wearing a procedure.

## 9. Report

**The first line is what a reader can now do that they could not before.** One sentence, in the tool's own terms — *"`?` opens a sheet of every gesture"* — and if the honest answer is *nothing yet*, that is the first line instead, naming the issue that will change it. Everything below is evidence for that sentence, and a report that buries it has hidden the only fact the reader was waiting for. It is here because no gate can check it: a pass once ended green, complete against its plan, and shipped a release in which nothing on screen had changed.

Then what was taken, what shipped, the numbers, what moved on the roadmap, and what the next task is. Then stop. Do not start it.

Name the **review outcome** too: whether Copilot commented, how many, and what happened to each. A review whose result nobody states is one nobody can tell you skipped — the same reason step 6's plan diff has to be said out loud.

And a **`vigia observations`** line: anything the pane showed that read wrong while this pass ran, or `none`, or `pane not open — unattended pass`. This is [#72](https://github.com/breferrari/vigia/issues/72)'s instrument, one line at a time; an observation that changes nothing goes to the issue anyway, because the workload evidence is the accumulation and not any single line.

And list **every decision taken without asking**, under its own heading, with the branch chosen and the one not taken. That is where the questions this pass did not stop for go, and it is the half that makes not stopping safe rather than merely faster: an unattended pass that finishes silently has decided things nobody can see. One line each is enough — it is a list to disagree with, not a justification.

Include the **plan-fidelity result** explicitly — "every promise delivered", or the deviations and what was done about them. Silence here reads as clean, and a step whose absence is indistinguishable from success is a step that stops happening.

Say what the record gave you, too: which recorded decisions the work stood on, or that there were none. The same reasoning applies — a consultation nobody reports is a consultation nobody can tell you skipped.

## Anti-patterns

- Surveying the whole roadmap instead of taking one task
- Taking a later task because it looks more interesting
- Writing code before an approved plan exists
- Self-approving a plan because the pass is unattended, or to avoid disturbing someone
- A plan too vague to diff — it passes the fidelity check by promising nothing
- A plan that lives only in the conversation, so it dies at the next compaction
- Justifying a deviation at audit time instead of when it was taken
- Reporting a deviation instead of correcting it
- Taking a task while an open `SPEC.md` §10 bullet says something else comes first
- Closing an issue whose invariant has no failing test
- Stopping at a ruling of *yes* and leaving the build for a later pass without saying, in the report's first line, that nothing a reader can see has changed
- Releasing a version whose only content is a ruling, without saying that the binary does what the last one did
- Filing a follow-up issue to avoid fixing something in scope
- Running the full suite on a markdown diff, or skipping it on a manifest diff
- Reporting green without naming what ran
- Halting on a question this file already answers, above all at the audit
- Offering "ship it as is" as an option
- Asking permission for the review agents, which the invocation already gave
- Letting a review agent build, measure or soak, instead of handing it the numbers you already ran
- Reissuing a search that has not returned, rather than checking whether it is still running
- Leaving a pass unfinished with a pending prompt instead of finished with a flagged decision
- Opening the PR ready, or marking it ready to see what CI says
- Reading a draft's empty check list as a passing one
- Requesting a Copilot review that automatic review was already sending
- Converging on a ready PR one push at a time instead of returning it to draft
- Ignoring a Copilot comment, or applying one that contradicts `SPEC.md` because a reviewer said it
- Merging on checks that ran against an earlier revision
- Finishing without `record_work`
