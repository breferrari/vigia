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
`gix` was the right call. On a 100-file, 100k-line diff: a real frame under
continuous edits is 3.93ms p99 to revalidate and 6.97ms p99 with a file edited
before every frame, against 18.28ms and 3.6 MiB for a cold frame with nothing to
reuse. The 16ms budget holds with room, and only the cold frame breaches it.

## Phase 2 — minimum monitor

Milestone: [Phase 2](https://github.com/breferrari/vigia/milestone/2)

| | Task | Issue |
|---|---|---|
| ✅ | The `ratatui` + `crossterm` shell | [#9](https://github.com/breferrari/vigia/issues/9) |
| ⬜ | I5 correct with zero interaction | [#6](https://github.com/breferrari/vigia/issues/6) |
| ⬜ | I6 legible at 40 columns | [#7](https://github.com/breferrari/vigia/issues/7) |
| ⬜ | I8 terminal restored exactly on exit | [#8](https://github.com/breferrari/vigia/issues/8) |
| ✅ | A truncated `.git/index` aborts instead of reporting | [#13](https://github.com/breferrari/vigia/issues/13) |
| ⬜ | I2b re-highlight only changed hunks (`syntect`) | [#4](https://github.com/breferrari/vigia/issues/4) |
| ⬜ | I3 flat resources over days (soak) | [#5](https://github.com/breferrari/vigia/issues/5) |

**The shell is in, so the rest of this phase has something to render into.** It
draws the working-tree diff, follows the watch engine's ticks, scrolls by keyboard
and wheel, and holds its own half of I4: one screenful reads only the files it
draws, gated across two fixtures in `crates/vigia/tests/reads.rs`.

**Take [#8](https://github.com/breferrari/vigia/issues/8) next.** The restoration
it proves is already implemented, in `crates/vigia/src/terminal.rs`, and it is the
one module in the shell with no test at all: raw mode, the alternate screen, mouse
capture and the panic hook are all outside a `TestBackend`, which is what the rest
of the suite is built on. It is also the only thing standing between the shell
being usable and being safe to leave running, and #13's abort is covered by it.

[#7](https://github.com/breferrari/vigia/issues/7) then has a baseline to argue
with: there are 40- and 80-column snapshots already, and what I6 still needs is
the assertions that make it an invariant rather than a screenshot. A diff line
does lose its tail at 40 columns today.

**[#6](https://github.com/breferrari/vigia/issues/6) is blocked on a decision, not
on code.** I5 promises the view follows the newest change untouched, and no follow
mode exists yet, so what happens when the reader scrolls is undecided — `SPEC.md`
§11.2 **B1**. Implementing #6 first would settle it accidentally inside a snapshot
test, which is how a behaviour becomes permanent without anyone choosing it. Rule
on B1, then take #6. **B2** — which file wins when a batch changes several — is
the same decision one layer down and lands with it.

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

## Phase 5 — deferred findings

Milestone: [Phase 5](https://github.com/breferrari/vigia/milestone/5)

Everything on the deferral shelf below has a milestone here, so shelved work is
still reachable by a milestone-filtered query rather than only readable in prose.
The shelf carries the *reason*; this table carries the *state*.

| | Task | Issue |
|---|---|---|
| ⬜ | A symlink diffs as its target's contents | [#15](https://github.com/breferrari/vigia/issues/15) |
| ⬜ | The fingerprint cannot see a timestamp-preserving write | [#16](https://github.com/breferrari/vigia/issues/16) |
| ⬜ | Two paths differing outside UTF-8 collapse onto one cache key | [#17](https://github.com/breferrari/vigia/issues/17) |
| ⬜ | A frame reads a whole file to discover it is binary | [#18](https://github.com/breferrari/vigia/issues/18) |
| ⬜ | An idle frame is one `stat` per changed file | [#19](https://github.com/breferrari/vigia/issues/19) |
| ✅ | `take-next`: pre-flight the spec against the tracker | [#20](https://github.com/breferrari/vigia/issues/20) |

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
| A symlink diffs as its target's contents ([#15](https://github.com/breferrari/vigia/issues/15)) | I2a, 2026-07-30 | Phase 2, with I2b | Pre-existing in `Worktree::diff`, which reads through the link where git stores the target *path*. Demonstrated against git as the oracle. Out of scope for I2a, which only caches whatever the primitive returns, but coupled to it: the fix has to move the fingerprint to `symlink_metadata` in the same change or a repoint between equal-sized targets reads as unchanged. Lands with the fidelity work I2b needs anyway |
| The fingerprint cannot see a timestamp-preserving write ([#16](https://github.com/breferrari/vigia/issues/16)) | I2a, 2026-07-30 | Phase 2 | `cp -p`, `rsync -t` and `touch -r` keep the length and put the modification time back, and no margin can catch that. Git carries the inode change time for it; `std` exposes no equivalent on Windows on stable, so closing it means depending on `windows-sys` directly, which is a spec decision rather than an implementation detail. Shipping the Unix half alone was rejected: a guarantee that differs by tier-1 platform is worse than one stated uniformly |
| Two paths differing outside UTF-8 collapse onto one cache key ([#17](https://github.com/breferrari/vigia/issues/17)) | I2a, 2026-07-30 | Phase 2 | `to_str_lossy` makes `FileChange::path` both the filesystem identity and the display string, and those are different jobs. The read half predates the frame path. Fixing it changes a published type, so it wants deciding rather than patching |
| A frame reads a whole file to discover it is binary ([#18](https://github.com/breferrari/vigia/issues/18)) | I2a, 2026-07-30 | Phase 2 | 64 MiB read and 16.24ms for a file the first 8000 bytes already condemn, with no size cap on either side. Pre-existing in `Worktree::diff`. Belongs with I3, which is where a memory ceiling gets decided |
| An idle frame is one `stat` per changed file ([#19](https://github.com/breferrari/vigia/issues/19)) | I2a, 2026-07-30 | Phase 2, with the shell | 36.71ms at 2000 changed files against a 16ms budget, almost all of it syscalls. The fix is to revalidate what is drawn rather than everything, which I4 already licenses and which needs a UI that knows what is visible. Not a defect in the rule, a consequence of the test having to materialise every file to avoid passing vacuously |

## Pull-forward log

Items that moved into an *earlier* phase than planned. Recorded for the same
reason as deferrals: movement should be visible. Over time the balance of this
list against the shelf says whether the plan was too ambitious or too cautious.

| Item | Moved | Why |
|---|---|---|
| `notify` named as a dependency in `SPEC.md` §6 | Into Phase 1 | I1 requires filesystem events rather than a timer, so the choice could not wait for the shell. Cross-platform C-toolchain-free status verified on all three tier-1 targets at the same time |
| Terminal restoration implemented with the shell, ahead of I8 | Into [#9](https://github.com/breferrari/vigia/issues/9) | Not scope creep and not I8 done early. A shell that takes the alternate screen without giving it back is not shippable at any stage, and `panic = "abort"` means a `Drop` alone cannot, so the panic hook had to land with the code that takes the screen. What [#8](https://github.com/breferrari/vigia/issues/8) still owns is the whole of its proof, which is also the whole of the invariant |
| I2 split into I2a and I2b | During Phase 1 | The original I2 conflated incremental re-diffing with incremental re-highlighting. Different dependencies, different phases, and Phase 1 could not close while one number meant two things |

---

## How work is taken

One task, taken to done, before the next. See `.claude/skills/take-next/`.
