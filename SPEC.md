# vigia — Specification

Status: **v0, 2026-07-30.** Source of truth. Code is written against this
document; see `CLAUDE.md` § Method for the drift rule.

---

## 1. Problem

Running a full git TUI in a pane beside an AI coding agent, to watch changes as
they land, is the wrong tool. A multi-panel git client spends the pane on
branches, commits, stashes and status. In a pane already halved against the
agent, every panel is a few lines tall and names truncate.

What is wanted: **the live diff, fullscreen, auto-updating, scrollable,
mouse-driven, and beautiful.** Nothing else.

## 2. Product class — the load-bearing decision

`vigia` is a **monitor**, not a reviewer. That single distinction generates every
budget below, and the budgets are the product.

| | Reviewer | **Monitor (`vigia`)** |
|---|---|---|
| How it starts | Launched per review | Already open |
| Interaction | The point | Rare or never |
| Correct when untouched | No | **Required** |
| Runtime | Minutes | **Days** |
| Latency budget | Seconds | **A frame** |

`btop` is the reference for what monitor-class feels like: you read state from
shape and colour, glance away, glance back, and never configure anything.

Design rationale beyond this section is recorded outside the codebase; call
`recall` when a decision needs it.

## 3. Invariants

Numbered because tests reference them. Each must have a test that fails on
violation. **An invariant without a failing test is a wish.**

Budgets are **absolute** and chosen to be defensible on their own terms, not
relative to any other tool.

| # | Invariant | Budget | How it is proven |
|---|---|---|---|
| **I1** | Redraw is **event-driven**, never a fixed timer. No filesystem event and no git index change means no work. | **0 wakeups** while idle | CPU sampled over a 60s idle window; assert no render calls |
| **I2a** | **Re-diffing is incremental** — the frame path never re-diffs a file that did not change. | re-diff cost ∝ what changed, **not** worktree size | Assert the re-diff count and byte count for a single-line edit in a large worktree |
| **I2b** | **Re-highlighting is incremental** — only changed hunks are re-parsed. | re-parse ∝ edit size, **not** file size | Assert the re-parse count and byte count for a single-line edit in a large file |
| **I3** | **Flat resources over days.** No unbounded growth in RSS, file handles, or temp files. | **RSS drift < 5%** over 24h; **zero** temp files retained | Soak test: 24h of synthetic edits, RSS sampled every 5 min |
| **I4** | **Streams, never buffers.** First paint is independent of total diff size. | **first paint < 100ms** on a 100k-line diff | `criterion`, gated in CI |
| **I5** | **Correct with zero interaction.** Auto-follows the newest change and scrolls to it, untouched. | — | Scripted edit sequence, snapshot the frame, no input given |
| **I6** | **Legible at 40 columns.** No horizontal overflow, no truncated-to-useless labels. | — | Snapshots at 40 / 80 / 120 columns |
| **I7** | Startup to first paint is imperceptible. | **< 50ms** | Timed, gated in CI |
| **I8** | Terminal restored exactly on exit — including `SIGINT` and panic. | — | Alternate-screen assertions; panic hook test |
| **I9** | Steady-state frame time holds 60fps under continuous edits. | **< 16ms** p99 | `criterion` under a synthetic edit storm |

A regression past any budget **fails the build.**

