# vigia — Specification

Status: **v0, 2026-07-30.** Source of truth. Code is written against this document; see `CLAUDE.md` § Method for the drift rule.

---

## 1. Problem

Running a full git TUI in a pane beside an AI coding agent, to watch changes as they land, is the wrong tool. A multi-panel git client spends the pane on branches, commits, stashes and status. In a pane already halved against the agent, every panel is a few lines tall and names truncate.

What is wanted: **the live diff, fullscreen, auto-updating, scrollable, mouse-driven, and beautiful.** Nothing else.

## 2. Product class — the load-bearing decision

`vigia` is a **monitor**, not a reviewer. That single distinction generates every budget below, and the budgets are the product.

| | Reviewer | **Monitor (`vigia`)** |
|---|---|---|
| How it starts | Launched per review | Already open |
| Interaction | The point | Rare or never |
| Correct when untouched | No | **Required** |
| Runtime | Minutes | **Days** |
| Latency budget | Seconds | **A frame** |

`btop` is the reference for what monitor-class feels like: you read state from shape and colour, glance away, glance back, and never configure anything.

Design rationale beyond this section is recorded outside the codebase; call `recall` when a decision needs it.

## 3. Invariants

Numbered because tests reference them. Each must have a test that fails on violation. **An invariant without a failing test is a wish.**

Budgets are **absolute** and chosen to be defensible on their own terms, not relative to any other tool.

