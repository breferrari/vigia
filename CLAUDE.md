# vigia

A live diff **monitor** for the terminal. Not a review tool.

`vigia` is Portuguese: a watchman, and — nautically — a porthole. The window you look through. Also the verb: `vigia .` reads as "watch this."

## What this is, in one paragraph

You run an AI coding agent in one pane and `vigia` in the pane beside it. It shows the working-tree diff as it changes, continuously, without being touched. It is ambient. It is closer to `btop` than to a git client: you read state from shape and colour, glance away, and glance back.

## The product class is the whole thesis

`vigia` is **monitor-class**: already open, rarely touched, correct with zero input, cheap for days, glanceable. It is not a **reviewer** — something you launch per changeset to step through, annotate and decide on.

That distinction is not stylistic. It generates every budget in `SPEC.md`, and those budgets are the product.

**If a change would make `vigia` a better reviewer at the cost of an invariant, the change is wrong.** Reject it and say why.

## Stack — settled, do not relitigate

| Layer | Choice | Why |
|---|---|---|
| TUI | `ratatui` + `crossterm` | `bottom`, a btop-class system monitor, runs exactly this pair. `crossterm` is the only backend with Windows plus cross-platform mouse; `termion` is Unix-only |
| Git | `gix` | In-process diff. No `git diff` subprocess per tick |
| Watch | `notify` | Native FS events per platform, which I1 requires instead of a timer. Pure Rust: no `cc` on any tier-1 target. Coalescing is ours, not its debouncer crate |
| Highlighting | `syntect` | Pure Rust, so no C toolchain in CI. What `delta` and `bat` use |
| Release | `cargo-dist` | Cross-platform binaries + Homebrew formula + GH workflow |

Everything above is pure Rust on purpose: `--target x86_64-unknown-linux-musl` gives a static binary with no cross-toolchain, and macOS/Windows are tier-1. **Choosing tree-sitter over `syntect` reintroduces a C toolchain** — that is a spec change, not an implementation detail.

**Reading a dependency's source: address it, never search for it.** `ratatui` 0.30 splits into `ratatui-core`, `ratatui-crossterm` and `ratatui-widgets`, so the types this project draws against most often (`Buffer`, `Cell`, the layout solver) live in a **transitive** crate that is not in this workspace. Its source sits in the Cargo registry cache behind a hash-suffixed index directory and a version-suffixed crate directory, which is not a path anyone can guess, which is exactly why a session starts searching for it instead. Both of these are instant:

```bash
ls ~/.cargo/registry/src/*/ratatui-core-*/src/buffer/          # the sources, addressed directly
cargo metadata --format-version 1 | jq -r '.packages[] | select(.name=="ratatui-core") | .manifest_path'
```

