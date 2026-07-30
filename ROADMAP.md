# vigia — Roadmap

**This file answers "what is next".** `SPEC.md` answers "what is true and why". Code is written against the spec; work is taken from here.

Every task links to an issue, so "done" has an external source of truth (issue closed, PR merged) rather than a self-report. If this file and the issues disagree, the issues win and this file is stale: fix it in the same pass.

Status legend: ✅ done · 🔨 in progress · ⬜ not started

---

## Principles

Each of these is a filter you can quote back at a proposal to kill or delay it. If a line here cannot do that, it is ornament and should be cut.

1. **Monitor, not reviewer.** If a change makes vigia a better tool to *sit down and review with*, at the cost of being correct-while-ignored, it is wrong.
2. **The budgets are the product.** A feature that cannot hold the frame budget is not a feature, it is a regression with a changelog entry.
3. **An invariant without a failing test is a wish.** Nothing counts as landed until a test fails when it is violated.
4. **Pure Rust, no C toolchain.** Any dependency pulling `cc`, `cmake` or `bindgen` breaks static Linux builds and Windows tier-1. That is a spec change, argued in the spec, never an implementation detail.
5. **Measure, never assume.** A type signature is not evidence. A single green run is not evidence. Numbers or it did not happen.

## Non-goals, permanent

Not "later". Never. Listed so the debate does not have to recur.

- **Staging, committing, rebasing.** Reviewer-class, and each would cost an invariant. Use a git client.
- **Branch and commit browsing.** Same.
- **Annotations and comment threads.** Reviewer-class by definition.
- **AI features of any kind.** The tool watches files. It does not summarise, explain or judge them.
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

**Phase 1 is closed.** The engine holds every budget it was written against, and `gix` was the right call. On a 100-file, 100k-line diff: a real frame under continuous edits is 3.93ms p99 to revalidate and 6.97ms p99 with a file edited before every frame, against 18.28ms and 3.6 MiB for a cold frame with nothing to reuse. The 16ms budget holds with room, and only the cold frame breaches it.

## Phase 2 — minimum monitor