> [!note] Why I2 is two numbers
> It was written as one, reading "re-highlighting is incremental", and that
> conflated two invariants with **different dependencies and different phases**.
> Incremental re-*diffing* needs only `gix` and is Phase 1. Incremental
> re-*highlighting* needs `syntect`, which Phase 1 does not include, so Phase 1
> could not close while one number meant both. Split deliberately rather than
> absorbed silently, per the drift rule. Measurement that forced it: re-diffing
> every changed file costs **18.58ms p99** on a 100k-line diff against **3.27ms**
> for a single file, so I2a is load bearing rather than an optimisation.
> Issues [#2](https://github.com/breferrari/vigia/issues/2) and
> [#4](https://github.com/breferrari/vigia/issues/4).

## 4. Scope

**In:** working-tree diff (unstaged by default), event-driven refresh, follow
mode, scroll (keyboard + mouse wheel), syntax highlighting, per-file churn
visualisation, responsive layout, theming.

**Out of v1, deliberately:** staging, committing, rebasing, branch or commit
browsing, annotations, comment threading, AI features, remote operations. Each is
reviewer-class and each would cost an invariant.

**Deferred, not rejected:** multi-worktree view (several agent sessions at once —
the strongest differentiator after glanceability, and the most btop-shaped);
Jujutsu and Sapling support.

## 5. The differentiator: glanceability

`btop`'s real achievement is that state is readable from **shape and colour**
without reading text. `vigia`'s translation:

- per-file **churn sparkline** — change density over time
- a **heat strip** locating change within the file
- live **+/− counters**
- a **visual pulse** on what just changed

This is what makes it a monitor rather than a narrow diff view, and it is where
design effort goes once the invariants hold.

## 6. Architecture

Cargo workspace, two crates:

- **`vigia-core`** — library. Git (`gix`), diff modelling, incremental
  highlighting (`syntect`), filesystem events (`notify`), the watch and coalesce
  engine. **No terminal I/O, no ratatui.** Every invariant except I6 and I8 is
  testable here, headlessly.
- **`vigia`** — binary. `ratatui` + `crossterm` shell: input, layout, theming.
  Thin by design, so the TUI stays swappable and the engine stays provable.

The split is a dependency decision, not a hedge: the TUI renders whatever the
core produces, so the core has to work first or the TUI is being built on sand.

`notify` is named here because I1 requires filesystem events rather than a
timer, and each platform delivers them differently: `inotify` on Linux,
`FSEvents` on macOS, `ReadDirectoryChangesW` on Windows. `notify` is the
standard Rust abstraction over those three, and it keeps the binary pure Rust.
Verified 2026-07-30 against `x86_64-pc-windows-msvc`,
`x86_64-unknown-linux-musl` and `aarch64-apple-darwin`: no `cc`, `cmake` or
`bindgen` in any of the three graphs. The `-sys` crates it pulls
(`inotify-sys`, `fsevent-sys`, `windows-sys`) are FFI declarations against
facilities the OS already ships, so they compile no C.

**Coalescing stays ours.** `notify` has a companion debouncer crate, and taking
it would move coalesce policy out of `vigia-core`, which is the one place I1 is
testable. `notify` supplies raw events and nothing else.

**The frame path keeps its diffs between frames.** I2a forbids re-diffing a file
that did not change, so the core holds the previous frame's diff per path and
revalidates instead of recomputing. Validity is decided from three things that
cost no file read: the index blob the change names, the kind of change, and a
`stat` of the working-tree file. Content is never hashed to decide this, because
hashing is the read I2a exists to avoid.

A `stat` on its own is not proof, and the gap is the one git calls *racily
clean*: two writes of the same length inside a single modification-time granule
are indistinguishable by `stat`. So a fingerprint is trusted only when the
modification time it records is **strictly older than the moment the content was
read**, and anything else is re-diffed. That trades a redundant diff of a file
being actively written, which is a file that changed anyway, for never showing a
stale one. It assumes the wall clock does not step backwards mid-frame, which is
the assumption git's own index makes.

## 7. Testing

- **Snapshot tests over `ratatui::backend::TestBackend` with `insta`** — render
  frames into an in-memory buffer, snapshot as text. This is what makes I5 and I6
  assertable at all: the UI becomes diffable text.
- **`criterion`** for I4, I7 and I9, **tracking rather than gating.** Criterion
  compares a run against a saved baseline, and the budgets in §3 are absolute,
  so it is the right instrument for "this got 20% slower" and the wrong one for
  a pass/fail line. Compiled in CI so the benchmarks cannot rot; not timed
  there, because a shared runner cannot produce a number worth comparing.
- **Budget gates** in `crates/vigia-core/tests/budgets.rs`, in **two tiers**,
  because an absolute wall-clock threshold is a strong instrument on a known
  machine and a weak one on a hosted runner:
  - *Structural* gates compare the engine against itself across fixtures that
    differ only in how much changed. They are ratios and exact byte counts, so
    they are hardware-independent, take **no slack**, and are what actually
    catches a regression. Making the frame path re-read every changed file is a
    5x wall-clock change that a generous threshold waves through, and a 100x
    byte-count change that these cannot.
  - *Absolute* gates hold the wall clock to §3. Release only, since the budgets
    were set against optimised code, and with a slack multiplier
    (`VIGIA_BUDGET_SLACK`, default 1) so a shared runner's variance does not
    read as a code regression.
- **Steady-state budgets are sampled after a warmup**, and over enough frames
  for a percentile to be one. I9 is a claim about steady state, so the cold path
  is outside its scope by definition; measured cold frames run ~40ms against a
  warm p99 of ~3ms, and at 30 samples a nearest-rank p99 is just the maximum.
- **A CI guard fails the build if `cc`, `cmake` or `bindgen` enters the
  dependency graph** on any tier-1 target, plus a musl build asserting the
  binary links no shared libraries. The pure-Rust constraint is what makes
  musl-static and Windows cheap, so it is enforced rather than trusted.
- **Soak test** for I3, scheduled rather than per-commit.
- **`proptest`** over diff parsing and hunk-boundary logic.

## 8. Phases

Live status, issue-linked, is in [`ROADMAP.md`](ROADMAP.md). This section is the
shape; that file is the state. Work is taken one task at a time via
`.claude/skills/take-next/`.

**Phase 1 — core engine.** `vigia-core` plus a `main` that prints frame timings.
Prove `gix` gives working-tree-vs-index diffs at the fidelity and speed needed —
it is the least-precedented dependency in the stack and everything sits on it.
Land **I1, I2a, I4, I9**, each gated. No TUI.

**Phase 2 — minimum monitor.** ratatui + crossterm shell. Follow mode (I5),
scroll, mouse, exit safety (I8), 40-column layout (I6). Plus **I2b** (needs
`syntect`) and **I3** (soak). Snapshot suite.

**Phase 3 — glanceability.** Section 5, plus theming.

**Phase 4 — distribution.** `cargo-dist`, crates.io publish, personal Homebrew
tap, prebuilt binaries on GitHub releases.

**Phase 5 — deferred items**, only if daily use asks for them.

## 9. Distribution

- **crates.io** — `cargo install vigia`. A name **cannot be reserved; it must be
  published**, and a publish is **permanent** (`cargo yank` hides a version from
  new dependents, it does not delete it, and the name stays taken). So the first
  publish should be a crate that does the minimal real thing. Verified free
  2026-07-30.
- **Homebrew** — `homebrew-core` **cannot be reserved** and requires notability:
  ≥30 forks **or** ≥30 watchers **or** ≥75 stars, plus a stable versioned
  release. Until then a **personal tap** (`breferrari/homebrew-tap`) gives
  `brew install breferrari/tap/vigia` immediately, is fully under our control, and
  is what `cargo-dist` generates.
- **GitHub releases** — prebuilt binaries per platform via `cargo-dist`.
- No domain. Not needed for a CLI.

## 10. Open questions

- [x] ~~Does `gix` cover working-tree-vs-index diff at the fidelity and speed
      needed?~~ **Answered 2026-07-30: yes.** Hunk boundaries match
      `git diff -U3` exactly, with git used as the oracle at every edit gap from
      0 to 10 lines. On a 100k-line diff, release build: first change available
      in 3.84ms against the 100ms I4 budget, single-file re-diff 3.27ms p99
      against the 16ms I9 budget, process start to first paint 20.37ms against
      the 50ms I7 budget. The dependency that could have forced a rethink did
      not.
- [ ] Rename tracking cannot stream. Pairing a deletion with an addition needs
      the whole walk, so with it on the first change arrives at 97% of the walk
      and time-to-first equals time-to-last. It is on by default anyway, because
      reporting a move as an unrelated delete plus add misdescribes what the
      agent did. Confirm against a week of real use, or paint without renames
      and reconcile them on a later frame.
- [x] ~~Re-diffing every changed file costs 18.58ms p99 on a 100k-line diff,
      over the I9 budget, against 3.27ms for a single file.~~ **Closed
      2026-07-30:** I2a is enforced. The frame path revalidates from the index
      blob, the change kind and a `stat`, so a single-line edit in a 100-file
      worktree recomputes exactly one diff and reads exactly that file.
- [ ] The frame path walks status to completion before it reports a file list,
      so it does not stream the way the raw change iterator does. Today that
      costs nothing, because rename tracking cannot stream either and is on by
      default. If renames ever move off the first frame, this becomes the thing
      that forfeits streaming and it has to be revisited with them.
- [ ] Is `syntect` fast enough incrementally to hold I2b, or does it force
      tree-sitter — and with it a C toolchain — back in?
- [ ] Default view: unstaged only, or working-tree-vs-HEAD? Unstaged is the
      thesis; confirm against a week of real use.
- [ ] Windows: supported target or best-effort? Truecolor needs Win10+, and
      legacy conhost degrades.