**Never run `find /` on Windows.** Under Git Bash `/` is the MSYS root (`C:\Program Files\Git\`), so `~/.cargo` is not under it and no match is possible; and MSYS mounts the Windows registry as directories at `/proc/registry`, `/proc/registry32` and `/proc/registry64`, so the walk enumerates the hives twice through live Win32 calls and never terminates. Six of these, left orphaned by sessions that moved on, burned 26.8 CPU-hours over five hours on 2026-08-18. Bound any exploratory scan with `timeout`, and prefer Glob or Grep pointed at an explicit path.

`gix` was the least-precedented dependency here (`delta` uses `git2`/libgit2 instead, likely for age reasons), so Phase 1 proved it before anything was built on top. **Proven 2026-07-30:** hunk boundaries match `git diff -U3` exactly and every Phase 1 budget holds with room. Evidence and the one constraint it came with are in `SPEC.md` §10.

## Method: spec-driven, drift-enforced

`SPEC.md` is the source of truth. Code is written against it.

1. If the code and `SPEC.md` disagree, **stop.** One of them is wrong; decide which, change that one deliberately, in its own commit.
2. Every invariant in `SPEC.md` has a test that fails when it is violated. An invariant without a failing test is a wish.
3. Performance budgets are tests, not aspirations. The thesis is a measurable claim, so a regression past budget **fails the build**.

Do not add a dependency, a flag, or a subcommand that `SPEC.md` does not name. Propose it, get it into the spec, then build it.

## Where design decisions live

Design rationale for this project is recorded outside this repo, reachable through the **`vigil` MCP server**. This repo declares its identity as `vigia` in `.om-project`, which is what scopes reads and writes to this project.

### Reading

| Tool | Use it for |
|---|---|
| **`search`** | The full written record: why the product class is what it is, why each dependency was chosen, which alternatives were rejected and on what evidence, what each budget was set against. **Start here.** |
| **`expand`** | Once you have a specific note, see what it links to and what links back. Cheaper and more exact than searching again for the neighbourhood. |
| **`recall`** | Short durable lessons scoped to this project. Pass `explain: true` when something you expected is missing: it distinguishes "scoped away" from "never existed". |
| **`reason`** | A judgement that needs several notes weighed against each other, e.g. "is what I am about to do consistent with what was decided". It spawns a second session, so it is slower and costs more. Reach for it only when `search` returned the notes but not the answer. |
| **`health`** | When something that should be there cannot be found. Every failure in this layer looks identical from the outside (no results), and this is what tells them apart. |
| Resources | Notes are also exposed as `vault://note/<path>` and can be read directly when the title already answers the question. |

**`recall` is empty until sessions put things in it.** Early on it returns nothing, and that is *not* evidence the record is missing. Do not conclude "there is no record" from an empty `recall`. Use `search`.

Consult the record before changing:

- the CLI surface (flags, subcommands, exit codes)
- the rendering contract (what a "diff view" includes and excludes)
- the file-watching strategy and its platform assumptions
- the performance budgets and what they were set against

If that record and this repo disagree, the record holds the *why*. Reconcile before changing behaviour.

### Writing

Two tools, and picking the wrong one is the common mistake. **The test is whether it would help someone working on a different project.**

**`remember`** stores a durable **lesson**: a constraint you discovered, a gotcha that cost time, a rule that generalises. Not status, not a task summary, not anything you would resent being told again in six weeks.

- `confidence` is `verified` | `inferred` | `unverified`. Be honest, it is what a reader trusts. Supply `verification` whenever you claim `verified`: how you know, i.e. the test you ran or the source you read.
- `scope` decides who ever sees the memory again, and it is the field most often got wrong from this repo. Three values, and the middle one is not decoration:
  - **`project`** — true because of how *this* tool is built. `scope: "project"` with `projects: ["vigia"]`. The settle margin being two seconds is this.
  - **`platform`** — true for anyone on the same technology, whoever they are. **This repo sits on two, and they are not the same one.** A `gix`, `syntect` or `ratatui` limitation is `platforms: ["rust"]`. A `TERM`, colour-depth, ANSI or terminfo fact is `platforms: ["terminal"]` — it reaches a Go or Python TUI identically, and reaches nothing that has no terminal. Name both when both are true.
  - **`general`** — true with no platform at all: test shapes, review process, git and CI workflow, how an audit converges. It reaches **every** project the vault serves, so a fact about `TERM` filed here is served to a backend that has no terminal in it.

`general` is the easy over-claim, because it is the only one that needs no vocabulary. **If the lesson names a specific library, runtime, or environment variable, it is almost certainly `platform`.**
- `links` connects it to existing notes by title. `supersedes` corrects an earlier memory: the old one is kept and back-linked rather than deleted. **Correcting the scope of an earlier memory is a real use for it** — re-file with the right scope and supersede the original. The original keeps the reach it declared, but is served only where its correction is also served, so the narrowing actually takes effect instead of leaving the stale, wider copy reaching everybody alone.
- `dry_run: true` previews first.

**`record_work`** files what happened **here** into the vault. Use it at the end of a real piece of work. Write it for a session that will not have your context and cannot re-read your diff, so fill every field you can: `summary`, `changes` (one line per file, what and why), `decisions` (especially where you rejected an alternative), `learned` (surprises and near-misses), `open` (unresolved threads nobody should assume are handled), `verification` (tests run and their result, failures stated honestly). `kind: "decision"` files it as a decision record; `informed_by` credits the notes you actually read.

**If it is refused for tool-call markup, do NOT re-send the same shape** — it folds again. **Retry smaller in TOTAL SIZE, not merely in field count**: a call with fewer fields but a longer body has been refused where a wider, shorter one succeeded. Send the required fields with the prose trimmed, let that write land, then add the rest in a follow-up call. Two records that arrive beat one that never does. The same fold hits `remember`, so this is not a `record_work` quirk. **The mechanism is not settled** — do not repeat one. **The cost, so you choose it knowingly:** the fields you drop survive as prose in the body but stop being queryable as fields. Tracked as [breferrari/obsidian-mind#244](https://github.com/breferrari/obsidian-mind/issues/244).

Rule of thumb: **a `gix` limitation that would bite any Rust project is a `remember`** — and note that it is `scope: "platform"` with `platforms: ["rust"]`, not `general`, which is exactly the call this section exists to get right. **"Landed the watch engine and here is what it cost" is a `record_work`.** Do both when both are true.

Before finishing work that changed or clarified a decision here, write it down. A finding that stays in this session is a finding the next session pays for again.

## Bias to building

**The budgets have room and this section exists because that stopped being obvious.** The frame budget is 16ms and the tool runs at **2.4ms p50, 3.1ms p99**, which is five times the headroom. Refusing a feature on cost, in a tool with that much slack, needs the slack quoted alongside the cost or it is not an argument.

### A cost measured in isolation is not a budget

**Measure the whole, not the part, and measure it before the cost is allowed to restrict anything.** A component timed on its own answers a question nobody asked. What decides whether something is affordable is the thing it sits inside, measured with and without it, interleaved, with both numbers quoted.

This rule exists because it was broken twice in one session, on the same 256-path burst, in opposite directions:

- **Once on a wall clock.** 2.38ms p50 on a machine running four agents and a build. The feature was one edit from being capped at a quarter of its range on a number that was mostly contention.
- **Once on a CPU clock that could not see it.** `GetThreadTimes` reports in 15.625ms steps on Windows, so the burst read **0ns** and that was taken as evidence the cost was off-CPU and therefore the host's. Timed across enough rounds to clear the quantum it is 2.60ms of genuine CPU, so the cap went back.

Both readings were about the syscall alone. **Neither measured the frame the syscall sits in**, and that is the only number that decides anything. Measured properly, interleaved over thirty rounds of a hundred-file bulk rewrite, a frame costs **18.43ms sizing nothing, 17.39ms sizing sixty-four paths and 17.93ms sizing all 256**: the *unsized* run is the slowest of the three, because the status walk on the same wake has already stat'd every one of those files and the metadata is warm. The cost is unmeasurable in situ and the cap was deleted.

So, before a number restricts a feature:

- **A zero from a clock is a quantum until proven otherwise.** Time it across enough repetitions to clear the resolution, or you are reading the instrument rather than the code.
- **Two fixtures, not one.** Same workload, with and without the thing being priced, interleaved so a loaded machine moves both. A single number has nothing to be compared against and will be compared against a budget instead.
- **Quote the headroom and the whole**, not the component. "2.6ms against 16ms" is an argument about a syscall; "17.93ms against 18.43ms unsized" is an argument about the product.
- **A cap is a feature restriction and needs the same bar as a refusal.** It draws a worse graph for the reader, permanently, on the strength of a number. Duplicated work that costs nothing measurable is an efficiency row to file, never a reason to ship less.

**The tell is the shape of the work:** if the measurement's only possible use is to justify spending less, and the thing it protects has not been measured, the bias has already won.

Four rows of Phase 8 spent a full session each and delivered a **decline**. One has since been reopened because *both* reasons it rested on were false: the first was an invariant whose budget could never have measured the case, and the second was an absence (*"the takeover does not enable focus reporting"*) that was one unwritten line rather than a fact about the world. Two other refusals in the same phase cited the same invariant and it did not reach either.

So, when a reader asks for something:

- **The default is build.** A refusal overrides the person whose product this is, so it needs a reason that survives being checked, not one that merely sounds sound.
- **Before citing an invariant, quote the row's own words and show it reaches this case.** I1's budget is *0 wakeups while idle*; neither pointer motion nor a held button is idle. A budget cited without its current headroom is a mood, not a measurement.
- **Check facts about the world against the world.** Read the dependency in `~/.cargo/registry`, and search the web. *"No API reports that"* is the claim most likely to be a year out of date, and it has been wrong here twice.
- **A reason that collapses reopens the question.** It does not get replaced by a better reason for the same conclusion. That happened to [#123](https://github.com/breferrari/vigia/issues/123) and the decline outlived both of its bases.
- **Reach a decline early or not at all.** The reason either holds under checking or it does not, and that is cheap. Hours spent after that point are spent justifying, and the tell is prose getting longer while the argument does not get stronger.
- **Size the rigor to the surface, and do not escalate past it.** Look and feel is `/simplify` plus a screenshot; the audit loop is for the frame path and the invariants. `ROADMAP.md` has said so since Phase 8 opened and it was overridden anyway.
- **A limit you were not asked for is a refusal wearing a yes.** Everything above covers *no*. It did not cover [#272](https://github.com/breferrari/vigia/issues/272): nobody declined to build wrapping, a session built it and imported `delta --wrap-max-lines`' default of two as a cap, and the reader had to say twice that he never asked for it. What ships is what was asked for and nothing narrower. A bound taken from a neighbouring tool's default is that tool's decision, not a ruling here.
- **Never cite a session's own prior decision as a constraint on the reader.** Evidence, yes. Decisions he made, yes, and naming them is a service. A ruling a session wrote is a record of what was done, not permission withheld.

Refusals deserve more scrutiny than builds, not less, and an unrequested limit deserves the most of all: a refusal is at least visible, while a feature delivered narrower than it was asked for leaves nothing to see.

## Rulings say who made them

Every ruling and every constant in `SPEC.md` names its author:

```
Ruled 2026-08-26, reader.
Ruled 2026-08-26, session.
```

**Unattributed means a session inferred it, and an inferred ruling does not bind the reader.** Two words, and it turns *"why will you not do this"* from an argument into a lookup.

Mark the origin of the **ruling**, not of the symptom. "Reported from the pane" describes where a complaint came from and says nothing about who chose the constraint attached to it. #272's cap was reader-reported and session-decided, and writing only the first is what let it be quoted back at him as his own.

## Rulings can be revoked

When the reader overrules a ruling, **delete it in the same commit as the change.** Not annotated, not marked superseded, not kept with a note about what it used to say.

A rule that survives being overruled re-fires on the next session, and he argues it again from zero. #272's cap was overruled and still sat in the source, the spec and a gate a day later. If it is not deleted, the override did not happen.

## The spec is present tense

`SPEC.md` says what holds now. When a ruling is replaced, the old text goes: git has it, and `RULINGS.md` takes the trail when the trail is still needed to apply the rule.

**Annotating in place is not an option, and this is structural rather than a preference.** In any single case the argument for keeping the paragraph is the better one. Enough better arguments in a row is how a contract stops being readable, and how a commit that *removed* a feature still grew the file.

## Releasing

**Dispatch the `bump and release` workflow.** That is the whole procedure:

```sh
gh workflow run "bump and release" -f bump=<patch|minor|major> -f rehearse=false
```

It raises the version, commits it to `main`, builds the four target artifacts, creates the GitHub release, publishes the Homebrew formula, and runs `cargo publish --workspace`. Nothing else starts a release: `git tag && git push --tags` was removed as a trigger, and a tag pushed by a workflow cannot fire another one anyway.

Pick the level from the diff. On `0.x` a new feature **and** a breaking public API change both go in the **minor**; `patch` is for fixes that change no signature. `rehearse=true` runs the whole path and publishes nothing, which is the way to check a change to the release machinery itself.

**`RELEASE-SMOKE.md` is not this procedure and reading it as one wastes a session.** It is a human pre-flight against a built artifact: most of its boxes need a person at a terminal on three platforms, killing the process from another pane and looking at what the terminal does next. An agent cannot tick them. It is worth reading before a release that changes packaging, installation or the takeover; it is not a gate to clear before every dispatch, and it is not where the release is performed.

## House rules

- **No agent-session artifacts in anything that lands in the repo.** No `claude.ai/code/session_*` URL, no `Claude-Session:` trailer, no local absolute path — not in commit messages, PR or issue bodies, or files. `Co-Authored-By:` is fine and wanted.
- **No em-dashes** in anything published under Brenno's name: README prose, release notes, issue and PR bodies, commit messages. Use a period, a comma, a colon, or parentheses.
- Probe capability by behaviour, never by asking. A single green run is not evidence when the defect is non-deterministic.
- Verify the whole artifact, not just the property you were fixing.
- **A comment exists where the code cannot explain itself.** Why the obvious approach is wrong, an invariant a caller must hold, a cost invisible at the call site. Not a restatement of the code, not issue numbers, not ruling ids, not the history of the comment's own corrections. Those belong in the commit message and the tracker, which already hold them. The test: would this make sense to a reader in two years with no knowledge of the session that wrote it? A docblock longer than the item it documents means one of the two is wrong.
- **A title says what is broken or what to build.** One clause, no *because*. The body carries the reason. `Band saturates at half on a steady worktree`, not `A steady worktree saturates half the band, because the factor above the mean was never measured on this signal`.
- **Nothing prose stops at a column. One paragraph is one line.** Markdown files, issue and PR bodies, commit message bodies: no hard wrapping anywhere. GitHub turns a single newline in a body into a real line break, so a paragraph wrapped at 80 arrives broken mid-sentence — 811 forced breaks across sixteen of this repository's PRs before anyone measured it. Gated by `register.rs::no_prose_paragraph_is_hard_wrapped` over every tracked `.md`, because the rule was written down twice and lost twice: an instruction cannot beat a corpus, and these documents held 7,351 hard-wrapped lines that every session reads before it writes anything. **Code comments are the deliberate exception** — nothing renders them, so a break there corrupts nothing, and they sit beside code held near a hundred columns where one long line reads worse. Check a body with `gh api repos/OWNER/REPO/pulls/N -H "Accept: application/vnd.github.html+json" --jq '.body_html' | grep -c '<br'`.