| # | Invariant | Budget | How it is proven |
|---|---|---|---|
| **I1** | Redraw is **event-driven**, never a fixed timer. No filesystem event and no git index change means no work. | **0 wakeups** while idle | CPU sampled over a 60s idle window; assert no render calls |
| **I2a** | **Re-diffing is incremental** — the frame path never re-diffs a file that did not change. | re-diff cost ∝ what changed, **not** worktree size | Assert the re-diff count and byte count for a single-line edit, across **two** fixtures differing only in changed-file count. One fixture cannot prove it: see §7 |
| **I2b** | **Re-highlighting is incremental** — only changed hunks are re-parsed. | re-parse ∝ edit size, **not** file size | Assert the re-parse count and byte count for a single-line edit, across **two** fixtures differing only in file size. One fixture cannot prove it, for the reason I2a's row gives: see §7 |
| **I3** | **Flat resources over days.** No unbounded growth in RSS, file handles, or temp files. | **RSS drift < 5%** over 24h; **zero** temp files retained | Soak test: 24h of synthetic edits, RSS sampled every 5 min |
| **I4** | **Streams, never buffers.** First paint is independent of total diff size. | **first paint < 100ms** on a 100k-line diff | `criterion`, gated in CI |
| **I5** | **Correct with zero interaction.** Auto-follows the newest change and scrolls to it, untouched. | — | Scripted edit sequence, snapshot the frame, no input given |
| **I6** | **Legible at 40 columns.** No horizontal overflow, no truncated-to-useless labels. | — | Snapshots at 40 / 80 / 120 columns, plus structural gates in `crates/vigia/tests/legibility.rs` sweeping every width from 1 to 120: no row over-occupies, no hint is cut in half, and every label that lost characters says so |
| **I7** | Startup to first paint is imperceptible. | **< 50ms** | Timed, gated in CI |
| **I8** | Terminal restored on **every exit the process controls**: the quit key (Ctrl-C included), an error return, and a panic under `panic = "abort"`. An externally delivered signal is not covered — see [#24](https://github.com/breferrari/vigia/issues/24). | — | Takeover order and its exact inverse; the partial-failure unwinding; a panic-hook test; escape sequences against DEC's own numbers |
| **I9** | Steady-state frame time holds 60fps under continuous edits. | **< 16ms** p99 | Gated over the **frame path**, not the primitives: a settled frame, one line rewritten before each frame, every file materialised. `criterion` tracks the same shape |

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

> [!note] Why I8 no longer says `SIGINT`
> It read "restored exactly on exit — including `SIGINT` and panic", and the
> `SIGINT` half encoded an assumption the shell falsified. **Raw mode removes the
> signal.** `enable_raw_mode` clears `ISIG` on Unix and `ENABLE_PROCESSED_INPUT`
> on Windows, so Ctrl-C is never translated: it arrives as an ordinary key event
> and is handled by the key map, which is why `Session` never needed a handler and
> why no test could ever have been written for the clause as worded.
>
> What that leaves genuinely uncovered is a signal nobody at this keyboard sent:
> `kill -INT` or `-TERM` from another pane, which runs neither `Drop` nor the panic
> hook. `std` has no signal API, so closing it is a **dependency decision** rather
> than an implementation detail, and the single-platform version of it
> (`signal-hook` on Unix, with `SetConsoleCtrlHandler` needed separately on
> Windows) ships a guarantee whose meaning differs by tier-1 platform. That is the
> same trade [#16](https://github.com/breferrari/vigia/issues/16) already rejected
> as worse than one stated uniformly. Tracked as
> [#24](https://github.com/breferrari/vigia/issues/24) rather than assumed away,
> and the invariant above now states its own limit instead of overselling it.

## 4. Scope

**In:** working-tree diff (unstaged by default), event-driven refresh, follow mode, scroll (keyboard + mouse wheel), syntax highlighting, per-file churn visualisation, responsive layout, theming.

**Out of v1, deliberately:** staging, committing, rebasing, branch or commit browsing, annotations, comment threading, AI features, remote operations. Each is reviewer-class and each would cost an invariant.

**Deferred, not rejected:** multi-worktree view (several agent sessions at once — the strongest differentiator after glanceability, and the most btop-shaped); Jujutsu and Sapling support.

## 5. The differentiator: glanceability

`btop`'s real achievement is that state is readable from **shape and colour** without reading text. `vigia`'s translation:

- per-file **churn sparkline** — change density over time
- a **heat strip** locating change within the file
- live **+/− counters**
- a **visual pulse** on what just changed

This is what makes it a monitor rather than a narrow diff view, and it is where design effort goes once the invariants hold.

### 5.1 What the README mockup commits to

`assets/preview.svg` is the most detailed design artifact in the project, it is **public**, and until now it was specified by a three-sentence caption. A picture in a README is a promise; these are the promises in it, and what each one costs.

Elements the four bullets above do **not** cover are marked **(unspecified)** — they are in the picture, so they are committed, and they had nowhere to be described.

| Element in the mockup | What it needs |
|---|---|
| Header: `watching · 3 files` | A **mode word**, so there is a set of modes. `watching` implies at least a settling state and an idle one. Changed-file count is the §10 header question. **(unspecified: the mode set)** |
| Per-file **sparkline** | A **retained time series per file** — samples of churn over a window, bucketed. This is the only unbounded state in the design and it is the one I3 forbids growing: the window and the sample rate are part of the invariant, not a rendering detail. **(unspecified: window, bucket width, eviction)** |
| Per-file **heat strip** | Hunk line-ranges projected onto a fixed number of buckets across the file's length, so it needs the file's **total line count**, not only its diff. Colour rule when one bucket holds both additions and deletions. **(unspecified: bucket count, mixed-bucket colour)** |
| Per-file `+42 −7` | Covered. Per-file counters are free — a file must be diffed to be drawn (§10). |
| A **dimmed row** (`Cargo.toml` in the mockup renders fainter than the rows above it) | A **recency gradient**: rows fade as their last change ages. **(unspecified entirely — and it is doing real work in the picture, since it is how the eye finds what moved without reading)** |
| `● just changed` on the diff header | The pulse, but drawn as a **persisting label with a dot**, not a flash. So it has a **decay**. **(unspecified: how long it persists, and whether it fades or cuts)** |
| Diff body: **syntax highlighted content**, five classes deep (`kw`, `fnn`, `typ`, `var`, `con`) over a default foreground | I2b, and a **class set** rather than a palette: what the picture commits to is which distinctions are worth a colour, not which colour each one gets. Ruled 2026-07-30 and implemented; the engine emits meanings and the shell colours them, which is what leaves the palette to #11. See §11.1 |
| A **tinted row** with a coloured left bar on every added and removed line | A per-row **background**. It is what separates a changed line from a context line in the picture, because the text itself is highlighted identically on both, and it is doing the same work the dimmed row above does: it is how the eye finds what moved without reading. Not drawable at sixteen foreground-only colours, where an ANSI background is a solid block rather than a tint, so it belongs with [#11](https://github.com/breferrari/vigia/issues/11). **(unspecified entirely, and the reason I2b's ruling costs something: see §11.1)** |
| Status bar `0.8ms frame` | Instrumenting the render path and drawing the result. Self-referential: measuring and painting the number costs frame time that I9 gates. **(unspecified: sampled or per-frame, and which statistic)** |
| Status bar `11MB` | A live RSS readout. I3 samples RSS in a **soak test**, never on screen; reading it per frame is a syscall on some platforms. **(unspecified entirely)** |
| Status bar `follow ▶` | A follow-state indicator, which presumes the mode exists. **Landed with I5**, on the footer rather than a third chrome line: see §11.1. I6 later gives it a line of its own, above the hints, at the widths where one line cannot hold both. |
| Key hints `q quit · f follow · ↑↓ scroll` | A hint bar, and it **constrains I6**: roughly thirty columns of it must degrade legibly at forty. **Ruled 2026-07-30: it does not degrade by shortening.** The footer takes a second line instead, and only below the width where a whole line holds the bar does it drop hints, `jk scroll` first. See §11.1. |

Two of these are corrections rather than gaps:

1. **`f` toggles follow.** The mockup shows a dedicated key and a state indicator. That is the answer to §11.2 B1, and it was published before the question was asked. Ruled and implemented 2026-07-30; `f` is in `input.rs` and the rule is §11.1.
2. **The dimmed row and the `just changed` label are one mechanism**, not two: both are recency rendered as intensity. Specifying them separately would produce two decay clocks that disagree on screen.

**A picture in a public README is a specification whether or not it is written down.** This one implied a retained time series, a recency gradient, two status readouts and a keybinding, none of which appeared in the spec, the roadmap, or any issue — while [#10](https://github.com/breferrari/vigia/issues/10) carried four of them in a single line. That is the same failure as §11: behaviour that exists somewhere real, with no line claiming it.

### 5.2 Where the mockup pulls against the invariants

§5 says design effort goes here "once the invariants hold", and [#10](https://github.com/breferrari/vigia/issues/10) repeats it: *"depends on the invariants holding first."* **That framing is wrong, and correcting it is the most consequential thing in this section.** At least two elements need retained state the frame path does not produce, in `vigia-core` rather than in the shell. They do not sit on top of the invariants. They **move** them.

**The sparkline needs precisely what eviction throws away.** `FrameStats.evicted` exists so the cached-diff map stays "bounded by the current diff rather than by everything ever edited" — that is how I3 is argued today. A churn sparkline is change density *over time*, so it has to survive a file settling: one that empties the moment a file stops changing shows nothing worth glancing at, and *"what was hot thirty seconds ago"* is the entire question it answers. Glanceability history therefore cannot live in the evicting map — and must not be unbounded either.

> [!warning] Proposed I10 — bounded history
> Deliberately **not** in the §3 table, because that table is for invariants with
> a failing test and this has none. By this document's own rule it is a wish, and
> writing it into the table would make the table lie.
>
> *Glanceability history is bounded by a fixed time window and a fixed cap on
> tracked paths, independent of how many files have changed in the session.* A
> bulk operation touching ten thousand files must not grow it past the cap, and a
> path that ages out of the window is dropped entirely.
>
> It needs a budget and a soak assertion before it earns a row. Until then every
> sparkline task is blocked on it, because **the data structure is the decision**
> and the drawing is the easy part.

**The heat strip needs a whole-file property.** Locating change within a file requires that file's **total line count**, and the frame path is built to avoid exactly that: pure revalidation reads **0 bytes** (§10), the number I2a is written against. Measured naively — every changed file, every frame — it reintroduces the read I2a removed. It is cacheable per `(path, blob id)`, since a file's length cannot change without its content changing, so it is payable once per version rather than once per frame. **That caching is not an optimisation; without it the heat strip breaks I2a.**

**Two status readouts measure the thing they run inside.** `0.8ms frame` means instrumenting the render path and drawing the result — a readout whose own cost falls inside the budget it reports, gated by I9 at 16ms p99. `11MB` is a live RSS number, and I3 samples RSS in a **soak test** precisely because reading it is a syscall on some platforms rather than free per frame. Both are honest to show; neither is free to show; the spec said nothing about either.

**Consequence for sequencing:** Phase 3 is not a rendering phase. Its two headline elements each need a core-side change with an invariant attached, so they belong in the same conversation as I2a and I3 rather than strictly after them.

## 6. Architecture

Cargo workspace, two crates:

- **`vigia-core`** — library. Git (`gix`), diff modelling, incremental highlighting (`syntect`), filesystem events (`notify`), the watch and coalesce engine, the frame path. **No terminal I/O, no ratatui.** Every invariant except I6 and I8 is testable here, headlessly.
- **`vigia`** — the `ratatui` + `crossterm` shell: input, layout, theming. Thin by design, so the TUI stays swappable and the engine stays provable. A library with a five-line binary on top rather than a binary alone, because §7 makes the snapshot suite the proof for I5 and I6 and a test cannot import a `main.rs`.

The split is a dependency decision, not a hedge: the TUI renders whatever the core produces, so the core has to work first or the TUI is being built on sand.

`notify` is named here because I1 requires filesystem events rather than a timer, and each platform delivers them differently: `inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows. `notify` is the standard Rust abstraction over those three, and it keeps the binary pure Rust. Verified 2026-07-30 against `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-musl` and `aarch64-apple-darwin`: no `cc`, `cmake` or `bindgen` in any of the three graphs. The `-sys` crates it pulls (`inotify-sys`, `fsevent-sys`, `windows-sys`) are FFI declarations against facilities the OS already ships, so they compile no C.

**Coalescing stays ours.** `notify` has a companion debouncer crate, and taking it would move coalesce policy out of `vigia-core`, which is the one place I1 is testable. `notify` supplies raw events and nothing else.

A coalesced tick also **names the file whose write landed last in it**, which is what I5 follows and what §11.1 specifies. The path is a by-product of the gitignore filter, which already resolves every event against the worktree root, so carrying it costs no syscall. This is the second thing the debouncer crate would have taken away.

**The frame path keeps its diffs between frames.** I2a forbids re-diffing a file that did not change, so the core holds the previous frame's diff per path and revalidates instead of recomputing. Validity is decided from three things that cost no file read: the index blob the change names, the kind of change, and a `stat` of the working-tree file. Content is never hashed to decide this, because hashing is the read I2a exists to avoid.

A `stat` on its own is not proof, and the gap is the one git calls *racily clean*: two writes of the same length inside a single modification-time granule are indistinguishable by `stat`.

The rule that closes it has to account for **flooring**. A filesystem stamps a modification time by truncating the current time to its own granularity, so a write landing *after* a read can record a time *before* it. "Older than the read" therefore proves nothing, because the granule may still be open: measured on NTFS, that mistake serves a stale diff on about one same-length rewrite in four hundred, and on a 1s-granule volume it would be most of them. So a fingerprint is trusted only once a **full granule has passed** between the stamp and the read, and anything else is re-diffed. The margin is a constant that must be an upper bound on real granularity: **2 seconds**, since FAT and exFAT quantise to 2s and HFS+ to 1s, where NTFS, ext4 and APFS are milliseconds or finer. That trades redundant diffs of files written in the last two seconds, which are files that just changed, for never showing a stale one.

**What it still cannot see** is a writer that restores a modification time it did not advance: `cp -p`, `rsync -t`, `unzip`, `touch -r`. Git carries the inode change time to catch those, and `std` exposes no equivalent on Windows, so closing it is a dependency decision rather than an implementation detail. Tracked on the deferral shelf rather than assumed away.

**And it assumes the modification time is stamped by the local clock.** The margin compares a filesystem timestamp against this process's clock, so on a network filesystem the server's skew subtracts directly from it: a server two seconds behind consumes the whole margin and the flooring problem returns. Local worktrees, which is what a monitor beside an agent watches, are unaffected.

## 7. Testing

- **Snapshot tests over `ratatui::backend::TestBackend` with `insta`** — render frames into an in-memory buffer, snapshot as text. This is what makes I5 and I6 assertable at all: the UI becomes diffable text.
- **`criterion`** for I4, I7 and I9, **tracking rather than gating.** Criterion compares a run against a saved baseline, and the budgets in §3 are absolute, so it is the right instrument for "this got 20% slower" and the wrong one for a pass/fail line. Compiled in CI so the benchmarks cannot rot; not timed there, because a shared runner cannot produce a number worth comparing.
- **Budget gates** in `crates/vigia-core/tests/budgets.rs`, in **two tiers**, because an absolute wall-clock threshold is a strong instrument on a known machine and a weak one on a hosted runner:
  - *Structural* gates compare the engine against itself across fixtures that differ only in how much changed. They are ratios and exact byte counts, so they are hardware-independent, take **no slack**, and are what actually catches a regression. Making the frame path re-read every changed file is a 5x wall-clock change that a generous threshold waves through, and a 100x byte-count change that these cannot.
  - *Absolute* gates hold the wall clock to §3. Release only, since the budgets were set against optimised code, and with a slack multiplier (`VIGIA_BUDGET_SLACK`, default 1) so a shared runner's variance does not read as a code regression.
- **An invariant the engine can only make possible gets a second structural gate over the caller**, in `crates/vigia/tests/`. I4 is the case: the core fetches content per file, so painting the top of a large diff without reading the bottom is *available*, and nothing in the core stops a renderer from asking for every file anyway. Asking is the natural way to write one. So what one screenful costs is gated where the screen is, against two fixtures differing only in changed-file count, for the reason the tier above gives.
- **An invariant whose two failure modes are not symmetrical gets a gate for each.** I2a is the case that made this a rule. Reusing too *little* is slow and loud, and the budget gate catches it. Reusing too *much* is fast, passes every budget, and shows a diff that no longer exists, so `crates/vigia-core/tests/frame.rs` compares every reused frame against one computed with no memory at all. A budget gate alone would have called the second failure a success.
- **A rule covering a window too small to race is extracted as a pure function and tested directly.** The racily-clean guard in §6 cannot be reached on demand from a filesystem: dropping it leaves the whole integration suite green and only a unit test over the decision function goes red. Where a correctness rule has no reachable integration path, the pure-function test *is* the gate, and that is worth stating rather than discovering.
- **Gates are mutation tested before they are trusted.** Break the code deliberately, confirm the gate goes red, restore. Two of the flaws found this way were invisible to reading: a structural gate comparing one call against the sum of its own calls, and a p99 over too few cold samples.
- **Steady-state budgets are sampled after a warmup**, and over enough frames for a percentile to be one. I9 is a claim about steady state, so the cold path is outside its scope by definition; measured cold frames run ~40ms against a warm p99 of ~3ms, and at 30 samples a nearest-rank p99 is just the maximum.
- **A CI guard fails the build if `cc`, `cmake` or `bindgen` enters the dependency graph** on any tier-1 target, plus a musl build asserting the binary links no shared libraries. The pure-Rust constraint is what makes musl-static and Windows cheap, so it is enforced rather than trusted.
- **Soak test** for I3, scheduled rather than per-commit.
- **`proptest`** over diff parsing and hunk-boundary logic.

## 8. Phases

Live status, issue-linked, is in [`ROADMAP.md`](ROADMAP.md). This section is the shape; that file is the state. Work is taken one task at a time via `.claude/skills/take-next/`.

**Phase 1 — core engine.** `vigia-core` plus a `main` that prints frame timings. Prove `gix` gives working-tree-vs-index diffs at the fidelity and speed needed — it is the least-precedented dependency in the stack and everything sits on it. Land **I1, I2a, I4, I7, I9**, each gated. No TUI.

**Phase 2 — minimum monitor.** ratatui + crossterm shell. Follow mode (I5), scroll, mouse, exit safety (I8), 40-column layout (I6). Plus **I2b** (needs `syntect`) and **I3** (soak). Snapshot suite.

**Phase 3 — glanceability.** Section 5, plus theming.

**Phase 4 — distribution.** `cargo-dist`, crates.io publish, personal Homebrew tap, prebuilt binaries on GitHub releases.

**Phase 5 — deferred items**, only if daily use asks for them.

## 9. Distribution

- **crates.io** — `cargo install vigia`. A name **cannot be reserved; it must be published**, and a publish is **permanent** (`cargo yank` hides a version from new dependents, it does not delete it, and the name stays taken). So the first publish should be a crate that does the minimal real thing. Verified free 2026-07-30.
- **Homebrew** — `homebrew-core` **cannot be reserved** and requires notability: ≥30 forks **or** ≥30 watchers **or** ≥75 stars, plus a stable versioned release. Until then a **personal tap** (`breferrari/homebrew-tap`) gives `brew install breferrari/tap/vigia` immediately, is fully under our control, and is what `cargo-dist` generates.
- **GitHub releases** — prebuilt binaries per platform via `cargo-dist`.
- No domain. Not needed for a CLI.

## 10. Open questions

- [x] ~~Does `gix` cover working-tree-vs-index diff at the fidelity and speed needed?~~ **Answered 2026-07-30: yes.** Hunk boundaries match `git diff -U3` exactly, with git used as the oracle at every edit gap from 0 to 10 lines. On a 100k-line diff, release build: first change available in 3.84ms against the 100ms I4 budget, single-file re-diff 3.27ms p99 against the 16ms I9 budget, process start to first paint 20.37ms against the 50ms I7 budget. The dependency that could have forced a rethink did not.
- [ ] Rename tracking cannot stream. Pairing a deletion with an addition needs the whole walk, so with it on the first change arrives at 97% of the walk and time-to-first equals time-to-last. It is on by default anyway, because reporting a move as an unrelated delete plus add misdescribes what the agent did. Confirm against a week of real use, or paint without renames and reconcile them on a later frame.
- [x] ~~Re-diffing every changed file costs 18.58ms p99 on a 100k-line diff, over the I9 budget, against 3.27ms for a single file.~~ **Closed 2026-07-30:** I2a is enforced. The frame path revalidates from the index blob, the change kind and a `stat`, so a single-line edit in a 100-file worktree recomputes exactly one diff and reads exactly that file. Measured over the same 100-file, 100k-line fixture, release build: a **real frame under continuous edits is 6.97ms p99** against the 16ms I9 budget, revalidating 99 files and recomputing the one that moved. Pure revalidation with nothing edited is 3.93ms and reads **0 bytes**. A cold frame with nothing to reuse is 18.28ms and reads 3.6 MiB, which agrees with the 18.58ms the spike measured over the primitives and is the cost I2a removes. Every number in this bullet is a frame-path measurement; the 18.58ms above is the spike's, over `Worktree::diff` called per file.
- [ ] The settle margin is a fixed 2 seconds, sized for the coarsest filesystem anyone might use rather than the one in front of us, so on NTFS it is over a hundred times more conservative than it needs to be and on APFS far more. **Measured cost, and it is not only redundant work:** after a bulk rewrite of all 100 files in the 100k-line fixture, every frame recomputes every diff for the whole margin, 18 to 21ms per frame, putting 82 of 620 consecutive frames over the 16ms I9 budget for about two seconds. A formatter, a branch switch or a multi-file agent edit all produce that shape. Single-file editing is unaffected: one write costs 503 redundant diffs over 2.004s and never approaches the budget, and autosave every 500ms stayed under it on every one of 973 frames. It can be narrowed per worktree with no extra I/O, which removes the breach rather than tolerating it: the smallest positive difference between the modification times status already reports is an upper bound on that filesystem's granularity, which on NTFS would take the margin from 2s to about 16ms. Do this before the soak test, since I3 will see the redundant work too.
- [ ] The frame path walks status to completion before it reports a file list, so it does not stream the way the raw change iterator does. Two reasons it costs nothing today: rename tracking cannot stream either and is on by default, and a scrollbar needs the file count regardless of how few files are drawn. What is open is whether both hold at ten thousand changed files, where the walk itself could exceed I4. Revisit together with rename tracking above, since they stand or fall together.
- [ ] The header counts changed files and not changed lines. A repository-wide `+`/`-` total needs every file's diff, and I4 makes first paint independent of total diff size, so the two cannot both hold on the first frame. §5's counters are per-file and cost nothing extra, since a file has to be diffed to be drawn; only the total is affected. What is open is whether it is worth computing behind the frame and revealing when it arrives, which belongs with the rest of §5 in Phase 3.
- [x] ~~Is `syntect` fast enough incrementally to hold I2b, or does it force tree-sitter — and with it a C toolchain — back in?~~ **Answered 2026-07-30: it holds, and tree-sitter stays out.** Release build, with `regex-fancy`; `syntect`'s *default* engine is `regex-onig`, which is oniguruma, so the defaults would have put `cc` in the graph on their own. Loading the bundled grammars is **318µs** against I7's 50ms, so nothing has to be deferred to first use. One screenful of Rust is **1.53ms** against the 16ms I9 budget, and a real shell frame under continuous edits over the 100-file, 100k-line fixture is **10.52ms p99** (p50 7.95ms, max 11.62ms), against **6.97ms** for the core frame path alone before any of this existed. What is not affordable is parsing a hunk *whole*: the 1006-line hunk that fixture produces costs **60.97ms**, which is 3.8x over budget and would be paid on every frame under I9's own shape. So parsing forward only as far as the screen has asked is load bearing rather than an optimisation, which is the same shape I2a found for re-diffing. Revalidation is a hash of the hunk rather than a counter, because inside the settle margin the frame path re-diffs an untouched file every frame and a counter would re-highlight files nobody edited. The cost that stays: the bundled grammars take the release binary from **3.20 MiB to 5.04 MiB**.
- [ ] Highlighting a hunk is forward-only, because `syntect` parses a line from the state the line before it left. The first frame that draws *deep* inside a large hunk therefore pays for everything above it in that hunk: landing on the last row of the 1006-line hunk above costs the whole **60.97ms** parse, once, before the cache makes it free. Scrolling never sees it, because a screenful at a time is a screenful of parsing. `G`, a follow jump into a large file, and scrolling **up** into the bottom of the previous file all land there directly. §7 puts the cold path outside I9 by definition and this is a first-touch cost rather than a breach, but it is a real hitch on one keypress and is recorded rather than assumed away. The fix, if daily use asks for one, is to bound the parse per frame and leave the tail unclassified until the next — which needs a redraw the event loop has no reason to schedule today, and I1 forbids inventing a timer to get one.
- [ ] Default view: unstaged only, or working-tree-vs-HEAD? Unstaged is the thesis; confirm against a week of real use.
- [ ] Windows: supported target or best-effort? Truecolor needs Win10+, and legacy conhost degrades.

## 11. Behaviour

§3 says how well `vigia` does things. Every number in it is defensible and every one has a test. Nothing above says **what happens when you press a key, or when nothing has changed** — and that gap is why the product reads as a screenshot with budgets attached.

Two parts: what the shell already does, recorded because it was decided in code first, and what is still undecided.

> [!warning] Behaviour decided in code without a line here is a defect
> The README says this file is the source of truth, written before the code. That
> was **not true** for the whole interaction surface on 2026-07-30: the keymap and
> the treatment of untracked files were both settled in implementation and appear
> nowhere above. §11.1 repays that, and the rule it leaves behind is the same
> shape as §3's: **an invariant without a failing test is a wish, and a behaviour
> without a spec line is an accident.** Neither is caught by `take-next`'s
> pre-flight, which compares invariant tokens rather than behaviour.

### 11.1 What the shell does today

Most of this was back-filled from the implementation on 2026-07-30 rather than newly decided. **Follow mode is the exception**: it was ruled on 2026-07-30, as B1 and B2 below, and written here *before* the code existed — which is the order the README claims for everything and which §11's warning exists because this file did not keep. Where this disagrees with the code, the code is the bug.

**What the diff contains.** Working tree against the index. **Untracked files are included** and diff as all-additions — load-bearing rather than incidental, since creating new files is among the most common things an agent does, and a tool blind to them would miss its own use case. Rename tracking is **on by default**: showing a move as an unrelated delete plus add misdescribes what the agent did. Its streaming cost is the open question in §10, not a settled trade.

**Keys.**

| Key | Action |
|---|---|
| `q`, `Esc`, `Ctrl-C`, `Ctrl-D` | quit |
| `j`, `↓` | scroll down one row |
| `k`, `↑` | scroll up one row |
| `Space`, `PgDn` | page down |
| `PgUp` | page up |
| `g`, `Home` | first changed file |
| `G`, `End` | last changed file |
| `f` | engage follow mode, or disengage it |
| mouse wheel | scroll |
| terminal resize | redraw, no state change |

**Scroll position is `(file, offset within that file)`, never a row index.** A frame that changes something above the viewport therefore does not teleport the view. This is a correctness property, not an implementation detail: with a row index, an agent writing to a file earlier in the list would yank the reader's position on every keystroke it makes.

**Follow mode**, which is I5. `less +F` semantics, and the toggle the README mockup already published: follow is **on at startup**, **any manual scroll disengages it**, and **`f` re-engages it and jumps straight to the newest change** rather than waiting for the next one. The footer shows `follow ▶` while it is engaged.

Two boundaries are load bearing, because each is a way for the mode to be quietly wrong rather than visibly broken. A **terminal resize does not disengage**: it moves no viewport and expresses no intent, and a monitor beside an agent is resized constantly. And **`G` disengages rather than re-engaging**, because "jump to the last changed file" and "resume following" are different intents that would otherwise be the same key, leaving a reader unable to look at the newest file without also re-arming the view.

**The newest change is the file whose write landed last in the settled batch**, and the batch is the coalesced tick. A monitor cannot find that file by looking: `stat`-ing every changed file is the cost [#19](https://github.com/breferrari/vigia/issues/19) already records as breaching I9 at scale, and I4 forbids reading files the frame does not draw. It does not have to look, because the filesystem event already carries the path. `Tick` reports it, so following costs no read, no `stat` and no diff. When the named path is not in the diff — an index write, or an edit reverted before the tick landed — the view stays where it is, because there is no newest *change* to follow.

**How the chrome fits**, which is I6. One rule, and the layout follows from it: **a thing made of items breaks, a thing made of characters marks its edge, and content is neither.**

The **hint bar is a list**, so when the footer cannot hold both halves on one line it takes a **second line** rather than shortening anything. The state moves to the upper of the two and the hints keep the bottom row, so narrowing a pane never moves the hints out from under a reader's eye. It grows only while at least two body rows survive — a monitor with no diff left in it has stopped being one — and only when there is a state worth moving. A notice, which replaces the hints, inherits that whole line when there is one, but **never causes it**: a notice is transient, and a footer that grew for one would jog the reader's diff down a row and back every time a file vanished between being named and being read. The height is a function of width, follow state and changed-file count, all of which change only when the diff does.

Below the width where even a full line holds the bar, it drops **whole hints** and never part of one: `jk scroll` first, then `q quit`, leaving `f follow` last. `q` and `jk` are pager reflexes and four keys reach quit, while `f` is the one nobody would guess and the only one that restores a state a reader can lose without noticing. The state has its own ladder, `follow ▶  N/M` then `follow ▶` alone, because the header already carries the file count. **State outlives advice at every width**, which is what keeps the mode visible when the pane is at its worst.

The **header never grows**. A worktree name is not a list and has nowhere to break, so it marks its edge like every other single token.

And a token that had to lose characters says so, in the direction it lost them. `…` on the **left** means the beginning is gone, and only a file path uses it, because the end of a path is what names the file. `›` on the **right** means it continues past the edge: the worktree name, a notice, a hunk header, a note, the empty-state line. A hunk header silently cut to `@@ -258,7 +25` reads as a different line number, which is the failure this closes.

**A clipped diff line is marked too, and is not a truncated label.** Content cannot wrap, because a wrapped line moves every line below it and the shape of the screen stops meaning anything. Nor can it elide, because unlike a label no part of it is the identifying part. So it is clipped and marked `›`, and I6's "truncated-to-useless labels" is read as being about labels rather than about content. That is a ruling, not an omission: the alternative was a horizontal pan, which is a key and a mode this spec does not name.

**How a diff line is coloured**, which is what I2b pays for. Content is syntax highlighted, and the mockup had already ruled how: added, removed and context lines all carry the same classes, the sigil is green or red, and the line number stays faint. **Ruled 2026-07-30: follow the picture literally**, per §5.1's rule that a published artifact answering an open question is the answer. Only content is highlighted. A file heading, a hunk header and a note are chrome, and chrome that changed colour by what it happened to name would stop being readable as chrome.

Three things follow, and each is a constraint rather than a preference.

**The engine emits meanings, not colours.** `vigia-core` maps a syntax scope onto one of nine classes and stops. Which colour a class gets is the shell's, and therefore [#11](https://github.com/breferrari/vigia/issues/11)'s. A core that emitted truecolour would have settled the palette question in the one place §6 says has no terminal in it, and it would have pre-empted the 256-colour degradation path before anyone wrote it.

**The diff signal degrades to the sigil column, and that is a loss rather than a simplification.** What separates an added line from a context line in the mockup is the row tint and the left bar of §5.1, and sixteen foreground-only colours can draw neither. So until #11 lands a background, `+` and `−` carry it alone. Recorded out loud because §5 makes shape and colour the whole differentiator, and this is the one place where following the picture spends some of it. The alternative considered and rejected was to keep unclassified text green on an added line and red on a removed one: it preserves the wash, it contradicts the picture, and it would have made the shell's colours mean two different things at once.

**A file type nothing recognises is not an error.** The syntax is resolved from the path, by extension and then by whole file name, and anything unresolved draws exactly as it did before there was highlighting at all. A monitor that refused a file because it could not colour it would have inverted its own job.

**CLI.** One optional positional path, defaulting to the working directory. No flags today.

### 11.2 Undecided — these gate Phase 2

Each carries a recommendation marked **(proposed)**. None is settled until ruled on, and none may contradict §3 — if one does, §3 wins and the recommendation is wrong. A ruled item moves to §11.1 and leaves its number behind here, because the numbers are cited elsewhere and renumbering would silently repoint those citations.

**B1 — What happens to follow mode when the reader scrolls. Ruled 2026-07-30: the proposal stands. See §11.1.** `less +F` semantics, on at startup, disengaged by any manual scroll, re-engaged by `f`. Rationale, kept because it is the part a later reader will want to argue with: disengage-on-scroll is the only rule that never fights a reader mid-read, and a dedicated toggle beats overloading `G`/`End`, because "jump to the last file" and "resume following" are different intents that would otherwise be the same key.

*An earlier draft of this bullet proposed `G`/`End` as the re-engage key. That contradicted the mockup, which is public and predates the question. When a published artifact already answers an open question, it is the answer — the question is only whether to keep it. It was kept.*

**B2 — Which file wins when several change at once. Ruled 2026-07-30: the proposal stands. See §11.1.** Follow the file whose write landed **last** in the settled batch, and let §5's visual pulse carry the others. Rationale: it reads "newest" literally, it is stable rather than heuristic, and the pulse already exists to say "these moved too" without moving the viewport for each.

What ruling it exposed, and what §11.1 now records: "last in the batch" is only affordable because the filesystem event names the path. Deriving it instead would mean `stat`-ing every changed file, which is [#19](https://github.com/breferrari/vigia/issues/19)'s breach, so the cheap answer and the correct one happened to coincide here rather than by design.

**B3 — The empty state.** Zero changes is not an edge case; it is the state the tool sits in most of the time, and it is the **first** thing anyone sees when they open it beside an agent that has not written yet. A blank pane is indistinguishable from a hang.

*(proposed)* Name it: repository, branch, "no changes", and an explicit statement that it is watching. This screen is the product's first impression and currently has no specification at all.

**B4 — Is the file list navigable?** The README mockup shows a file list above the diff. Selectable, with the diff jumping to the selection, or a map rather than a menu?

*(proposed)* **Not navigable in v1** — one continuous scroll, list as map. Rationale: selection implies focus, focus implies a second mode, and modes are reviewer-class (§2). The pane is 40 columns beside an agent, not a full-screen client.

**B5 — Not a git repository, and submodules.** Neither appears anywhere in this spec.

*(proposed)* Not a repository: exit non-zero with one line, **before** entering the alternate screen — an error painted inside a TUI that then restores the terminal is an error nobody reads. Submodules: out of v1, shown as an opaque directory and said so, because recursing into them costs the incremental guarantees in I2a.

**B6 — CLI surface and configuration.** No flags exist; §5 theming arrives in Phase 3 with nowhere to configure it.

*(proposed)* Hold the CLI at one positional path plus `--version` / `--help` through v1, and add flags only when something asks for one. Configuration lands **with** theming in Phase 3, not before: a config file with one thing in it invites a second thing, and every option is a behaviour that needs a line in this section.
