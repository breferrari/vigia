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

Rule of thumb: **a `gix` limitation that would bite any Rust project is a `remember`** — and note that it is `scope: "platform"` with `platforms: ["rust"]`, not `general`, which is exactly the call this section exists to get right. **"Landed the watch engine and here is what it cost" is a `record_work`.** Do both when both are true.

Before finishing work that changed or clarified a decision here, write it down. A finding that stays in this session is a finding the next session pays for again.

## Bias to building

**The budgets have room and this section exists because that stopped being obvious.** The frame budget is 16ms and the tool runs at **2.4ms p50, 3.1ms p99**, which is five times the headroom. Refusing a feature on cost, in a tool with that much slack, needs the slack quoted alongside the cost or it is not an argument.

Four rows of Phase 8 spent a full session each and delivered a **decline**. One has since been reopened because *both* reasons it rested on were false: the first was an invariant whose budget could never have measured the case, and the second was an absence (*"the takeover does not enable focus reporting"*) that was one unwritten line rather than a fact about the world. Two other refusals in the same phase cited the same invariant and it did not reach either.

So, when a reader asks for something:

- **The default is build.** A refusal overrides the person whose product this is, so it needs a reason that survives being checked, not one that merely sounds sound.
- **Before citing an invariant, quote the row's own words and show it reaches this case.** I1's budget is *0 wakeups while idle*; neither pointer motion nor a held button is idle. A budget cited without its current headroom is a mood, not a measurement.
- **Check facts about the world against the world.** Read the dependency in `~/.cargo/registry`, and search the web. *"No API reports that"* is the claim most likely to be a year out of date, and it has been wrong here twice.
- **A reason that collapses reopens the question.** It does not get replaced by a better reason for the same conclusion. That happened to [#123](https://github.com/breferrari/vigia/issues/123) and the decline outlived both of its bases.
- **Reach a decline early or not at all.** The reason either holds under checking or it does not, and that is cheap. Hours spent after that point are spent justifying, and the tell is prose getting longer while the argument does not get stronger.
- **Size the rigor to the surface, and do not escalate past it.** Look and feel is `/simplify` plus a screenshot; the audit loop is for the frame path and the invariants. `ROADMAP.md` has said so since Phase 8 opened and it was overridden anyway.

Refusals deserve more scrutiny than builds, not less: a bad build is loud and a bad refusal is silent, so the mistake that survives longest here is the one no gate can see.

## House rules

- **No agent-session artifacts in anything that lands in the repo.** No `claude.ai/code/session_*` URL, no `Claude-Session:` trailer, no local absolute path — not in commit messages, PR or issue bodies, or files. `Co-Authored-By:` is fine and wanted.
- **No em-dashes** in anything published under Brenno's name: README prose, release notes, issue and PR bodies, commit messages. Use a period, a comma, a colon, or parentheses.
- Probe capability by behaviour, never by asking. A single green run is not evidence when the defect is non-deterministic.
- Verify the whole artifact, not just the property you were fixing.
