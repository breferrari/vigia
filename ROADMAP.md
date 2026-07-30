# vigia — Roadmap

**This file answers "what is next".** `SPEC.md` answers "what is true and why". Code
is written against the spec; work is taken from here.

Every task links to an issue, so "done" has an external source of truth (issue
closed, PR merged) rather than a self-report. If this file and the issues
disagree, the issues win and this file is stale: fix it in the same pass.

Status legend: ✅ done · 🔨 in progress · ⬜ not started

---

## Principles

Each of these is a filter you can quote back at a proposal to kill or delay it.
If a line here cannot do that, it is ornament and should be cut.

1. **Monitor, not reviewer.** If a change makes vigia a better tool to *sit down
   and review with*, at the cost of being correct-while-ignored, it is wrong.
2. **The budgets are the product.** A feature that cannot hold the frame budget
   is not a feature, it is a regression with a changelog entry.
3. **An invariant without a failing test is a wish.** Nothing counts as landed
   until a test fails when it is violated.
4. **Pure Rust, no C toolchain.** Any dependency pulling `cc`, `cmake` or
   `bindgen` breaks static Linux builds and Windows tier-1. That is a spec
   change, argued in the spec, never an implementation detail.
5. **Measure, never assume.** A type signature is not evidence. A single green
   run is not evidence. Numbers or it did not happen.

## Non-goals, permanent

Not "later". Never. Listed so the debate does not have to recur.

- **Staging, committing, rebasing.** Reviewer-class, and each would cost an
  invariant. Use a git client.
- **Branch and commit browsing.** Same.
- **Annotations and comment threads.** Reviewer-class by definition.
- **AI features of any kind.** The tool watches files. It does not summarise,
  explain or judge them.
- **Remote operations.** No fetch, no push, no network.
- **A GUI.** Terminal only.

---

## Phase 1 — core engine

Milestone: [Phase 1](https://github.com/breferrari/vigia/milestone/1)

Prove `gix` before anything is built on top, since everything sits on it. No TUI.

| | Task | Issue |
|---|---|---|
| ✅ | `gix` gives working-tree-vs-index diffs at fidelity and speed | closed by the Phase 1 spike, evidence in `SPEC.md` §10 |
| ✅ | I1 redraw is event-driven, never a timer | [#1](https://github.com/breferrari/vigia/issues/1) |
| ✅ | I2a the frame path never re-diffs what did not change | [#2](https://github.com/breferrari/vigia/issues/2) |
| ✅ | Gate every Phase 1 budget in CI (I4, I7, I9) | [#3](https://github.com/breferrari/vigia/issues/3) |

**Phase 1 is closed.** The engine holds every budget it was written against, and
`gix` was the right call: revalidating a 100-file frame over a 100k-line diff
costs 3.93ms p99 and reads nothing, against 18.28ms and 3.6 MiB to recompute it.

## Phase 2 — minimum monitor

Milestone: [Phase 2](https://github.com/breferrari/vigia/milestone/2)

| | Task | Issue |
|---|---|---|
| ⬜ | The `ratatui` + `crossterm` shell | [#9](https://github.com/breferrari/vigia/issues/9) |
| ⬜ | I5 correct with zero interaction | [#6](https://github.com/breferrari/vigia/issues/6) |
| ⬜ | I6 legible at 40 columns | [#7](https://github.com/breferrari/vigia/issues/7) |
| ⬜ | I8 terminal restored exactly on exit | [#8](https://github.com/breferrari/vigia/issues/8) |
| ⬜ | I2b re-highlight only changed hunks (`syntect`) | [#4](https://github.com/breferrari/vigia/issues/4) |
| ⬜ | I3 flat resources over days (soak) | [#5](https://github.com/breferrari/vigia/issues/5) |

## Phase 3 — glanceability

Milestone: [Phase 3](https://github.com/breferrari/vigia/milestone/3)

| | Task | Issue |
|---|---|---|
| ⬜ | Sparklines, heat strips, counters, pulse | [#10](https://github.com/breferrari/vigia/issues/10) |
| ⬜ | Theming, with a 256-colour degradation path | [#11](https://github.com/breferrari/vigia/issues/11) |

## Phase 4 — distribution

Milestone: [Phase 4](https://github.com/breferrari/vigia/milestone/4)

| | Task | Issue |
|---|---|---|
| ⬜ | `cargo-dist`, crates.io, Homebrew tap | [#12](https://github.com/breferrari/vigia/issues/12) |

---

## Deferral shelf

Items that surfaced mid-phase and would have derailed the block they surfaced
in. Deferral is a first-class outcome recorded here, not a dropped ball and not
scope creep absorbed silently. Each one carries the phase it moved to.

| Item | Surfaced | Moved to | Why |
|---|---|---|---|
| Multi-worktree view: several agent sessions at once | Market pass, 2026-07-30 | Phase 5 | The strongest differentiator after glanceability, and the most monitor-shaped. Needs the single-worktree frame path to be cheap first, or it multiplies a cost we have not paid down |
| Jujutsu and Sapling support | Market pass, 2026-07-30 | Phase 5 | Git is the thesis. A second VCS before the first one is beautiful is scope, not reach |
| A truncated `.git/index` aborts instead of reporting ([#13](https://github.com/breferrari/vigia/issues/13)) | I2a, 2026-07-30 | Phase 2, with I8 | A `gix` defect, not a frame-path one: an index shorter than the object hash underflows a slice and panics, and `panic = "abort"` makes it uncatchable. The local defences are worse than the problem, and terminal restoration on panic is settled by I8 anyway. What #2 gates is the part vigia owns: given an error, the previous frame survives it |

## Pull-forward log

Items that moved into an *earlier* phase than planned. Recorded for the same
reason as deferrals: movement should be visible. Over time the balance of this
list against the shelf says whether the plan was too ambitious or too cautious.

| Item | Moved | Why |
|---|---|---|
| `notify` named as a dependency in `SPEC.md` §6 | Into Phase 1 | I1 requires filesystem events rather than a timer, so the choice could not wait for the shell. Cross-platform C-toolchain-free status verified on all three tier-1 targets at the same time |
| I2 split into I2a and I2b | During Phase 1 | The original I2 conflated incremental re-diffing with incremental re-highlighting. Different dependencies, different phases, and Phase 1 could not close while one number meant two things |

---

## How work is taken

One task, taken to done, before the next. See `.claude/skills/take-next/`.
