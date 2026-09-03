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

**Never run `find /` on Windows.** Under Git Bash `/` is the MSYS root, which mounts the Windows registry as directories under `/proc`, so the walk never terminates, and `~/.cargo` is not under it anyway. `.claude/scripts/scan-guard.mjs` refuses the call. Bound any exploratory scan with `timeout`, and prefer Glob or Grep pointed at an explicit path.

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

**If a write is refused for tool-call markup, retry once smaller in total size** (the required fields, prose trimmed), then file the note by hand under `projects/vigia/notes/` and comment on [breferrari/obsidian-mind#244](https://github.com/breferrari/obsidian-mind/issues/244). The same fold hits `remember`. Fields you drop survive as prose but stop being queryable.

Rule of thumb: **a `gix` limitation that would bite any Rust project is a `remember`** — and note that it is `scope: "platform"` with `platforms: ["rust"]`, not `general`, which is exactly the call this section exists to get right. **"Landed the watch engine and here is what it cost" is a `record_work`.** Do both when both are true.

Before finishing work that changed or clarified a decision here, write it down. A finding that stays in this session is a finding the next session pays for again.

## Bias to building

**The default is build, and a refusal needs a reason that survives being checked.** `SPEC.md` §0 carries the rules for citing the record, and every reader passes it: a ruling is its reason plus a date, a reason naming an absence expires fastest, an invariant's own words have to reach the case, a budget arrives with its current headroom, and a reason that collapses reopens the question. Four rows of Phase 8 spent a session each on a decline, and one was reopened because both of its reasons were false. The frame path runs at **2.4ms p50** against a 16ms budget, so a cost cited without that headroom is a mood, not a measurement.

Three rules that are this file's rather than the spec's:

- **Measure the whole, not the part, and before the cost restricts anything.** Two fixtures, the same workload with and without the thing being priced, interleaved so a loaded machine moves both. A zero from a clock is a quantum until timed across enough repetitions to clear it: `GetThreadTimes` steps at 15.625ms on Windows and read a 2.60ms burst as nothing. A cap is a feature restriction and needs the same bar as a refusal; measured in situ, a sizing cap read 17.93ms against 18.43ms unsized and was deleted.
- **What ships is what was asked for and nothing narrower.** A bound taken from a neighbouring tool's default is that tool's decision, not a ruling here ([#272](https://github.com/breferrari/vigia/issues/272) imported `delta`'s wrap cap, and the reader had to say twice that he never asked for it). A limit nobody asked for is a refusal wearing a yes, and it is the harder kind to see.
- **A session's own prior decision is a record of what was done, not permission withheld.** Name the reader's decisions; never cite a session's as a constraint on him.

Size the rigor to the surface: look and feel is `/simplify` plus a screenshot, and the audit loop is for the frame path and the invariants.

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

- **No agent-session artifacts in anything that lands in the repo.** No `claude.ai/code/session_*` URL, no `Claude-Session:` trailer, no local absolute path — not in commit messages, PR or issue bodies, or files. `Co-Authored-By:` is fine and wanted. `.claude/scripts/leak-guard.mjs` refuses a `gh` publish or a `git commit` carrying one, and `sh .claude/scripts/selftest.sh` exercises every guard offline.
- **No em-dashes** in anything published under Brenno's name: README prose, release notes, issue and PR bodies, commit messages. Use a period, a comma, a colon, or parentheses.
- Probe capability by behaviour, never by asking. A single green run is not evidence when the defect is non-deterministic.
- Verify the whole artifact, not just the property you were fixing.
- **A comment exists where the code cannot explain itself.** Why the obvious approach is wrong, an invariant a caller must hold, a cost invisible at the call site. Not a restatement of the code, not issue numbers, not ruling ids, not the history of the comment's own corrections. Those belong in the commit message and the tracker, which already hold them. The test: would this make sense to a reader in two years with no knowledge of the session that wrote it? A docblock longer than the item it documents means one of the two is wrong.
- **A title says what is broken or what to build.** One clause, no *because*. The body carries the reason. `Band saturates at half on a steady worktree`, not `A steady worktree saturates half the band, because the factor above the mean was never measured on this signal`.
- **Nothing prose stops at a column. One paragraph is one line.** Markdown files, issue and PR bodies, commit message bodies: no hard wrapping anywhere. GitHub turns a single newline in a body into a real line break, so a paragraph wrapped at 80 arrives broken mid-sentence — 811 forced breaks across sixteen of this repository's PRs before anyone measured it. Gated by `register.rs::no_prose_paragraph_is_hard_wrapped` over every tracked `.md`, because the rule was written down twice and lost twice: an instruction cannot beat a corpus, and these documents held 7,351 hard-wrapped lines that every session reads before it writes anything. **Code comments are the deliberate exception** — nothing renders them, so a break there corrupts nothing, and they sit beside code held near a hundred columns where one long line reads worse. Check a body with `gh api repos/OWNER/REPO/pulls/N -H "Accept: application/vnd.github.html+json" --jq '.body_html' | grep -c '<br'`.