Milestone: [Phase 2](https://github.com/breferrari/vigia/milestone/2)

| | Task | Issue |
|---|---|---|
| ✅ | The `ratatui` + `crossterm` shell | [#9](https://github.com/breferrari/vigia/issues/9) |
| ✅ | I5 correct with zero interaction | [#6](https://github.com/breferrari/vigia/issues/6) |
| ✅ | I6 legible at 40 columns | [#7](https://github.com/breferrari/vigia/issues/7) |
| ✅ | I8 terminal restored on every exit the process controls | [#8](https://github.com/breferrari/vigia/issues/8) |
| ✅ | A truncated `.git/index` aborts instead of reporting | [#13](https://github.com/breferrari/vigia/issues/13) |
| ✅ | `vigia .` never redrew: a relative root matched no event path | [#30](https://github.com/breferrari/vigia/issues/30) |
| ✅ | I2b re-highlight only changed hunks (`syntect`) | [#4](https://github.com/breferrari/vigia/issues/4) |
| ⬜ | I3 flat resources over days (soak) | [#5](https://github.com/breferrari/vigia/issues/5) |

**The shell is in, so the rest of this phase has something to render into.** It draws the working-tree diff, follows the watch engine's ticks, scrolls by keyboard and wheel, and holds its own half of I4: one screenful reads only the files it draws, gated across two fixtures in `crates/vigia/tests/reads.rs`.

**The shell is now safe to leave running.** [#8](https://github.com/breferrari/vigia/issues/8) closed the last untested module: the takeover is data, giving it back is asserted as its exact inverse, and a half-finished `Session::enter` undoes exactly what it took. Seven mutations, each killed by a named test. It also settled what I8 can honestly promise: raw mode means Ctrl-C is a key event and never a signal, so an externally delivered `kill` is out of reach without a dependency `SPEC.md` does not name. That is [#24](https://github.com/breferrari/vigia/issues/24), on the shelf below.

**The phase's correctness half is done.** [#6](https://github.com/breferrari/vigia/issues/6) landed I5: the view moves itself to the file that just changed, with nothing pressed. It was blocked on a decision rather than on code, so **B1 and B2 were ruled first, in their own commit**, and the implementation was written against the ruled spec. Ruling second would have settled the question by accident inside a snapshot test.

What ruling it exposed is worth more than the ruling. "The newest change" needs a recency signal, and the obvious one — `stat` every changed file — is [#19](https://github.com/breferrari/vigia/issues/19)'s recorded breach of I9 at scale. The filesystem event already names the path, and the gitignore filter was already resolving it and throwing it away, so `Tick` now carries it and following costs no read, no `stat` and no diff. The cheap answer and the correct one coincided, which is luck rather than design and is why it is written down.

**I6 is in, and it turned out to be a layout question rather than a text-shortening one.** [#7](https://github.com/breferrari/vigia/issues/7) inherited the collision I5 left: at forty columns, follow on, the footer drew `q quit · f follow · jk scr`, a hint cut mid-word in the **default** state. The answer was to stop shortening. `SPEC.md` §11.1 now rules one sentence the whole layout follows from, **a thing made of items breaks, a thing made of characters marks its edge, and content is neither**, so the hint bar takes a second footer line rather than losing a letter, everything else says which end it lost (`…` on the left for a file path, `›` on the right for everything else), and a clipped diff line is marked rather than counted as a truncated label. That last one closes the "a diff line still loses its tail" question this file used to carry.

**The invariant is now assertable, which it was not before.** `crates/vigia/tests/legibility.rs` sweeps every width from 1 to 120 with twelve gates, because a snapshot records a width and asserts no rule. The 40 / 80 / 120 triple §3 names is complete for the first time; 120 was missing entirely. Two things the mutation pass turned up are worth keeping: the row helper was reading `TestBackend`'s `Hidden by multi-width symbols` note instead of the row whenever a wide glyph was on screen, and the mark can be **swallowed** by a two-column glyph landing on the final column, which is a clipped line drawn as one that ends. Only a gate over double-width content reaches that, and twelve mutants now die to a named test with none surviving.

**I2b is in, and the open question it carried is answered: `syntect` holds, and tree-sitter stays out.** [#4](https://github.com/breferrari/vigia/issues/4) landed incremental re-highlighting. One screenful of Rust is 1.53ms against the 16ms frame budget, loading the grammars is 318µs against I7's 50ms, and a real shell frame under continuous edits over the 100-file, 100k-line fixture is **10.52ms p99** against 6.97ms for the core frame path alone. The dependency's own defaults were the trap: `syntect` selects `regex-onig`, which is oniguruma, so `cargo add syntect` alone would have put `cc` in the graph and cost the static musl binary and Windows tier-1.

**The number that decided the design was the one for doing it the obvious way.** Parsing a hunk whole costs 60.97ms for the 1006-line hunk the budget fixture produces, 3.8x over budget and paid on *every* frame under I9's own shape. So a hunk is parsed forward only as far as the screen has asked, and what was parsed is kept: the same finding I2a made about re-diffing, arrived at independently. Revalidation is a hash of the hunk rather than a counter the frame path bumps, because inside the two-second settle margin the frame path re-diffs an untouched file every frame and a counter would re-highlight files nobody edited.

**What it cost the reader: the diff signal narrowed to the sigil column.** `SPEC.md` §11.1 rules that highlighting follows the mockup literally, which means added, removed and context lines are coloured identically. What the picture uses to keep them apart is a row background tint and a left bar, and sixteen foreground-only colours can draw neither, so until [#11](https://github.com/breferrari/vigia/issues/11) lands truecolour the `+` and `−` carry it alone. That is a real loss against §5's glance thesis, recorded rather than absorbed, and §5.1 gained the two rows the mockup never had.

**A defect found by using the tool rather than by testing it.** [#30](https://github.com/breferrari/vigia/issues/30): `vigia .` drew one frame and then ignored every filesystem event for the rest of the session, silently. `gix` returns the path it was discovered with, so a relative argument leaves the worktree root as `"."`, and no event path ever begins with that. Ten watch tests missed it because `Scratch::worktree` always discovers by an absolute path, so the relative case had never been run once. Folded into this phase's work deliberately rather than deferred, because a monitor that does not refresh cannot be used to check anything else.

**What it cost the frame path: one row, at narrow widths only.** `body_height` takes the chrome and the changed-file count now, since the footer's height varies, and both it and the renderer plan through one function so the caller's row budget and the layout cannot drift. The height deliberately does **not** depend on a notice: a transient error that grew the footer would jog the reader's diff down a row and back, which is what I5 already ruled out for a resize.

## Phase 3 — glanceability

Milestone: [Phase 3](https://github.com/breferrari/vigia/milestone/3)

| | Task | Issue |
|---|---|---|
| ⬜ | Sparklines, heat strips, counters, pulse | [#10](https://github.com/breferrari/vigia/issues/10) |
| ⬜ | Theming, with a 256-colour degradation path | [#11](https://github.com/breferrari/vigia/issues/11) |

**This phase is mis-scoped and [#10](https://github.com/breferrari/vigia/issues/10) must split before anything here is taken.** Reading `assets/preview.svg` as the specification it already is (`SPEC.md` §5.1) turned up eight distinct pieces of work behind two rows, and #10 alone carries four features that share no implementation. `take-next` step 4 is explicit: *if the issue is genuinely two things, split the issue first.*

The correction that matters is not the count. **It is that this is not a rendering phase** (`SPEC.md` §5.2). Two of its headline elements need retained state in `vigia-core`, so they move invariants rather than sit on top of them:

- the **sparkline** needs history that survives a file settling — which is exactly what `FrameStats.evicted` throws away to keep I3 provable. Blocked on **proposed I10 (bounded history)**: the data structure is the decision, the drawing is the easy part.
- the **heat strip** needs each file's total line count, a whole-file read the frame path exists to avoid. Cacheable per `(path, blob id)`; without that cache it breaks I2a.

Split #10 along the seams of what each element actually needs, not along what they look like: history-backed (sparkline, recency gradient, `just changed` decay — one clock, not three), whole-file-backed (heat strip), and free (per-file counters, which cost nothing because a file must be diffed to be drawn).

Untracked entirely, and all visible in the mockup: the header **mode word** (`watching`, implying a mode set), the **status bar** (`0.8ms frame`, `11MB` — one measuring inside I9's budget, the other a syscall I3 only samples in soak), and the **key-hint bar**, which constrains I6 because roughly thirty columns of it has to degrade at forty.

## Phase 4 — distribution

Milestone: [Phase 4](https://github.com/breferrari/vigia/milestone/4)

| | Task | Issue |
|---|---|---|
| ⬜ | `cargo-dist`, crates.io, Homebrew tap | [#12](https://github.com/breferrari/vigia/issues/12) |

## Phase 5 — deferred findings

Milestone: [Phase 5](https://github.com/breferrari/vigia/milestone/5)

Everything on the deferral shelf below has a milestone here, so shelved work is still reachable by a milestone-filtered query rather than only readable in prose. The shelf carries the *reason*; this table carries the *state*.

| | Task | Issue |
|---|---|---|
| ⬜ | A symlink diffs as its target's contents | [#15](https://github.com/breferrari/vigia/issues/15) |
| ⬜ | The fingerprint cannot see a timestamp-preserving write | [#16](https://github.com/breferrari/vigia/issues/16) |
| ⬜ | Two paths differing outside UTF-8 collapse onto one cache key | [#17](https://github.com/breferrari/vigia/issues/17) |
| ⬜ | A frame reads a whole file to discover it is binary | [#18](https://github.com/breferrari/vigia/issues/18) |
| ⬜ | An idle frame is one `stat` per changed file | [#19](https://github.com/breferrari/vigia/issues/19) |
| ⬜ | An external kill leaves the terminal in raw mode | [#24](https://github.com/breferrari/vigia/issues/24) |
| ✅ | `take-next`: pre-flight the spec against the tracker | [#20](https://github.com/breferrari/vigia/issues/20) |

---

## Deferral shelf

Items that surfaced mid-phase and would have derailed the block they surfaced in. Deferral is a first-class outcome recorded here, not a dropped ball and not scope creep absorbed silently. Each one carries the phase it moved to.

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
| An external kill leaves the terminal in raw mode ([#24](https://github.com/breferrari/vigia/issues/24)) | I8, 2026-07-30 | Phase 5 | I8 promised "including `SIGINT`" and the shell falsified the premise: raw mode clears `ISIG` and `ENABLE_PROCESSED_INPUT`, so the interrupt key is a key event and never a signal. What is left is a signal nobody at this keyboard sent, and `std` has no way to catch one, so closing it is a dependency decision rather than an implementation detail. The single-task version is Unix-only (`signal-hook`, with `SetConsoleCtrlHandler` needed separately on Windows), which is the same asymmetric guarantee #16 already rejected as worse than one stated uniformly. `SPEC.md` I8 was narrowed to say so out loud instead of overselling |

## Pull-forward log

Items that moved into an *earlier* phase than planned. Recorded for the same reason as deferrals: movement should be visible. Over time the balance of this list against the shelf says whether the plan was too ambitious or too cautious.

| Item | Moved | Why |
|---|---|---|
| `notify` named as a dependency in `SPEC.md` §6 | Into Phase 1 | I1 requires filesystem events rather than a timer, so the choice could not wait for the shell. Cross-platform C-toolchain-free status verified on all three tier-1 targets at the same time |
| Terminal restoration implemented with the shell, ahead of I8 | Into [#9](https://github.com/breferrari/vigia/issues/9) | Not scope creep and not I8 done early. A shell that takes the alternate screen without giving it back is not shippable at any stage, and `panic = "abort"` means a `Drop` alone cannot, so the panic hook had to land with the code that takes the screen. What [#8](https://github.com/breferrari/vigia/issues/8) still owns is the whole of its proof, which is also the whole of the invariant |
| I2 split into I2a and I2b | During Phase 1 | The original I2 conflated incremental re-diffing with incremental re-highlighting. Different dependencies, different phases, and Phase 1 could not close while one number meant two things |

---

## How work is taken

One task, taken to done, before the next. See `.claude/skills/take-next/`.
