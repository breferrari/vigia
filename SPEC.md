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
| **I3** | **Flat resources over days.** No unbounded growth in RSS, file handles, or temp files. | **RSS drift < 5%** over 24h; **zero** temp files retained | Soak: synthetic edits driven through the whole pipeline, RSS sampled at a fixed count across the window. Scheduled rather than per-commit, and the scheduled window is shorter than 24h for a reason that is not ours: see the note below and §7 |
| **I4** | **Streams, never buffers.** A frame **builds** only what it draws: diffs, highlighting and reads of file *content* follow the window, never the worktree. The one exception is the diff's **height**, which is counted for every changed file once per tick, without materialising any of it. | **first paint < 100ms** on a 100k-line diff; **counting the height < 16ms** on the same fixture | `criterion`, gated in CI; the counting bound in `crates/vigia/tests/reads.rs`, and `what_a_row_exact_scrollbar_would_cost` records what it was admitted on |
| **I5** | **Correct with zero interaction.** Auto-follows the newest change and scrolls to it, untouched. | — | Scripted edit sequence, snapshot the frame, no input given |
| **I6** | **Legible at 40 columns.** No horizontal overflow, no truncated-to-useless labels. | — | Snapshots at 40 / 80 / 120 columns, plus structural gates in `crates/vigia/tests/legibility.rs` sweeping every width from 1 to 120: no row over-occupies, no hint is cut in half, and every label that lost characters says so |
| **I7** | Startup to first paint is imperceptible. | **< 50ms** | Timed, gated in CI |
| **I8** | Terminal restored on **every exit the process controls**: the quit key (Ctrl-C included), an error return, and a panic under `panic = "abort"`. An externally delivered signal is not covered — see [#24](https://github.com/breferrari/vigia/issues/24). | — | Takeover order and its exact inverse; the partial-failure unwinding; a panic-hook test; escape sequences against DEC's own numbers |
| **I9** | Steady-state frame time holds 60fps under continuous edits. | **< 16ms** p99 | Gated over the **frame path**, not the primitives: a settled frame, one line rewritten before each frame, every file materialised. The caller-side gate **paints as well as collects**, for the reason §7 gives. `criterion` tracks the same shape |
| **I10** | **Glanceability history is bounded.** Churn history is bounded by a fixed window and a fixed cap on tracked paths, independent of how many files the session changed. A path that ages out of the window is dropped entirely. | **≤ 256 paths**, **≤ 120s**, whatever the session changed | Structural, in `crates/vigia-core/tests/history.rs`: a fixture recording **10,000** distinct paths asserts the store sits *at* the cap and that eviction actually ran, and a window of silence empties it. The soak asserts the same over the real process, and **refuses to assert** in a window that reached neither rule. Not in `tests/budgets.rs`: every gate there is a ratio or a duration, and this is a count |

A regression past any budget **fails the build.**

> [!NOTE]
> **I4 was narrowed on 2026-08-01, and the measurement is why**
>
> It read *"first paint is independent of total diff size"*, full stop, and
> [#49](https://github.com/breferrari/vigia/issues/49) had already refused a
> repository-wide `+`/`-` total on the strength of it. That ruling stands for a
> **sum over content**. It was then applied a second time, to the diff's
> **height**, and that was wrong: the two are not the same quantity, and the
> difference is measurable.
>
> A height is hunk boundaries and line counts. A [`FileDiff`] is those *plus* an
> owned `String` for every drawn line, so totalling a worktree through one
> allocates once per changed line and once per line of context. Measured on the
> reference machine, release, over a hundred files of five hundred rewritten
> lines: totalling through full diffs is **442.71ms**, and counting the same
> answer is **8.76ms**. `git diff --numstat` over the identical shape is 46ms, so
> the counting path is not merely cheaper than our own mistake, it is in the
> range the tool everyone compares against occupies.
>
> What the narrowing costs, stated rather than buried: a tick now reads every
> changed file's bytes, where before it read only the window's. It is **once per
> tick and not once per frame** — the count is cached until the next
> `Frame::advance`, so scrolling pays nothing and a redraw still reads zero.
> Diffs, highlighting and every allocation still follow the window, which is the
> half of I4 that was doing the real work.
>
> Why it was worth it: a scrollbar that cannot say where the end is says nothing.
> The version that avoided this walk had to approximate the whole from the
> current file's height, and it vanished on a short file, ballooned on a long
> one, and never reached the bottom. Reported from use rather than caught by a
> gate, which is the fourth time. `what_a_row_exact_scrollbar_would_cost` is the
> diagnostic that holds the numbers above, so the next person re-runs them
> instead of re-arguing this.

> [!NOTE]
> **Why I2 is two numbers**
>
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

> [!NOTE]
> **Why I8 no longer says `SIGINT`**
>
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

> [!NOTE]
> **Why the scheduled soak is not twenty-four hours long**
>
> The budget is a claim about a day and it stays one. What changed is the proof
> column, because the number in it was unrunnable: a **GitHub-hosted job is
> terminated at six hours** of execution time, where a self-hosted one gets five
> days. Verified against GitHub's published limits, 2026-07-31.
>
> So the scheduled run takes the longest window that fits under the cap, and the
> full 24h is reached by `workflow_dispatch`, which carries the duration, on a
> machine with no cap. The shape of the measurement does not change with the
> window: the sample **count** is fixed, so the cadence is exactly the five
> minutes above at 24h and proportionally tighter below it, and the statistic is
> computed identically either way.
>
> What does not scale down is the warmup. Every process climbs to an allocator
> plateau before it is flat, so a window short enough to be all warmup can only
> measure warmup, and the gate refuses to assert there rather than reporting a
> number it cannot stand behind. §7 carries that as a rule.

## 4. Scope

**In:** working-tree diff (unstaged by default), event-driven refresh, follow mode, scroll (keyboard + mouse wheel), syntax highlighting, per-file churn visualisation, a pinned file list above the diff (§11.1), responsive layout, theming.

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
| Header: `vigia · 3 changed` … `watching` | A **mode word**, so there is a set of modes. The changed-**line** total is the §10 header question, and the file count drawn here is not it: a per-file count is free where a repository-wide one is not. **Ruled 2026-07-31: there is no line total** ([#49](https://github.com/breferrari/vigia/issues/49)), so this row is now fully specified rather than partly and the header's facts are exactly three. A repository-wide `+`/`-` is a reviewer's summary of a changeset rather than a monitor's account of what is happening, it puts first paint back in proportion to the size of the diff against I4, and the only way round that is the wake I1 forbids. §10 carries the argument. **Ruled 2026-07-31 and implemented: the set is two**, `watching` and `not watching`. This cell used to say that `watching` "implies at least a settling state and an idle one", and it implies neither: both are durations, and a duration cannot be drawn honestly by a shell that only wakes when a file changes. That is the same wall the pulse hit one row below, and the two words left are the only two states the shell can actually tell apart. The header also draws the **worktree** name on the left where the picture draws `vigia`, which is the first of two deliberate departures from the mockup, the second being §5.1's element split. See §11.1 for both. **Reopened 2026-08-01 on the phrasing rather than the facts** ([#67](https://github.com/breferrari/vigia/issues/67)): the three facts are right and `watching · 3 files` reads as *"watching 3 files"*, a verb with an object, naming a curated set that does not exist and that B6's no-flags ruling puts out of scope. The ladder is the proof they were meant to be independent — the count drops first and the word survives alone — and at the full rung they fuse anyway. The count is also not about the watched thing: this watches the whole worktree, and `3` is what changed inside it. **Ruled 2026-08-02 and implemented: the count moves to the left, with the worktree, and reads `3 changed`; the mode word takes the right alone.** The fix is *adjacency* rather than vocabulary, which is what makes it a layout ruling and not a rewording: §11.1 already lays the footer out by subject, the header has the same three subjects available, and it was seating a tree-fact next to a self-fact. Separated, the count has nothing beside it that a participle could govern. **`changed` rather than `files` is the load-bearing half**, because `vigia · 3 files` on the left would be a *worse* claim than the one being replaced: this repository has more than three files in it, so the count has to name the change rather than the things. Two cheaper alternatives were rejected and are recorded so the ruling is a choice: `watching · 3 files changed` stops parsing as a sentence, since a clause cannot be a participle's object, and costs eight columns on a row this section forbids from taking a second line; `live · 3 files` stops parsing for the same reason and spends the `watching` / `not watching` negation pair ruled one sentence above, which exists so that a reader who has learned one word has learned both. See §11.1 for the ladder, which keeps its order and changes only which side each rung is dropped from |
| Per-file **sparkline** | A **retained time series per file** — samples of churn over a window, bucketed. This is the only unbounded state in the design and it is the one I3 forbids growing: the window and the sample rate are part of the invariant, not a rendering detail. **Ruled 2026-07-31 and implemented: a 120-second window in 8 buckets of 15 seconds, capped at 256 paths, evicted by window and by least-recently-changed.** That is I10, which now has a row above rather than a warning below. One sample per path per coalesced tick, and heights are scaled against the **busiest bucket on screen** rather than per row, because the question a reader asks down a file list is which file is busiest |
| Per-file **heat strip** | Hunk line-ranges projected onto a fixed number of buckets across the file's length, so it needs the file's **total line count**, not only its diff. Colour rule when one bucket holds both additions and deletions. **Ruled 2026-07-31 and implemented.** Bucket count is **12**, from the picture, which draws exactly twelve; the picture also draws an empty bucket as a **dark track** rather than as a gap, so the strip is always its full width and a reader can see how much of the file is untouched. A mixed bucket is **yellow**: every alternative paints it as pure, and separating addition from removal by position is the strip's whole job. **Intensity is three steps, matching the picture** ([#11](https://github.com/breferrari/vigia/issues/11)). This cell said two for a phase, because sixteen foreground-only colours hold a normal and a bright of each hue and no third stop, so the ramp was as wide as the palette could draw rather than as wide as the picture asked for. A wider palette closed it. At sixteen colours it is still two, and `ansi` spells that out in its own fields rather than leaving the depth ladder to collapse them by accident. The line count it needs is **free**: see §5.2 |
| Per-file `+42 −7` | Covered. Per-file counters are free — a file must be diffed to be drawn (§10). |
| A **dimmed row** (`Cargo.toml` in the mockup renders fainter than the rows above it) | A **recency gradient**: rows fade as their last change ages. **Ruled 2026-07-31 and implemented as three rungs of one ladder** — pulse, live, cold — read from I10's store; see §11.1. Sixteen foreground-only colours have three intensities to spend, so the gradient is a ramp of three rather than a fade. **Corrected 2026-07-31: the fourth rung is not [#11](https://github.com/breferrari/vigia/issues/11)'s to bring**, and this cell used to say it was. The rung count belongs to `Recency` in `vigia-core`, which has exactly three variants and whose `cold` means *untracked* rather than *old*, so a wider palette draws the same three rungs in better colours and cannot draw a fourth. A real fade needs the store to emit an age fraction, which is a core change with an I10 budget on it. See §11.1 |
| `● just changed` on the diff header | The pulse, but drawn as a **persisting label with a dot**, not a flash. So it has a **decay**. **Ruled 2026-07-31 and implemented: it persists for exactly one tick and cuts rather than fades.** It is the top rung of the recency ladder above, not a second mechanism, and the rung is deliberately *not* a duration: see §11.1 for why a wall-clock decay cannot be drawn without the timer I1 forbids. It marks **every** path in the newest tick, which is what §11.2 B2 said the pulse was for |
| Diff body: **syntax highlighted content**, five classes deep (`kw`, `fnn`, `typ`, `var`, `con`) over a default foreground | I2b, and a **class set** rather than a palette: what the picture commits to is which distinctions are worth a colour, not which colour each one gets. Ruled 2026-07-30 and implemented; the engine emits meanings and the shell colours them, which is what leaves the palette to #11. See §11.1 |
| A **tinted row** with a coloured left bar on every added and removed line | A per-row **background**. It is what separates a changed line from a context line in the picture, because the text itself is highlighted identically on both, and it is doing the same work the dimmed row above does: it is how the eye finds what moved without reading. Not drawable at sixteen foreground-only colours, where an ANSI background is a solid block rather than a tint. **Ruled 2026-07-31 and implemented** ([#11](https://github.com/breferrari/vigia/issues/11)): the wash is painted across the whole row, gutter and trailing blanks included, so it reads as a band rather than as a highlight behind some text. **The left bar is the sigil cell, inverted** — the diff hue behind, the row's own wash in front. The picture draws that bar three pixels wide in a nine-pixel cell, so it is sub-cell and has no terminal equivalent that does not spend a whole column, and I6 forbids spending one on decoration; the sigil is the one cell on the row that already means *this line changed*, so it carries the bar rather than a column being found for it. Both are absent on a palette that declines them and at a depth that cannot express them, which is where §11.1's recorded loss still stands |
| Status bar `0.8ms frame` | Instrumenting the render path and drawing the result. Self-referential: measuring and painting the number costs frame time that I9 gates. **Ruled 2026-07-31 and implemented: the p99 of the last 128 completed frames, nearest-rank, sampled every frame** ([#41](https://github.com/breferrari/vigia/issues/41)). The **statistic is p99 because I9 is**, and a readout reporting a median would be silent about exactly the frames the budget exists to bound. **128 because §7 already says why not 30**: at 30 samples a nearest-rank p99 is just the maximum, and at 128 it is rank 127, so one cold outlier is excluded and two are not. That is the behaviour a monitor wants, since the 60.97ms first-touch parse two rows down is a real frame that should not sit on the readout for two minutes. **A frame is the whole turn of the loop** — the wake, the drain, every `Frame::advance` in the batch, `View::collect`, and the paint — which is §7's rule that a gate is written against the caller's whole frame, applied to a readout instead. The number drawn is necessarily the **previous** frames', because a frame cannot include its own paint in what that paint draws; that is honest rather than stale, since it describes completed frames and stays true while nothing happens |
| Status bar `11MB` | A live RSS readout. I3 samples RSS in a **soak test**, never on screen; reading it per frame is a syscall on some platforms. **Ruled 2026-07-31 and implemented: it is read once per painted frame, on all three tier-1 targets, and drawn in `MiB`** ([#41](https://github.com/breferrari/vigia/issues/41)). Three things had to be settled and only one of them was cost. **The read is a syscall everywhere it ships**, which is not what the soak's own helper suggested: `soak.rs` spawns `ps` on macOS and `tasklist` on Windows, and the latter is **42.8ms median** on the reference machine against the 16ms I9 budget, 2.7x it for the read alone. What makes it affordable on screen is that both platforms have an in-process answer through a crate `gix` **already** puts in the graph, so neither costs a new crate: `libc::proc_pidinfo` on macOS and `windows-sys`' `GetProcessMemoryInfo` on Windows, against `/proc/self/status` on Linux, which needs nothing. The soak keeps its subprocess readers where the shipped one has none and is sampled 288 times across a window rather than per frame, which is what that 42.8ms is the reason for. **`MiB` rather than the mockup's `MB`**, because I3's soak is the only other place this quantity is quoted and it is in MiB throughout; two units for one number is two dialects, and a reader comparing the screen against a soak report would read the 4.9% difference as drift. `assets/preview.svg` was corrected to match. **And the number is "as of the last change", not "now"** — the trade that had to be accepted rather than solved: this shell wakes only on a filesystem event, so a pane left open on an idle tree shows the RSS from whenever the last write landed. Refreshing it needs a wake I1 forbids inventing, and the pulse's escape from the same wall does not transfer, because an event names paths and never bytes. It is consistent rather than broken: the diff on screen is *also* as of the last change, and this readout now carries the same contract as everything beside it |
| Status bar `follow ▶` | A follow-state indicator, which presumes the mode exists. **Landed with I5**, on the footer rather than a third chrome line: see §11.1. I6 later gives it a line of its own, above the hints, at the widths where one line cannot hold both. |
| Key hints `q quit · f follow · ↑↓ scroll` | A hint bar, and it **constrains I6**: roughly thirty columns of it must degrade legibly at forty. **Ruled 2026-07-30: it does not degrade by shortening.** The footer takes a second line instead, and only below the width where a whole line holds the bar does it drop hints, `JK files` first and then `jk scroll`. See §11.1 for why the first of those is a bonus rung that never decides the footer's height. See §11.1. |

Three of these are corrections rather than gaps:

1. **`f` toggles follow.** The mockup shows a dedicated key and a state indicator. That is the answer to §11.2 B1, and it was published before the question was asked. Ruled and implemented 2026-07-30; `f` is in `input.rs` and the rule is §11.1.
2. **The dimmed row and the `just changed` label are one mechanism**, not two: both are recency rendered as intensity. Specifying them separately would produce two decay clocks that disagree on screen.
3. **The table above is a list of elements and was silent about the container**, which is what let B4 sit `(proposed)` for two phases while the picture and the code drew different screens. Ruled 2026-08-01 ([#66](https://github.com/breferrari/vigia/issues/66)): the file list is a **region**, pinned above the diff, and §11.1 carries it. Every row in this table is unchanged by that; what changed is where three of them are drawn and for how long they stay there.

**And the picture's split of elements across the two regions is not kept**, which is the second deliberate departure from `assets/preview.svg` after the header's worktree name. It gives the summary rows no kind letter and the diff heading no counters, so each region draws a different subset. That is coherent only while the region shows every changed file, and an automatic unbounded changed set means it cannot: a file scrolled out of a capped region would take its counters with it and leave nowhere to read them. Both regions draw the same row through the same `Painter::file_row`, which also leaves one degradation ladder to gate rather than two.

**A third departure, older than either and recorded here because auditing the picture against the code hits it first: the mockup orders a row's right-hand side differently from the renderer.** The picture places sparkline, then counters, then heat strip, left to right, with the counters coloured green and red. `Painter::file_row` allocates right to left by priority — counters first because they are the row's content, then the pulse, then the heat strip, then the sparkline — so the drawn order is pulse, heat, sparkline, counters, and the counters are one dim string rather than two coloured ones. The renderer's order is the one that survives a narrow pane, because allocating by priority is what lets the ladder drop the least valuable element first; the picture's is the one that reads well at a width nothing degrades at. This is a departure in the **picture**, not a gap in the code, and it is left standing rather than redrawn: the mockup is a design artifact and its job here is to promise a layout, which after B4 it does correctly.

**A picture in a public README is a specification whether or not it is written down.** This one implied a retained time series, a recency gradient, two status readouts and a keybinding, none of which appeared in the spec, the roadmap, or any issue — while [#10](https://github.com/breferrari/vigia/issues/10) carried four of them in a single line. That is the same failure as §11: behaviour that exists somewhere real, with no line claiming it.

### 5.2 Where the mockup pulls against the invariants

§5 says design effort goes here "once the invariants hold", and [#10](https://github.com/breferrari/vigia/issues/10) repeats it: *"depends on the invariants holding first."* **That framing is wrong, and correcting it is the most consequential thing in this section.** At least two elements need retained state the frame path does not produce, in `vigia-core` rather than in the shell. They do not sit on top of the invariants. They **move** them.

**The sparkline needs precisely what eviction throws away.** `FrameStats.evicted` exists so the cached-diff map stays "bounded by the current diff rather than by everything ever edited" — that is how I3 is argued today. A churn sparkline is change density *over time*, so it has to survive a file settling: one that empties the moment a file stops changing shows nothing worth glancing at, and *"what was hot thirty seconds ago"* is the entire question it answers. Glanceability history therefore cannot live in the evicting map — and must not be unbounded either.

> [!NOTE]
> **I10 earned its row on 2026-07-31**
>
> It was written here as a **proposal**, deliberately kept out of the §3 table,
> because that table is for invariants with a failing test and this had none. It
> now has a budget (256 paths, 120 seconds), a fixture that drives ten thousand
> paths through it, and a soak assertion over the real process, so it has moved
> up. [#38](https://github.com/breferrari/vigia/issues/38).
>
> Two things it cost that this section did not predict. The store had to be fed
> from the **watch** rather than from the frame path, so `Tick` now carries every
> file a burst wrote instead of only the last, capped at the same 256 so a bulk
> operation cannot make a tick expensive before the store gets a chance to bound
> it. And the decay the mockup asks for turned out to be an I1 question rather
> than a rendering one: see §11.1.
>
> What it got right is the sentence everything else followed from. **The data
> structure was the decision and the drawing was the easy part.**

**The heat strip needs a whole-file property.** Locating change within a file requires that file's **total line count**, and the frame path is built to avoid exactly that: pure revalidation reads **0 bytes** (§10), the number I2a is written against. Measured naively — every changed file, every frame — it reintroduces the read I2a removed. It is cacheable per `(path, blob id)`, since a file's length cannot change without its content changing, so it is payable once per version rather than once per frame. **That caching is not an optimisation; without it the heat strip breaks I2a.**

> [!NOTE]
> **The cache above was never needed, and the reason is worth more than the prediction**
>
> **Corrected 2026-07-31 by building it.** [#39](https://github.com/breferrari/vigia/issues/39).
> The paragraph above is right that a whole-file read would break I2a and wrong
> that a new cache is what avoids it. `hunk::compute` interns **both sides** to
> diff them at all, so the working-tree side's line count is already computed on
> every diff and was being thrown away. It is now a field of `FileDiff`, which
> means it is cached and invalidated with the diff itself.
>
> That is **stricter** than the `(path, blob id)` key proposed here, not merely
> cheaper: a blob id names the index side, and a working-tree edit does not touch
> it, so a cache keyed that way would have served a stale length for exactly the
> file a reader is watching being written. `tests/frame.rs` gates the difference
> with an edit that keeps a file's byte length and changes its line count.
>
> The general shape, and it is the third time this project has hit it: **the
> expensive-looking property was a by-product of work already being done.** I5
> found the follow target already resolved by the gitignore filter; I10 found the
> burst's paths already resolved by the same filter; this found the line count
> already interned by the differ. Look for the by-product before designing the
> cache.

**Two status readouts measure the thing they run inside.** `0.8ms frame` means instrumenting the render path and drawing the result — a readout whose own cost falls inside the budget it reports, gated by I9 at 16ms p99. `11MB` is a live RSS number, and I3 samples RSS in a **soak test** precisely because reading it is a syscall on some platforms rather than free per frame. Both are honest to show; neither is free to show; the spec said nothing about either.

**Consequence for sequencing:** Phase 3 is not a rendering phase. Its two headline elements each need a core-side change with an invariant attached, so they belong in the same conversation as I2a and I3 rather than strictly after them.

**Confirmed by building the first one.** [#38](https://github.com/breferrari/vigia/issues/38) landed the sparkline, the recency gradient and the pulse, and the diff was mostly `vigia-core`: a new bounded store, a change to what a `Tick` carries, a new invariant with its own budget, and a soak assertion. The drawing was a bucket ladder and a three-step ramp. This section predicted that split and it was right, which is worth recording because the alternative reading — that Phase 3 is where the shell gets prettier — was the one both §5 and [#10](https://github.com/breferrari/vigia/issues/10) originally invited.

## 6. Architecture

Cargo workspace, two crates:

- **`vigia-core`** — library. Git (`gix`), diff modelling, incremental highlighting (`syntect`), filesystem events (`notify`), the watch and coalesce engine, the frame path. **No terminal I/O, no ratatui.** Every invariant except I6 and I8 is testable here, headlessly.
- **`vigia`** — the `ratatui` + `crossterm` shell: input, layout, theming. Thin by design, so the TUI stays swappable and the engine stays provable. A library with a five-line binary on top rather than a binary alone, because §7 makes the snapshot suite the proof for I5 and I6 and a test cannot import a `main.rs`.

The split is a dependency decision, not a hedge: the TUI renders whatever the core produces, so the core has to work first or the TUI is being built on sand.

`notify` is named here because I1 requires filesystem events rather than a timer, and each platform delivers them differently: `inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows. `notify` is the standard Rust abstraction over those three, and it keeps the binary pure Rust. Verified 2026-07-30 against `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-musl` and `aarch64-apple-darwin`: no `cc`, `cmake` or `bindgen` in any of the three graphs. The `-sys` crates it pulls (`inotify-sys`, `fsevent-sys`, `windows-sys`) are FFI declarations against facilities the OS already ships, so they compile no C.

**`libc` on macOS and `windows-sys` on Windows** are named here for §5.1's memory readout, and they are named rather than merely used because CLAUDE.md's rule is that a dependency reaches the spec before it reaches a manifest. Both are **target-conditional dependencies of `vigia` alone**, both are `unsafe` FFI declarations against facilities the OS already ships, and **neither adds a crate to any graph**: `gix` already puts `libc` into the Apple graph through `gix-hash` to `sha1-checked` to `cpufeatures`, and `windows-sys` 0.61 into the Windows graph through `gix-sec`. What each one buys is one syscall in place of a **subprocess**. `soak.rs` reached the same numbers by spawning `ps` and `tasklist`, which is affordable 288 times across an hour and not sixty times a second: `tasklist` alone is 42.8ms median on the reference machine against a 16ms frame. Linux needs neither, since `/proc/self/status` is a file read.

> [!WARNING]
> **Check the lock file, do not recall what a dependency costs**
>
> The paragraph above was nearly two platforms instead of three. `libc` was
> confirmed already-present with `cargo tree` and `windows-sys` was assessed from
> memory as *"a new crate in the graph"*, in the same hour, by reasoning that
> should have applied to both — and the Windows readout was filed as deferred work
> ([#56](https://github.com/breferrari/vigia/issues/56)) on the strength of it. The
> sentence directly above this one had said otherwise since Phase 1. **A
> dependency's cost is a fact about the lock file and is one command away, so an
> assessment of it that was not run is a guess** wearing a measurement's clothes.

**Coalescing stays ours.** `notify` has a companion debouncer crate, and taking it would move coalesce policy out of `vigia-core`, which is the one place I1 is testable. `notify` supplies raw events and nothing else.

**There are two coalescings, and only the first is that one.** `vigia-core` coalesces **events**: which filesystem writes count as one change. That is I1's, and it stays in the core for the reason just given. The shell coalesces **paints**: how many frames one burst of wakes is worth. That is I9's, it cannot live in the core because a paint is the shell's and because one of the two wake sources is the terminal, which §6 puts outside `vigia-core` entirely, and it decides nothing about which events are real. It costs I1 nothing, because draining a queue that is already there is a `try_recv` and never a wait: an idle shell still blocks on one `recv` and still does no work. [#45](https://github.com/breferrari/vigia/issues/45).

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
- **An invariant the engine can only make possible gets a second gate over the caller**, in `crates/vigia/tests/`. I4 is the case: the core fetches content per file, so painting the top of a large diff without reading the bottom is *available*, and nothing in the core stops a renderer from asking for every file anyway. Asking is the natural way to write one. So what one screenful costs is gated where the screen is, against two fixtures differing only in changed-file count, for the reason the tier above gives. **Either tier can be the one that has to move there**, and I2b needed both: `reads.rs` holds the structural half, and `crates/vigia/tests/budgets.rs` holds an *absolute* one, because highlighting follows the viewport and so happens in the shell — a frame timed in the core is a frame with the syntax parser missing.
- **A budget measured at one position is measured at its cheapest one.** A caller starts at the top, so a gate that never scrolls tests the corner of the space where the least work is possible. I2b's frame gate ran at row zero and passed while a frame four screenfuls down cost 53ms against a 16ms budget, sustained and with no input; its own assertion was right and could not fire. Any gate over a windowed view is parameterised by where the window is.
- **A frame-time gate that never paints has timed half a frame.** The third along the same axis as the two above, and it went unnoticed longest because the omission is invisible from inside the gate: every budget gate in this repo timed `Frame::advance` plus `App::view` and stopped, so `render` was outside both tiers on both crates. That is where a row's *width* is decided, so the cost of drawing was measured by nothing at all. A row carrying **7.2x more line than pane** then passed a 16ms gate for two phases, and what found it was a reader scrolling a Japanese README rather than any test. The rule generalises past this instance: **a gate is written against the caller's whole frame, and any stage left outside it is a stage nothing can regress you on.** `crates/vigia/tests/paint.rs` holds the structural half, against a counter the renderer returns. [#45](https://github.com/breferrari/vigia/issues/45).
- **A fixture over ASCII cannot tell a column bound from a character bound**, because over ASCII they are the same number. The budget fixtures are generated source, 34 columns of it a line, so *"a row costs the pane"* and *"a row costs the whole line"* produced identical counts across every gate in the suite. Anything measured in **rendered width** needs a fixture where width and length disagree: `Scratch::wide_lines` is 531 columns over 412 characters of Japanese, emoji and Latin. Same shape as the row-budget gate's hundred-file fixture, one axis over.
- **A fixture population sitting on its tooling's default configuration cannot observe the code that exists for the non-default.** The same family as the line above, one axis further out: there the fixtures were uniform in their *content*, here they are uniform in how they were *built*. Every fixture in this repository is created by `Scratch`, which shells out to `git init` and sets `core.autocrlf false`, so both sides of every diff were always LF, git's clean filter never had anything to convert, and the code path that skipped it produced byte-identical output to the code path that runs it. Not one test was wrong and not one of them could have failed: [#65](https://github.com/breferrari/vigia/issues/65) drew a file `git diff` called unchanged as a 1,790-line rewrite while the suite was green, and what found it was pointing the tool at a real repository. **The tell is checkable and is the one to reach for: delete a normalisation, filter or conversion step and see whether anything goes red.** That is a mutation test with a specific target, "does any fixture make this branch's presence observable" rather than "is this branch covered". The answer is to **span** the axis rather than move along it, so `Scratch::crlf_worktree` stands beside `Scratch::new` instead of replacing it. Git alone offers `autocrlf`, `.gitattributes` filters, `core.symlinks`, `core.ignorecase` and sparse checkout; a fixture built by `mkdir` and `write` has silently taken the same position on all of them.
- **A gate that settles before it measures has measured the cheapest state.** The same mistake as the line above, along a different axis: time rather than position. `settle()` exists so a fixture written moments ago can be *proved* unchanged, and every structural gate in `crates/vigia/tests/reads.rs` calls it first, so the settle margin — the one window in which the frame path deliberately recomputes instead of reusing — was the one window nothing measured. §10's claim that the margin breaches I9 under a bulk rewrite therefore went two phases with no gate on either tier, in either direction: nothing would have caught the breach if it were real, and nothing showed that it was not. An invariant that governs a window needs a gate that runs *inside* it. [#32](https://github.com/breferrari/vigia/issues/32).
- **A gate can assert the defect it was named against, and the tell is an exact small count where the rule is a bound.** `crates/vigia/tests/scroll.rs::the_bottom_of_the_diff_is_content_rather_than_blank` was written against a blank pane, said so in its name, said so again in its comment — *"an empty pane, which in a monitor is indistinguishable from a broken one"* — and then asserted `view.rows.len() == 1` against a twenty-two row body. Every word around it was right and the one line that runs pinned the failure in place. That is worse than the absent gate §7 already warns about, because a defect with a green gate over it is one nobody goes looking for: [#57](https://github.com/breferrari/vigia/issues/57) was found by a reader watching an agent run `git reset --hard`, two phases after the gate shipped. The signature is a **specific small number where the rule is about a bound** — "the screen is full" is `== height`, never `== 1` — and it is legible by reading the assertion against the test's own name, which is the cheapest review there is and was never done here.
- **An invariant whose two failure modes are not symmetrical gets a gate for each.** I2a is the case that made this a rule. Reusing too *little* is slow and loud, and the budget gate catches it. Reusing too *much* is fast, passes every budget, and shows a diff that no longer exists, so `crates/vigia-core/tests/frame.rs` compares every reused frame against one computed with no memory at all. A budget gate alone would have called the second failure a success.
- **A rule covering a window too small to race is extracted as a pure function and tested directly.** The racily-clean guard in §6 cannot be reached on demand from a filesystem: dropping it leaves the whole integration suite green and only a unit test over the decision function goes red. Where a correctness rule has no reachable integration path, the pure-function test *is* the gate, and that is worth stating rather than discovering.
- **Gates are mutation tested before they are trusted.** Break the code deliberately, confirm the gate goes red, restore. Two of the flaws found this way were invisible to reading: a structural gate comparing one call against the sum of its own calls, and a p99 over too few cold samples.
- **Steady-state budgets are sampled after a warmup**, and over enough frames for a percentile to be one. I9 is a claim about steady state, so the cold path is outside its scope by definition; measured cold frames run ~40ms against a warm p99 of ~3ms, and at 30 samples a nearest-rank p99 is just the maximum.
- **A CI guard fails the build if `cc`, `cmake` or `bindgen` enters the dependency graph** on any tier-1 target, plus a musl build asserting the binary links no shared libraries. The pure-Rust constraint is what makes musl-static and Windows cheap, so it is enforced rather than trusted.
- **The soak drives the whole pipeline, not the engine.** I3 is a claim about the process a reader leaves open, so measuring `vigia-core` alone would prove it about a program nobody runs. The harness is `vigia::run` with the terminal removed: a real `notify` watch thread, real coalescing, `Frame::advance` per tick, follow and scroll through `App`, `View::collect` driving the `Highlighter`, and `render` into a `ratatui` buffer. What it leaves out is named rather than discovered later: the terminal takeover, which needs a tty and is I8's; the input thread; and index writes, because the loop reaches the same mass-eviction state by reverting files and keeping `git` out of the measured window is what makes the temp-file gate exact.
- **The soak is two tiers, like every other budget here.** *Structural* gates — the retained caches bounded by the current diff and by the viewport, eviction actually happening, and **zero temp files retained** — are counters and directory entries, so they are hardware-independent, take no slack, and run in every `cargo test`. The *absolute* gate is RSS drift, and it is the one that has to be scheduled.
- **A drift gate over a window shorter than its own warmup is measuring warmup.** RSS climbs to an allocator plateau in the first minutes of any process and only then goes flat, so a short window reads that climb as a leak and a generous threshold would then wave a real leak through. The gate therefore discards a warmup prefix, compares the median of the first quarter of what remains against the median of the last quarter, and **refuses to assert at all** below a window where that leaves two ends worth comparing. Refusing is the point: a gate that cannot say "no drift" has not been tested, and one that says it from four samples is worse than absent.
- **A bound is only evidence when something reached it, so a gate over one asserts that eviction happened as well as that the bound held.** I10 is the case that made this a rule and the numbers say why: the per-commit soak window touches about eighty distinct paths against a cap of 256 and never turns the 120-second window over once, so the per-sample bound `tracked <= 256` is satisfied there the way an empty room satisfies a fire code. The soak therefore **refuses to assert** when the run reached neither eviction rule and prints why, exactly as the drift gate does; the deterministic proof is a fixture that drives ten thousand paths in every `cargo test`. Verified by running the soak at 300 files, where it reports 256 tracked at the cap with 207 evicted, so the gate can say yes and not only "not applicable".
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
- [ ] Rename tracking cannot stream. Pairing a deletion with an addition needs the whole walk, so with it on the first change arrives at 97% of the walk and time-to-first equals time-to-last. It is on by default anyway, because reporting a move as an unrelated delete plus add misdescribes what the agent did. Confirm against a week of real use, or paint without renames and reconcile them on a later frame. Tracked by [#48](https://github.com/breferrari/vigia/issues/48); the real-use half is [#72](https://github.com/breferrari/vigia/issues/72).
- [x] ~~Re-diffing every changed file costs 18.58ms p99 on a 100k-line diff, over the I9 budget, against 3.27ms for a single file.~~ **Closed 2026-07-30:** I2a is enforced. The frame path revalidates from the index blob, the change kind and a `stat`, so a single-line edit in a 100-file worktree recomputes exactly one diff and reads exactly that file. Measured over the same 100-file, 100k-line fixture, release build: a **real frame under continuous edits is 6.97ms p99** against the 16ms I9 budget, revalidating 99 files and recomputing the one that moved. Pure revalidation with nothing edited is 3.93ms and reads **0 bytes**. A cold frame with nothing to reuse is 18.28ms and reads 3.6 MiB, which agrees with the 18.58ms the spike measured over the primitives and is the cost I2a removes. Every number in this bullet is a frame-path measurement; the 18.58ms above is the spike's, over `Worktree::diff` called per file.
- [x] ~~The settle margin is a fixed 2 seconds and is over a hundred times more conservative than NTFS needs. Narrowing it per worktree, from the smallest positive difference between the modification times status already reports, removes a measured I9 breach. Do this before the soak test.~~ **Closed 2026-07-30: the margin stays at 2 seconds, and both halves of that were wrong.** [#32](https://github.com/breferrari/vigia/issues/32).

  **The estimator is unsound**, because it bounds the *smallest* granule observed while soundness needs the *largest* that occurs, and a filesystem's granularity is not uniform within one volume. Measured on the reference NTFS volume, release build: 10,324 same-length rewrites of one file over 3s produced 1,959 distinct stamps whose positive gaps spanned **502µs to 17,522µs, a 34.8x spread**, and a bulk write of 100 files produced 20 distinct stamps with a smallest cross-path gap of **998µs**. Narrowing the margin to 998µs would leave a real 17.5ms granule uncovered and serve exactly the stale diff `settled` exists to prevent. The "about 16ms" the old bullet predicted is itself under the 17.5ms measured. Nothing passive does better: a monitor never writes, so it only ever sees the gaps its user's tools happened to leave, and those bound the granule from the wrong side. An active probe would have to write into the worktree to measure the right filesystem, which a read-only monitor will not do, so 2 seconds stays — an upper bound chosen from the table of filesystems in `frame.rs` rather than inferred from a sample.

  **And the breach is the fixture rather than the product.** The 18 to 21ms was measured over the *core* frame path, whose fixture materialises every file to avoid passing vacuously. Over the same fixture, the same event and 2.5s of frames, release build: the core ran **98 of 182 frames over the 16ms budget** (p50 19.71ms, p99 22.34ms, max 35.42ms), and the shell ran **0 of 1060** (p50 2.36ms, p99 2.66ms, max 11.90ms). I4 already makes the shell diff only what it draws, which over this fixture is about one file a frame, so the margin costs it one recompute rather than a hundred. What is left is the core's own number, and removing it is [#19](https://github.com/breferrari/vigia/issues/19)'s job rather than the margin's. **I3 ([#5](https://github.com/breferrari/vigia/issues/5)) is unblocked**: there is no redundant work in the shipped shell for a soak to see.
- [ ] **I3 is gated, and the window in its budget has not been run.** Measured on the reference machine, release, over the same 100-file, 100k-line fixture the budget gates use, **3600 seconds**: 73,265 frames, every one of them a full screen, from 70,086 write rounds and 14,018 files created and deleted. **RSS drift 2.18%** against the 5% budget, 25.6 MiB after warmup against 26.1 MiB at the end, peak 27.3 MiB, from a cold 19.7 MiB. At most 68 diffs held against 106 changed files, over 14,118 distinct paths ever changed, which is the number that separates "bounded by the diff" from "bounded by the session". Zero temp files retained, zero failed frames, 2.0 GiB read and 1.59M lines highlighted. Tracked by [#47](https://github.com/breferrari/vigia/issues/47).

  What that does **not** cover is the window §3 names. Four hours is what the scheduled job runs, nightly on Linux and weekly on all three tier-1 targets, and 24 hours needs a runner without the six-hour cap. Recorded as open rather than closed, because a gate that fires and a budget that has been met at its own window are different claims.

  **And there is a residual slope that one run cannot call.** The hour's post-warmup quarter medians rise monotonically, 25.58, 25.86, 25.90 and 26.14 MiB, a least-squares **+0.92 MiB/h**, which extrapolated naively to a day would be over budget. It is *not* extrapolated, because the 900-second run of the same code slopes **−0.70 MiB/h** with flat quarters and reports 0.17% drift, and the two disagree in sign. Both are about one percent of the **+81.3 MiB/h** that a deliberate 1 KiB-per-frame leak produced, so what is left is run-to-run variation at this sample size rather than a trend, and the honest way to settle it is the window the budget already names.

  It also fixes where the warmup ends, which is the number §7's rule is written against: RSS reaches its plateau about **thirty seconds** in, while the same code over a fifteen-second window reports **10% drift** and is measuring nothing but the climb.

  **First scheduled run, 900 seconds, 40 x 500, all three tier-1 targets, green:** Linux 17,804 frames at 2.56% drift, macOS 6,053 at 2.49%, Windows 20,427 at 0.00%. Every one hit the viewport bound at equality, and Linux reports **9 descriptors at the first sample and 9 at the most**, which is the first time that metric has been observed at all. The reference machine's own two runs were 0.17% and 2.18% over the same statistic. So the observed spread of a **healthy** process is roughly **0 to 2.6%** against a 5% budget, which is real headroom and not a lot of it: a longer window could cross the line on variation rather than on a leak, and the answer if it does is a measured warmup or a measured budget, never a wider one.

  Two narrower gaps travel with this one. File handles are only countable from `/proc`, so the descriptor gate has never run on the machine these numbers come from and is exercised for the first time by CI on Linux. And the measured window deliberately contains no index writes: the loop reaches mass eviction by reverting files instead, which is what keeps `git` out of the private temp directory the retained-file gate asserts on, so staging over a long run is soaked by nothing.
- [ ] The frame path walks status to completion before it reports a file list, so it does not stream the way the raw change iterator does. Two reasons it costs nothing today: rename tracking cannot stream either and is on by default, and a scrollbar needs the file count regardless of how few files are drawn. What is open is whether both hold at ten thousand changed files, where the walk itself could exceed I4. Revisit together with rename tracking above, since they stand or fall together. Tracked by [#48](https://github.com/breferrari/vigia/issues/48).
- [x] ~~The header counts changed files and not changed lines. A repository-wide `+`/`-` total needs every file's diff, and I4 makes first paint independent of total diff size, so the two cannot both hold on the first frame. §5's counters are per-file and cost nothing extra, since a file has to be diffed to be drawn; only the total is affected. What is open is whether it is worth computing behind the frame and revealing when it arrives, which belongs with the rest of §5 in Phase 3.~~ **Ruled 2026-07-31: the header does not carry one, and the first reason is the product class rather than the cost.** [#49](https://github.com/breferrari/vigia/issues/49).

  **A repository-wide total is a reviewer's number.** It summarises a changeset for someone deciding about it, which is the other product class in §2. What a monitor is asked is what is happening *now*, and the header's file count together with §5's per-file counters answers that already, at no cost and with nothing rounded off. §5.1's own rule points the same way: the picture had a header to spend and spent it on `watching · 3 files`, so the published artifact answers this by omission rather than leaving it open. That header now reads `vigia · 3 changed` … `watching` ([#67](https://github.com/breferrari/vigia/issues/67)), which moves two of the three facts and adds none: the omission this ruling rests on is unaffected.

  **The cost is what makes it unaffordable rather than merely unnecessary.** The total needs every changed file's diff, so drawing it puts first paint back in proportion to the size of the diff, which is the single thing I4 exists to forbid. At the ten thousand changed files [#48](https://github.com/breferrari/vigia/issues/48) names, the walk alone could exceed the budget.

  **And the one variant that dodges I4 is forbidden by I1.** Computing behind the frame and revealing when it arrives is a wake no filesystem event caused, and in the interval between the tick and the reveal the header would carry a number for a diff that has already moved: a stale claim that looks live, which is the frozen-clock failure §11.1 rejected for the pulse decay on the same grounds. The pulse escaped by being redefined against **event identity** rather than elapsed time. Nothing here can, because an event names paths and never magnitudes, so there is no cheap identity to define a total against.

  **The by-product check §5.2 asks for came back negative, and that is the part worth recording.** That rule has paid three times in this project — I5's follow target, I10's burst paths, and the heat strip's line count were all already being computed and thrown away — and this is the first time it has not. `FileDiff.added` and `removed` exist only where a diff was computed, and I10's history buckets count ticks per path rather than lines. Nothing already in flight adds up to this, so there was no cheap version to find.

  Conditional variants fall together: drawing the total only while the worktree is small enough to afford it puts an element on screen whose presence rule is invisible to a reader, and it would disappear at exactly the moment a tree got busy enough for anyone to want it.

  Gated by `crates/vigia/tests/render.rs::the_header_carries_no_changed_line_total`, over the **drawn row** rather than over what a frame cost. `tests/reads.rs` cannot see this one, and that was confirmed rather than assumed: with a total drawn into the header, all eleven of its gates stayed green, because a total computed on a handle of its own never touches the frame stats they read.
- [x] ~~Is `syntect` fast enough incrementally to hold I2b, or does it force tree-sitter — and with it a C toolchain — back in?~~ **Answered 2026-07-30: it holds, and tree-sitter stays out.** Release build, with `regex-fancy`; `syntect`'s *default* engine is `regex-onig`, which is oniguruma, so the defaults would have put `cc` in the graph on their own. Loading the bundled grammars is **318µs** against I7's 50ms, so nothing has to be deferred to first use. One screenful of Rust is **1.53ms** against the 16ms I9 budget, and a real shell frame under continuous edits over the 100-file, 100k-line fixture is **10.52ms p99** (p50 7.95ms, max 11.62ms), against **6.97ms** for the core frame path alone before any of this existed. What is not affordable is parsing a hunk *whole*: the 1006-line hunk that fixture produces costs **60.97ms**, which is 3.8x over budget and would be paid on every frame under I9's own shape. So parsing forward only as far as the screen has asked is load bearing rather than an optimisation, which is the same shape I2a found for re-diffing. Revalidation is a hash of the hunk rather than a counter, because inside the settle margin the frame path re-diffs an untouched file every frame and a counter would re-highlight files nobody edited. The cost that stays: the bundled grammars take the release binary from **3.20 MiB to 5.04 MiB**.
- [ ] Highlighting a hunk is forward-only, because `syntect` parses a line from the state the line before it left. The **first** frame that draws deep inside a large hunk therefore pays for everything above it there: landing on the last row of the 1006-line hunk above costs the whole **60.97ms** parse, once. `G`, a follow jump into a large file, and scrolling **up** into the bottom of the previous file all land there directly; scrolling down never does, because a screenful at a time is a screenful of parsing. §7 puts the cold path outside I9 by definition, so this is a first-touch cost rather than a breach, but it is a real hitch on one keypress and is recorded rather than assumed away. The fix, if daily use asks for one, is to bound the parse per frame and leave the tail unclassified until the next — which needs a redraw the event loop has no reason to schedule today, and I1 forbids inventing a timer to get one. Tracked by [#51](https://github.com/breferrari/vigia/issues/51).

  **Daily use did ask, and the half that was fixable is fixed.** [#45](https://github.com/breferrari/vigia/issues/45) reported a fast scroll dropping frames, and the measurement split this bullet in two. A *first* entry from below still costs the whole walk and is still the cold path: **25.01ms** for a 120-row hunk of wide-character Markdown, against 139µs for a frame that reused it. That is unchanged, because the timer objection above is unchanged. What was not the cold path is a hunk re-entered after leaving the screen, which was paying the same 26.39ms for a parse that had been in memory one frame earlier, and `RETAINED_HUNKS` fixes it: four hunks are kept after they scroll off, so reversing direction over ground already read costs **397µs** instead. Bounded by a constant added to the viewport, not by the session, so I3 sees a slightly higher plateau rather than drift.

  **And the per-line cost is a function of line length, not of the script.** The obvious hypothesis was `fancy-regex` backtracking over CJK, and it is wrong: 660-byte Markdown parses at **0.32µs a byte** against **1.76µs a byte** for the 34-byte Rust lines above, so wide content is *cheaper* per byte and expensive only because the lines are nineteen times longer. Nothing here is Japanese-specific. Attributed by running the identical bytes under an extension `syntect` has no grammar for: **708µs of a 764µs collect** at p99 is the parse.
- [x] ~~Does a drawn row cost the pane it is drawn into, or the whole line behind it?~~ **Answered 2026-07-31: the whole line, and it is fixed.** [#45](https://github.com/breferrari/vigia/issues/45). `printable` walked every character of a span and `put_runs_marked` clipped only afterwards, so an 80-column pane showing a 531-column line walked and allocated all of it. Measured over a 22-row body of Japanese: **8231 source characters examined to fill 1600 columns, 5.1x**, and unchanged by pane width, which is the signature of a cost that follows the content. Bounded now at the pane, and the same body examines **1342**. What it is worth in wall clock is small and is stated rather than rounded up: the paint went from about **104µs to 60µs p99**, against a 16ms frame. It is fixed anyway, for the reason the `heat_of` bullet below gives: the shape is the one I4 exists to forbid, and leaving a known instance of it both unfixed and ungated is what let this one reach a reader. The gate is `crates/vigia/tests/paint.rs`, and the counter it reads is `PaintStats`.

  Two things fell out of it that are worth more than the microseconds. **No gate in this repo had ever painted** (§7 now says so as a rule), and **no fixture had a line wider than a pane**, so neither tier could have caught this whatever it cost.
- [x] ~~A trackpad reports one flick as a stream of scroll events. Is one redraw each affordable?~~ **Answered 2026-07-31: no, and it was the larger half of the report.** Each wheel event was its own full frame, so a fast gesture rendered every position it passed through, and over a large diff most of those positions enter a hunk nothing has parsed. The loop now drains what is already queued, handles every wake in arrival order, and paints **once** per batch, capped at 64 so a faster event source cannot starve the screen. Measured structurally: a hundred notches drawn one at a time highlight 4x more lines than the same travel drawn once, and the reader lands in the same place. It costs nothing to I1, because a drain is `try_recv` and never a wait.

  > [!NOTE]
  > **What is *not* open here, because it was a breach and is fixed**
  >
  > The **repeat** of that cost was, and it was found by auditing rather than by
  > using the tool. A hunk whose content changed used to throw its whole parse
  > away, so the walk above was paid on *every* frame for as long as the file
  > being read was the file being written — 520 lines per frame for a 22-row
  > body, **53ms p99** four screenfuls in, sustained and with no input. A changed
  > hunk now rewinds to the deepest parse position the new content still agrees
  > with, so a frame costs a screenful plus one `CHECKPOINT_STRIDE` at any depth:
  > 40 lines and **11.47ms p99** on the same fixture, against 10.02ms at the top.
  >
  > Two lessons went into §7 rather than staying here. A budget measured at one
  > position is measured at its cheapest one, and the caller-side gate §7 asks for
  > can be the *absolute* tier rather than only the structural one.
- [ ] **The heat projection is the one drawn thing whose cost follows the file rather than the window.** Placing a change needs its working-tree line number, and a line number is only known by counting the lines before it, so `heat_of` walks every line of a drawn file's hunks even though the viewport shows a screenful. Everything else in the shell follows the screen: rows above the window are counted rather than built, highlighting is asked for only on a row that is pushed, and a file the viewport never reaches is never diffed. This is not a read, so `reads.rs` does not move and I2a is untouched; it is arithmetic over lines already in memory, and it runs only for rows actually drawn. **The count that "actually drawn" means changed on 2026-08-01**, and the measurement below predates it: it used to be one or two headings a frame, and the pinned list of §11.1 adds one per row it draws that the diff walk did not already build, so the bound is now `LIST_ROWS` plus the headings in the diff. The walk hands its entries to the list precisely so the overlap is not paid twice, but the non-overlapping rows are new walks. Measured, release, over the 100-file 100k-line fixture **before the region existed**: the shell frame went from **7.66ms to 8.14ms p99** against the 16ms I9 budget, and on the same runs the **core** frame path, which does not execute any of this code, moved 6.96ms to 7.29ms. So most of the difference is run-to-run variation and what is left is a few tenths of a millisecond. Recorded rather than assumed away, because the shape is the one I4 exists to prevent even where this instance of it is affordable. If it ever matters, the fix is to carry the line number on `Line` rather than recomputing it, which is the same information the gutter already derives twice. Tracked by [#55](https://github.com/breferrari/vigia/issues/55).
- [ ] Default view: unstaged only, or working-tree-vs-HEAD? Unstaged is the thesis; confirm against a week of real use. Tracked by [#50](https://github.com/breferrari/vigia/issues/50), and the week of real use it waits on is [#72](https://github.com/breferrari/vigia/issues/72).
- [ ] Windows: supported target or best-effort? **Half answered 2026-07-31 by [#11](https://github.com/breferrari/vigia/issues/11), and the halves are separated the way B5's were.** The technical half is closed: degradation is a mechanism with a gate over it rather than a hope, so a console that cannot draw 24-bit gets 256 by detection and not by accident, and the colours it does draw were chosen for it. That was the part of this bullet that named an actual risk. What is left is a **posture** question with no work in it, which is whether the README and the release notes say *supported* or *best-effort*, and it belongs with the first release rather than here. `ROADMAP.md` has recorded that placement since before this was half-answered.

## 11. Behaviour

§3 says how well `vigia` does things. Every number in it is defensible and every one has a test. Nothing above says **what happens when you press a key, or when nothing has changed** — and that gap is why the product reads as a screenshot with budgets attached.

Two parts: what the shell already does, recorded because it was decided in code first, and what is still undecided.

> [!WARNING]
> **Behaviour decided in code without a line here is a defect**
>
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

**The working-tree side is normalised the way git's clean filter would normalise it**, and the diff is taken against that rather than against the bytes on disk. Git does not compare raw bytes either: it runs the working-tree side through the clean filter first, so on a checkout with `core.autocrlf=true` — what the Windows installer writes — the CRLF on disk is turned back into the LF the blob holds before anything is compared. Skipping that made every line of every such file differ from its stored form, which is [#65](https://github.com/breferrari/vigia/issues/65): a 21-line edit drawn as **+905 −885**, and a file `git diff` calls unchanged drawn as a whole-file rewrite, on the installed default of one of three tier-1 targets. Two shapes, and the smaller is the one that was reported: with `* text=auto eol=lf` only an editor writing CRLF creates the discrepancy, while with no `.gitattributes` at all the checkout itself does, so *every* edit to *any* text file read as a rewrite.

**A file whose only difference is its line endings reports no change, and stays listed while doing so.** The second half is a ruling rather than a leftover. Dropping it from the list needs its content during `Frame::advance`, which is a read per changed file on the path I4 governs and is exactly what that invariant forbids; and a header count disagreeing with the body under it is the contradiction B3's empty state exists to remove. Both halves are also what git does, which is the tie-breaker rather than a coincidence: `git status` lists such a file and `git diff` reports nothing for it.

**The rules the filter applies are re-read once a frame, not once a process.** `.gitattributes` and `core.autocrlf` decide what a diff normalises, and both are things the agent in the other pane is free to write: a monitor that resolved them at startup would go on drawing the old answer indefinitely while a restart drew a different one, which is I5 failing silently over a window I3 measures in days. So `Frame::advance` drops the filter and the next working-tree read rebuilds it, which bounds staleness to one frame without giving up the laziness: a frame whose diffs are all reused reads nothing and so rebuilds nothing, and only a frame already going to disk pays. `gix` rebuilds its own attributes stack on every status walk, so this matches the freshness of the walk it is paired with rather than inventing a policy. `crates/vigia-core/tests/normalise.rs` gates it **through `Frame::advance`** rather than through two bare calls to `Worktree::diff`, because nothing in the latter marks a frame boundary and a gate written that way passes whatever the filter's lifetime is.

**`core.safecrlf` is not consulted, because it guards writing.** It makes `git add` refuse a file whose conversion would not round-trip, so that history cannot be corrupted by one; `git diff` ignores it entirely, which is checkable and is checked, since the same mixed-terminator file git **refuses to add** is one it reports as unchanged. `vigia` never writes, so the guard covers an operation it does not perform, and inheriting it meant failing the frame over one file with mixed line endings, on that frame and every frame after it, because the file stays mixed. One such file anywhere in the tree stopped the pane working. Found by auditing rather than by using it, which is the only reason it is not a third row in the table above.

**External filter drivers are not run, and that is §6 rather than an omission.** `gitattributes` can point the clean filter at an external program, and Git LFS is the common one: a repository using it sets `filter.lfs.clean` to a command. Running those would mean spawning a process per file per frame, which is precisely the shape §6 rules out when it takes an in-process diff over a `git diff` subprocess per tick. `eol`, `working-tree-encoding` and `ident` are pure Rust and do apply. What the ruling costs is recorded rather than hidden: under LFS the worktree holds real content while the blob holds a pointer, so an LFS-tracked *text* file diffs as a rewrite. That is not a regression, since it is what happened before any filter ran at all, and it is [#69](https://github.com/breferrari/vigia/issues/69). What decides that a file is **binary** is a separate question this file does not yet answer, and is [#68](https://github.com/breferrari/vigia/issues/68): the engine sniffs content for NUL bytes, which is git's fallback and not its rule, so a path the attributes declare `binary` is diffed as text anyway.

**The body is two regions: a pinned file list, a rule, and the scrolling diff.** This is B4, ruled 2026-08-01 as a **layout** question rather than the navigation one it was written as ([#66](https://github.com/breferrari/vigia/issues/66)). The proposal that stood here for two phases — *"one continuous scroll, list as map"* — answered *navigable?* and settled *is it a region at all?* underneath it, in the direction the code happened to have taken. `assets/preview.svg` draws the other answer and has since before the question was asked: `src/engine/watch.rs` appears **twice** in it, once at `y=92` in a block of three summary rows and once at `y=202` as the diff heading, with a rule at `y=178` between them, which one stream never does. §5.1's rule that a published artifact answering an open question **is** the answer applies to layout exactly as it applied to `f`.

**What it buys is the thing §5 calls the differentiator actually staying on screen.** Three of the four glance elements — sparkline, heat strip, counters — ride a `Row::File` heading, so in one stream they are visible only while that heading is, and scrolling two hundred lines into a file takes every file's glance signal off the screen at once. That is the inverse of what a monitor is for: §2 buys *readable at a glance*, and a glance surface that survives only at scroll position zero is a glance surface that is absent whenever anyone is actually reading.

**The list stays not navigable, which is the half B4 proposed and which stands.** There is no selection, no focus and no second mode: `▸` marks the file the diff is inside and is not a cursor, and no key changes meaning depending on where anything is. B4's own rationale is the reason — *selection implies focus, focus implies a second mode, and modes are reviewer-class* — and it is honoured here rather than overridden. The region is a **map**, which is what "list as map" wanted and what a stream of headings could not supply, because §11.2's phrasing presupposed a list to be a map *of* and there was none, there were headings.

**Its height is the changed-file count, capped at six, plus the rule.** Three changed files draws three rows, which is the picture exactly; a formatter touching two hundred draws the cap and scrolls. Six is the largest block that still reads as a glance rather than as a list to be searched, and on the 24-row pane this tool is built for it leaves fourteen rows of diff after the header, the rule and a one-line footer.

The height is a function of **pane geometry, follow state and changed-file count**, which is exactly the set `Footer::plan` takes, because the list divides whatever the footer leaves. None of those is transient, and that is the property being bought: the region cannot jog a reader's diff the way a notice or a passing failure would. It does move when the footer does — `f` on a forty-column pane takes a second footer line and the list gives up a row for it — and that is a reader's own keypress changing the shape of the screen, which is the same thing pressing `f` already did before this region existed. It gives way on a short pane the way the footer gives up its second line, and the diff keeps `MIN_BODY` rows before the list gets any.

**The list is ordered the way the stream is**, which is `Frame::files()`, which is status order. That is what makes it a map rather than a second opinion: the caret's place in the list and the thumb's place on the diff's scrollbar then describe the same position. Ordering by diff size was rejected on I4 — it needs every changed file's diff, which is [#49](https://github.com/breferrari/vigia/issues/49)'s argument against a repository-wide total arriving in a second place, landing here identically. Ordering by recency from `History` is genuinely free and was rejected for a different reason: it decouples the list from the stream, so the caret stops corresponding to the scroll position and the map stops being one.

**A list row draws exactly what a heading in the stream draws**, through the same `Painter::file_row` and the same degradation ladder. This is the **second deliberate departure from `assets/preview.svg`**, beside the header's worktree name, and it is recorded rather than absorbed: the picture splits the elements across the two regions, giving the summary rows no kind letter and the diff heading no counters. That split stops being defensible the moment the list scrolls, because a file scrolled out of the region would lose its counters altogether and there would be nowhere left to read them.

**A visible list row costs one `Frame::diff` and no more**, which is what keeps the region inside I4 rather than beside it. The bound is the region's own height, never the changed-file count, so the cost follows the window exactly as the body's does. That is an I4 claim rather than a new invariant, and `crates/vigia/tests/reads.rs` is where it is gated, alongside the ones that already bound the body.

**What that row costs depends on which side of the settle margin the file is on, and both halves are worth saying.** Settled, it is a `stat` and a cache hit reading **zero bytes**, which is I2a doing its job and is the state a monitor sits in for almost all of its life. *Inside* the margin it is a real read, because `Frame::diff` cannot prove a file written in the last two seconds unchanged and re-reads it by design. So a six-row region on a tree being actively written costs six reads a frame rather than six `stat`s, and the earlier claim here that the region's doubled ask was "a count that doubles and not work that does" was true only of the settled half.

**The two regions therefore hand entries to each other rather than asking twice.** The file an agent has just written is the one file the margin is certain to cover, it is always the file the diff is inside, and it is always in the window while the list follows, so asking the frame for it a second time read and diffed it again on **every frame this tool exists for**: 258,790 bytes where 36,970 will do, measured over twenty files of five hundred lines. `View::collect`'s walk now records the entries it builds and `take_list` draws from those, which is the same one-pass rule that section of the code already followed for the diff. `a_screen_a_single_file_fills_reads_that_single_file` gates it as a relation, that the region computes one fewer diff than it asks for, so it survives a change to the region's height.

**The list follows the diff, and `J`/`K` move it without moving anything else.** It slides on its own to keep the caret visible, so the region is correct untouched the way I5 requires of everything else. Shift is the modifier because `Ctrl-J` is LF and `Ctrl-C`/`Ctrl-D` already quit, Alt is intercepted by terminal emulators and by macOS Option, and `G` has already taught a reader that case is load bearing here. A plain letter also reaches terminals that never report a modified arrow, so neither binding is the only way in.

**`J` takes the window over, and anything that moves the diff hands it back.** That is the list's own version of follow mode, and it is a mirror rather than an analogy: a manual scroll takes the *diff* away from following and gives the *list* back to it, in the same line of the same function. It was ruled the other way first, as *snap back when the diff lands outside the window*, and that rule is wrong in a way only building it showed: the window can never be away from the current file in the first place, because the frame after `J` drags it straight back, so the keys do nothing. The two halves are both load bearing. Without the takeover `J` is inert; without the handback a reader who browsed once has a map that never agrees with the diff again. **The caret marks the current file whenever the window still shows it, and is simply absent once a browsed window has moved off it.** It is not suppressed by the takeover, and that distinction is worth stating because the opposite reads as the tidier rule: the caret says *the diff is in this file*, which stays true while a reader browses, and hiding it on a window that is showing that file would withhold a fact the reader can act on. What the takeover changes is where the window looks, not what the mark means.

**Scrolling the list does not disengage follow mode**, and that is a ruling rather than an omission. Follow is a claim about the **diff** viewport, and moving a window over a map expresses no intent about what the diff should show — the same reasoning that already exempts a terminal resize one paragraph down. Browsing the changed set while the diff goes on following what an agent is writing is the monitor behaviour, and the two would fight if one disengaged the other. `Action::is_manual_scroll` is an exhaustive match precisely so that a new action has to answer this rather than inheriting a default.

**Both regions carry a scrollbar, and both are exact.** The list's thumb spans the visible window over the changed-file count, both of which are free. The diff's spans the screen's rows over the diff's **total** rows and sits at the rows above it, which is what every other scrollbar means.

**The diff's was ruled coarse first, and that ruling was wrong.** It read that a total needs every changed file diffed, which I4 forbids, so the bar approximated the whole from the current file's height. Reported from use within the hour: it vanished on a short file, ballooned on a long one, and never reached the bottom, because the trailing files are rarely the height of the one being read. The argument was right that *diffing* every file is unaffordable and wrong that a total needs it. A height is hunk boundaries and line counts; a `FileDiff` is those plus an owned `String` per drawn line. Counting instead of building took the reference fixture from **442.71ms to 8.76ms**, against `git diff --numstat`'s 46ms for the same work, and §3's I4 now carries the narrowing that admits the walk.

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
| `J`, `Shift-↓` | scroll the file list down one row |
| `K`, `Shift-↑` | scroll the file list up one row |
| `f` | engage follow mode, or disengage it |
| mouse wheel | scroll the region under the pointer |
| drag either scrollbar | move that region |
| click a listed file | put the diff at that file |
| terminal resize | redraw, no state change |

**The caret travels the window; the window moves the least it can.** It is held while the current file is inside it and pushed by exactly the overshoot when the file leaves. Scrolling from the start therefore walks the caret down the rows, and only then does the list move under it. **Two fixed positions were ruled first and both were wrong**, in opposite directions and for one reason: a constant row is not following a file, it is dragging the window on every step. Ending the window on the current file showed the six files *before* the six the diff was drawing. Starting the window on it fixed that and pinned the caret to the first row, which reads as the list scrolling while the marker never moves. Both reported from use. Minimal movement subsumes them: at the top the caret is on the first row because the file is first, and at the end the pull-back rests the last file on the bottom row.

**A click on a listed file puts the diff at it.** The gesture a reader tries without being told, next to a region that draws the names of the things it is a map of. It is **not** selection and B4 stands: nothing is remembered, no row becomes special, no mode appears, and the event after it means what it would have meant. That is the same argument that already licensed dragging a scrollbar, and the caret is still a marker rather than a cursor: it cannot be moved on its own. A click on the diff below stays inert, because nothing there is selectable.

**The wheel reads where the pointer is, and it is the only thing in this shell that does.** A reader hovering the map and turning the wheel means the map. §2 makes `btop` the reference and that is what `btop` does. It is not selection and it is not focus: nothing is remembered between events, so B4's ruling stands.

**A drawn thumb is a published affordance, so both are draggable**, and a press anywhere on a track jumps to it. The scrollbar column is tested before the region containing it, or every drag would read as a wheel.

**A track maps onto travel, not onto the whole.** The list's travel is its file count less its own height; the diff's is its total rows less a screenful. Mapping onto the whole instead leaves the last screenful's worth of track dead, since every fraction past the bound clamps to the same window, and the pointer then reaches the bottom with the view still short of the end. **The diff's drag resolved against the file count until 2026-08-02**, which is what its bar had counted before the narrowing above made the bar row-exact: three long files gave a track dozens of rows tall three landing spots. Reported from use, which is the fifth time a defect on this surface was, and the gates are `dragging_the_diff_bar_resolves_to_a_row_and_reaches_the_end` and `dragging_the_list_bar_reaches_the_first_file_and_the_last`. Both check the **middle** of the track as well as its ends, because the clamp makes the ends agree either way.

**Scroll position is `(file, offset within that file)`, never a row index.** A frame that changes something above the viewport therefore does not teleport the view. This is a correctness property, not an implementation detail: with a row index, an agent writing to a file earlier in the list would yank the reader's position on every keystroke it makes.

**A position past the end of the diff resolves to the last screenful, not the last row.** The two are not the same place, and taking the second for the first draws one line of content over a blank screen while the header goes on truthfully saying how many files changed: one thing on screen contradicting the other two, which is the ambiguity B3's empty state exists to remove. A pager rests the final row at the *bottom* of the viewport and so does this. Resolving it is `View::collect`'s alone, because `App::view` stores the resolved position back on every frame, so there is one rule in one place rather than a clamp in the scroll path and another behind it.

Reachable two ways, and **only one of them is scrolling**. The other is the diff shrinking under a position that was reasonable when it was taken: `git reset --hard`, a branch switch, an agent reverting its own work. That is an ordinary event on the pane this tool exists for and it is exactly when someone looks over, which is why it is here rather than filed as an edge case. [#57](https://github.com/breferrari/vigia/issues/57).

The walk back reads only the files the screen is about to draw, so I4 is untouched. It is deliberately **not** what `G` does: `End` goes to the last *file* from its top. That was ruled on cost, and **the cost argument expired on 2026-08-01** when I4 was narrowed: adding up every file's height is what `Frame::height` now does once a tick, so a row-exact `End` is affordable in a way it was not when this was written. The behaviour stands on the other reason, which is what it should always have rested on: `g` and `G` are the two ends of the *file list*, the pair the header counts and the pinned region draws, and a `G` that landed mid-file would make the two keys mean different units. Backing off from an end already in hand still costs a screenful.

**Follow mode**, which is I5. `less +F` semantics, and the toggle the README mockup already published: follow is **on at startup**, **any manual scroll disengages it**, and **`f` re-engages it and jumps straight to the newest change** rather than waiting for the next one. The footer shows `follow ▶` while it is engaged.

Two boundaries are load bearing, because each is a way for the mode to be quietly wrong rather than visibly broken. A **terminal resize does not disengage**: it moves no viewport and expresses no intent, and a monitor beside an agent is resized constantly. And **`G` disengages rather than re-engaging**, because "jump to the last changed file" and "resume following" are different intents that would otherwise be the same key, leaving a reader unable to look at the newest file without also re-arming the view.

**The newest change is the file whose write landed last in the settled batch**, and the batch is the coalesced tick. A monitor cannot find that file by looking: `stat`-ing every changed file is the cost [#19](https://github.com/breferrari/vigia/issues/19) already records as breaching I9 at scale, and I4 forbids reading files the frame does not draw. It does not have to look, because the filesystem event already carries the path. `Tick` reports it, so following costs no read, no `stat` and no diff. When the named path is not in the diff — an index write, or an edit reverted before the tick landed — the view stays where it is, because there is no newest *change* to follow.

**How the chrome fits**, which is I6. One rule, and the layout follows from it: **a thing made of items breaks, a thing made of characters marks its edge, and content is neither.**

The **hint bar is a list**, so when the footer cannot hold both halves on one line it takes a **second line** rather than shortening anything. The state moves to the upper of the two and the hints keep the bottom row, so narrowing a pane never moves the hints out from under a reader's eye. It grows only while at least two body rows survive — a monitor with no diff left in it has stopped being one — and only when there is a state worth moving. A notice, which replaces the hints, inherits that whole line when there is one, but **never causes it**: a notice is transient, and a footer that grew for one would jog the reader's diff down a row and back every time a file vanished between being named and being read. The height is a function of width, follow state and changed-file count, all of which change only when the diff does.

Below the width where even a full line holds the bar, it drops **whole hints** and never part of one: `JK files` first, then `jk scroll`, then `q quit`, leaving `f follow` last.

**And `JK files` is a bonus rung, which is a rule rather than a position in that list.** The footer's height is decided by the widest bar a reader is owed at forty columns, not by the widest bar that exists; rungs above that are drawn where there happens to be room and are never worth a row. Adding this one made the widest bar exactly forty columns, so the footer began taking a second line at the width I6 is named for, and every reader lost a body row to advice including the ones who never press `J`. The list is also the one hinted thing a reader can see without being told: it is drawn on every screen with room for it and it slides on its own, where `f` restores a state whose *absence* is invisible. That is why `f` is last standing and this is first to go. `q` and `jk` are pager reflexes and four keys reach quit, while `f` is the one nobody would guess and the only one that restores a state a reader can lose without noticing. The state has its own ladder, `follow ▶  N/M` then `follow ▶` alone, because the header already carries the file count. **State outlives advice at every width**, which is what keeps the mode visible when the pane is at its worst.

**And advice outlives instrumentation**, which is the rung the two status readouts join. `0.8ms frame` and the memory cell sit left of the state as one **diagnostics** group, dropping the memory cell first and the frame cell second, so both are gone before a single hint is. That ordering is not the ladder rule above repeated: the hints are how a reader *operates* the tool and the state is what the *tree* is doing, while these two describe **`vigia` itself**, which is the least of the three things a narrow pane owes anyone. Memory before frame time, because the frame cell reports a budget a reader can act on and the memory cell reports a claim that barely moves.

**Neither readout can change how many rows the footer takes**, and that is a rule rather than a consequence. `Footer::plan` decides its height from the state ladder alone, exactly as it did before these existed, and the diagnostics take only what is left after the hints have their widest fitting rung. Two things would otherwise jog a reader's diff down a row and back for no reason they could see: the frame cell **does not exist on the first paint**, because no frame has completed to have a p99 of, and the memory cell does not exist on a platform with no cheap read. That is the same rule a notice already has one paragraph up, and it is here for the same reason — a transient or absent thing must not move content.

**Both cells are a constant width whatever they say.** `  0.8ms frame`, ` 12ms frame` and `999ms frame` are all eleven columns, and the number is right-aligned inside them, so a value changing between frames never shuffles what is beside it. Past the field, magnitude gives way to a sigil rather than to precision: `>1s` for a frame and `>9GiB` for memory, because the exact value at those magnitudes tells a reader nothing the sigil does not, and a number that grew a column would move the footer under their eye.

The **header never grows**, and after the ruling above that has to be said of both sides rather than of one token. A worktree name is not a list and has nowhere to break, so it marks its edge like every other single token; the clause it now leads *is* a thing made of items, and it breaks the way the rule says by dropping a whole rung rather than by taking a line. A second line could not guarantee a fit either way, and it would spend a body row on a maybe.

**The header is laid out by subject, the way the footer is.** Two facts about the **tree** on the left, the worktree name and then how much of that tree has changed, and the one fact about **`vigia` itself** on the right, which is whether the watch is still live. That is the three-subject rule the footer runs on further up — advice, then what the tree is doing, then what `vigia` is doing — applied to the row at the other end of the screen. **The grouping is borrowed and the priority is not**, and the difference is worth stating because the two rows rank the same subject oppositely: on the footer, what `vigia` is doing is the least a narrow pane owes anyone and the diagnostics drop first, while on the header the self-fact is the last thing standing. Neither is inconsistent, because the priority was never the subject in the first place. It is **recoverability**: the footer's readouts describe a budget and a number that barely moves, and both are recomputed on the next frame, where a header that has stopped saying it is live leaves a reader with no way to find out. It is a ruling rather than an arrangement, and [#67](https://github.com/breferrari/vigia/issues/67) is what it costs to get wrong: the header used to seat the count beside the mode word, where `watching · 3 files` read as *"watching 3 files"*, a participle with an object naming a curated watch set that does not exist and that B6's no-flags ruling puts out of scope. Neither fact was wrong. Their **adjacency** was, and a separator that promises one subject is what let English supply one. Reported by a reader who asked, in order, what `watching` is, how to toggle watching something, and whether it is automatic. All three questions follow from the row, and the screen confirmed the wrong answer rather than correcting it, because `3 files` sat directly above exactly three summary rows.

The count reads `changed` rather than `files` for a reason that only appears once it has moved: `vigia · 3 files` would be a **worse** claim than the one it replaced, since the repository has more than three files in it. Next to the name of a tree, a bare noun describes that tree; `changed` is what makes the number a fact *about* it. It is also one rule where there were two, because `changed` is a participle with no plural to inflect.

**The ladder keeps its order and changes only which side each rung is dropped from.** Widest first: `{worktree} · {N} changed` with the mode word, then the worktree name with the mode word, then the worktree name alone. That is the ladder; it is not the whole list of screens, because the two sides degrade independently and the paragraph below gives the five states a narrowing pane actually walks through. The count still yields before the mode word, for the reason it always did. Inside the left-hand clause the rule the rest of this section runs on decides the rest: the count is an **item** and drops whole, the name is a **token** and marks its edge, and the count is the one that goes, because the name is the header fact a reader cannot reconstruct by looking at the body and because B3's empty state leans on it to say which repository this is. Both facts on the left are drawn in one weight, which is the visual half of the same ruling: two weights inside one clause would tell a reader in colour that these are separate claims, which is the seam being removed.

**The left-hand side leads with the worktree, not the program**, and this is the first of the two places the layout departs from `assets/preview.svg` on purpose, the other being §5.1's element split. The picture puts `vigia` there. A title bar spends six of forty columns telling the reader which program they started, which is the one thing they already know, and what they do not know from looking is *which tree* — the question that decides whether two panes side by side mean anything. Written here 2026-07-31 rather than left in a code comment, because §5.1's own rule is that a published artifact answering a question is the answer, so a deliberate departure from one is exactly the thing that has to be argued in the open or it reads as drift.

And it is the worktree's own name **even when the path it was given does not contain one**. `vigia .` hands `gix` a workdir of `.`, which has no final component, so the header drew `.` — the single thing it exists to say, and the one thing it could not. The path is resolved for **display** before its last component is taken. Display only: the value the watch compares event paths against is left alone, because resolving it there would introduce `\\?\C:\` on Windows and `/private/var` on macOS, and [#30](https://github.com/breferrari/vigia/issues/30) is the record of what a root that matches no event path costs.

**The mode word, and the set it implies.** It sits on the right, alone. The mockup drew it there with the count beside it; `assets/preview.svg` was corrected with the ruling above rather than departed from, so the picture and the shell agree on this row and the deliberate departures stay at two.

| Mode | What it means |
|---|---|
| `watching` | the watch is live, so the screen follows the tree |
| `not watching` | the watch never armed, or it ended, so this is a still picture |

**Two, and I1 is the reason rather than minimalism.** §5.1 read `watching` as implying at least a settling state and an idle one. It implies neither, because both are *durations* and this shell wakes only when something changes. `Watcher::next_tick` blocks until a burst has settled, so the shell is never awake **during** settling and could not draw it without a redraw nothing schedules; and "idle" needs a wake that says nothing happened, which is the timer I1 forbids. Either word could come into existence and then never leave, which is precisely the frozen clock the recency ladder above rejected for the pulse. A header that lies about the present until the next file is written is worse than one with fewer words in it.

**The word starts at `watching`, before the watch is armed.** Arming happens after first paint on purpose, so the watch does not observe the shell's own setup reads. A third word for those microseconds would flicker on every single launch to describe a state that always resolves the same way within one wake, and when arming genuinely fails the failure arrives as its own wake and corrects the word.

**`not watching`, not `stalled` and not `still`.** It is the mockup's word negated, so a reader who has learned one has learned both. `stalled` reads as temporary when this is not, and `still` means both "motionless" and "continuing".

**The mode is state, so the reason for it is not.** A watch that ends puts `not watching` on the header **and** its error on the footer as a notice. The header carries what is durable — this diff has stopped being live — and the notice carries which failure did it, which is not. Before this the durable half rode the notice alone, and survived only because the tick that clears a notice can never arrive again once the watch is gone.

**And a dead watch is drawn loud while a live one stays quiet.** `watching` takes the header's dim grey, the one the footer's own secondary tokens are drawn in; `not watching` takes the footer's alert. The changed-file count left that class with [#67](https://github.com/breferrari/vigia/issues/67) and now takes the worktree name's weight, which is the ruling one paragraph up rather than an exception to this one. A state nobody can catch at a glance has not been reported, and §5 makes colour half the differentiator, so a header whose failure looks exactly like its working state fails twice. It is the **footer's own** alert rather than a colour of its own, because the notice carrying which failure already uses it and the two halves of one event should not arrive in two different reds. That is a reuse of an existing style rather than a palette decision, which stays [#11](https://github.com/breferrari/vigia/issues/11)'s.

**The mode word outranks everything else wherever it fits at all**, which is why it holds the side that is placed first: the count summarises a body that is on screen and can be recovered by counting it, and whether the pane is still live is recoverable from nowhere. That is the footer's own rule further up, and it matters most at exactly the widths where the body has nothing in it to count.

**It is not, however, the last thing standing on the row, and the difference is measurable rather than pedantic.** The two sides have independent budgets: the word is all-or-nothing at its own width, while the name marks its edge at any width above zero. So on a live watch the name has the row alone from 5 to 7 columns, the word takes it alone from 8, a marked fragment of the name rejoins it at 10, and both are drawn whole from 14; with `not watching`, four columns wider, those are 5 to 11, 12, 14 and 18. Widening a pane from 7 to 8 columns therefore *removes* the worktree name. That is unchanged behaviour and it is what placing the right first has always meant; it is recorded because the tidier claim — one total order over three facts — is false, and this file's own rule is that an invariant with no failing test is a wish. There is no test for the tidier claim because there is nothing true to gate.

**The numbers in the paragraph above are gated by `crates/vigia/tests/legibility.rs::the_header_degrades_at_the_widths_the_spec_records`**, and that is a rule rather than a courtesy. They were wrong here first: this said both facts appear from 13, where 13 draws `vig› watching` and only 14 draws the name whole. A measurement quoted in prose and nowhere else drifts from the thing it measured, silently, because no document fails.

**The count is nothing at all when it is zero**, the same way a position is nothing when there is no diff to be positioned within. `0 changed` spends columns restating what the empty state says below in words.

**And there is no fourth fact.** The header carries the worktree, the changed-file count and the mode word, and it does not carry a repository-wide `+`/`-` total. That is a ruling rather than an omission, and it is [#49](https://github.com/breferrari/vigia/issues/49): a total needs every changed file's diff, which puts first paint back in proportion to the size of the diff and is exactly what I4 forbids, and computing it behind the frame to dodge that needs a wake no filesystem event caused, which is I1's. §10 carries the argument and §5.1 records what it settles about the mockup. `crates/vigia/tests/render.rs` gates it over the drawn row rather than over what a frame cost, because a total computed on a handle of its own would never reach the stats `tests/reads.rs` reads.

**And the mode word is never cut.** It is drawn whole or dropped, which is stricter than the marking rule the rest of the header follows: `wat›` is a state a reader cannot read, and unlike a path it has no half that identifies it. **What delivers that is `put_right`, which draws its token whole or not at all**, rather than a ladder of the word's own. It had one until [#67](https://github.com/breferrari/vigia/issues/67), because this side carried the count too and so had a real choice to make; moving the count left left a ladder of one rung wrapped around a mechanism that was already doing the work, and two mechanisms guaranteeing one rule is a rule nobody can locate.

**The empty state**, which is B3. This is the screen the tool sits on most of the time, and the first thing anyone sees when they open it beside an agent that has not written yet, where a blank pane and a hang look identical.

Four facts, two of which the header already carries, so the body spends one line rather than four:

| Fact | Where it is drawn |
|---|---|
| which repository | the header's left-hand side |
| that it is watching | the header's mode word |
| which branch | the body line |
| that there is nothing to show | the body line |

The line reads `no unstaged changes · main`, and `no unstaged changes` alone when HEAD names no branch.

**It no longer reads `working tree clean`, and that phrase was wrong rather than merely plain.** It is git's, and git means index-against-HEAD as well as tree-against-index. This diff is the working tree against the index, so a worktree with every change staged draws zero files and was being told it was clean while `git status` said the opposite. `no unstaged changes` is exactly what zero files means here, untracked files included, since an untracked file is unstaged too.

**The branch is orientation, not the comparison.** Nothing about it changes what is diffed, and it is named anyway because two agents on two worktrees of one repository are otherwise identical on screen, which is the multi-worktree case §4 defers rather than rejects. A detached HEAD names no branch and the line drops it rather than inventing one: `HEAD@abc123` would put a commit id in a monitor that shows no commits.

**It costs one `.git/HEAD` read, taken only on frames that draw no diff.** I4 holds because the thing read is the thing drawn, and the frame that draws it is the cheapest one there is: nothing to diff, and nothing else to read.

And a token that had to lose characters says so, in the direction it lost them. `…` on the **left** means the beginning is gone, and only a file path uses it, because the end of a path is what names the file. `›` on the **right** means it continues past the edge: the worktree name, a notice, a hunk header, a note, the empty-state line. A hunk header silently cut to `@@ -258,7 +25` reads as a different line number, which is the failure this closes.

**A clipped diff line is marked too, and is not a truncated label.** Content cannot wrap, because a wrapped line moves every line below it and the shape of the screen stops meaning anything. Nor can it elide, because unlike a label no part of it is the identifying part. So it is clipped and marked `›`, and I6's "truncated-to-useless labels" is read as being about labels rather than about content. That is a ruling, not an omission: the alternative was a horizontal pan, which is a key and a mode this spec does not name.

**How a diff line is coloured**, which is what I2b pays for. Content is syntax highlighted, and the mockup had already ruled how: added, removed and context lines all carry the same classes, the sigil is green or red, and the line number stays faint. **Ruled 2026-07-30: follow the picture literally**, per §5.1's rule that a published artifact answering an open question is the answer. Only content is highlighted. A file heading, a hunk header and a note are chrome, and chrome that changed colour by what it happened to name would stop being readable as chrome.

Three things follow, and each is a constraint rather than a preference.

**The engine emits meanings, not colours.** `vigia-core` maps a syntax scope onto one of nine classes and stops. Which colour a class gets is the shell's, and therefore [#11](https://github.com/breferrari/vigia/issues/11)'s. A core that emitted truecolour would have settled the palette question in the one place §6 says has no terminal in it, and it would have pre-empted the 256-colour degradation path before anyone wrote it.

**The diff signal degrades to the sigil column, and that is a loss rather than a simplification.** What separates an added line from a context line in the mockup is the row tint and the left bar of §5.1, and sixteen foreground-only colours can draw neither. So until #11 lands a background, `+` and `−` carry it alone. Recorded out loud because §5 makes shape and colour the whole differentiator, and this is the one place where following the picture spends some of it. The alternative considered and rejected was to keep unclassified text green on an added line and red on a removed one: it preserves the wash, it contradicts the picture, and it would have made the shell's colours mean two different things at once.

**A file type nothing recognises is not an error.** The syntax is resolved from the path, by extension and then by whole file name, and anything unresolved draws exactly as it did before there was highlighting at all. A monitor that refused a file because it could not colour it would have inverted its own job.

**How recency is drawn**, which is I10 and which §5.1 asked for as two separate things. It is **one ladder with three rungs**, read from one store through one lookup, and each rung is an intensity on the file's own heading:

| Rung | What it means | How it draws |
|---|---|---|
| **pulse** | named by the **most recent tick** | brightest, plus `● just changed` |
| **live** | has a sample inside the 120-second window | plain |
| **cold** | nothing in the window, so nothing is tracked for it | faintest |

Three rulings are inside that table and none is cosmetic.

**The top rung is not a duration, and I1 is the reason.** §5.1 reads the mockup's persisting label as a decay, and a decay measured against a wall clock has to be *seen* decaying, which needs a redraw nothing schedules. Inventing a timer to get one is exactly what I1 forbids, and it is the same wall §10's highlight-tail bullet already hit. So the window stays real time and the **sampling** is event-driven: the store advances once per coalesced tick, on the wake that was already going to redraw. Nothing on screen changes without something changing on disk, which is what a monitor is for. Defining the pulse as *newest tick* rather than *last three seconds* is what stops that from being a lie: a tree that has gone quiet keeps the label on the file that really is the newest change, indefinitely and correctly, instead of freezing a clock mid-count.

**Cold is not a fourth rule.** A path with nothing left in the window is dropped from the store entirely, per I10, so "cold" and "untracked" are the same state. That is also what the **first** frame of a session looks like: a worktree that was already dirty draws every row cold, because a monitor has no way to know what happened before it was looking and inventing a recency for it would light up rows nothing has touched.

**Three is a property of the store, not of the palette, and this file said otherwise for a phase.** §5.1 and `theme.rs` both promised that the *real* gradient arrives with [#11](https://github.com/breferrari/vigia/issues/11)'s truecolour, on the reasoning that sixteen foreground-only colours have only bold, plain and dim to spend. The arithmetic is right and the conclusion does not follow. What #11 widens is how many distinct colours a rung may be drawn *in*; what decides how many rungs there **are** is `Recency`, and it has three variants because the store can answer exactly three questions about a path: is it in the newest tick, is it anywhere in the window, or is it not tracked at all. A fourth rung would have to mean *how far through the window*, which nothing currently computes and which the row above makes expensive to add honestly: the window is real time, the sampling is event-driven, and a fraction of it read on a quiet tree is a number that ages without being redrawn. So #11 replaced bold, plain and dim with three chosen luminances, and stopped there. Widening the ladder is an I10 change with a budget attached, not a theme.

The general shape, and it is the second time this file has recorded it: **a limit blamed on the rendering layer was a limit of the data behind it.** §5.2 caught the mirror image, an expensive-looking property that turned out to be a by-product already computed. Before assigning a limitation to the thing that draws, check what the thing that supplies can actually distinguish.

**The sparkline is a thing made of items**, so under the layout rule above it **breaks**: it drops whole buckets, oldest first, and never draws a partial one or a squeezed strip.

**The heat strip is a fourth kind of thing, and the rule needed a third clause to say so.** *A thing made of items breaks, a thing made of characters marks its edge, and content is neither* covers a list, a token and a line. A heat strip is made of items and is **not a list**: its items are slices of one file, in order, and the set of them *is* the claim. Dropping the last six would draw the first half of a file as though it were the whole of it, and a reader would conclude the tail is untouched. Less detail is honest; a prefix presented as a whole is not.

So **a projection re-projects rather than dropping items**: a narrower rung sums adjacent slices and classifies the sums, which at halves is exact. Twelve slices, then six, then none. `crates/vigia/tests/legibility.rs` gates it with a file changed at both ends and quiet in the middle, which is the one shape where truncation and re-projection differ, and it reads **cell colours** rather than symbols because every slice draws the same block.

**And the two strips scale against different things, deliberately.** A sparkline's height is measured against the busiest bucket **on screen**, because the question a reader asks down a file list is which file is busiest, and a row scaled against itself draws every file at full height the moment it is the busiest thing it has ever been. A heat slice's intensity is measured against the busiest slice **of its own file**, because the question is where in *this* file the work is, and the neighbouring rows have nothing to do with it. One is a comparison down the list, the other across a row. Its heights are scaled against the busiest bucket **on screen** rather than against each row's own maximum, because scaling per row draws every file at full height the moment it is the busiest thing it has ever been, which answers a question nobody asked while destroying the one comparison a file list is for. A file with no churn draws no strip at all rather than an empty one, so it spends none of its row.

**And no glance element may take a heading below twelve columns of path.** The counters, the pulse and the strip are all placed from the right, in that order of priority, against a floor the path keeps. A row reduced to `M …` would have stopped naming its own file, which is the truncated-to-useless shape I6 forbids, arrived at by decoration rather than by narrowing.

**CLI.** One optional positional path, defaulting to the working directory. No flags today, and an argument beginning with `-` is refused with one line naming that fact rather than being taken as a path, so `vigia --help` is told there are no options instead of being told `--help` is not a repository. That is not `--help` implemented; B6 leaves the question of whether it should be, and rules everything else about this line.

**Configuration is a theme file and one environment variable**, which is B6 ruled 2026-07-31 and amended the same day. The CLI gains nothing either way.

**A palette is a preference, so it lives in a file.** `~/.config/vigia/theme` is read when nothing overrides it, resolved from `HOME` or `USERPROFILE`, which is **one** rule rather than one per platform: no XDG matrix, no `%APPDATA%` special case, no discovery crate. `VIGIA_THEME` still names a built-in or points at a file, and still wins, because a variable is how you say "not this time" without editing anything.

**A colour depth is not a preference, it is a property of the terminal you are in right now.** One machine, one user, one afternoon: this pane is truecolour, that one is an `ssh` into something ancient, the third is CI. A file gives all three the same answer and is wrong for two of them. So `VIGIA_COLOR` stays a variable, which is the same reasoning that makes `NO_COLOR` one.

The amendment costs less than B6 assumed because **the format and the parser already existed**: `VIGIA_THEME` has always been able to point at a theme file. What was added is a place to look, not a surface. And it removes a step rather than adding one, since a preference set once now survives a new shell.

### How colour degrades

`SPEC.md` §5 makes shape and colour the whole differentiator, so a palette that assumed 24-bit colour would be a product that silently stops working on the terminals that do not have it. §10 named that and left it open. This is the answer, and it is **two axes rather than a fallback chain**.

**The palette decides what may be drawn. The depth decides how finely it can be expressed.** Both have to allow an element before it appears, and they genuinely disagree: `ansi` refuses a row wash at *every* depth, because a wash has to assume a background and that palette's entire contract is that it assumes none, while `dark` draws one wherever the depth can express it.

Three palettes ship. **`ansi` is the default**, unchanged from what shipped before there was a choice: every colour in it is one of the sixteen *names*, so it resolves to whatever the reader's own terminal scheme says and `vigia` matches the pane beside it instead of fighting it. It is the only palette that is correct on a background nothing has detected, and detecting one needs a tty round-trip this shell does not make. `dark` is `assets/preview.svg` read out of the file rather than approximated. `light` is the same design re-picked at light-background luminance, with every ramp reversed, because on white it is the dark end that has contrast to spend.

The depth ladder, and what each rung loses:

| Rung | Foreground | Background |
|---|---|---|
| truecolour | 24-bit, as authored | as authored |
| 256 | quantised to the xterm palette | quantised |
| 16 | nearest of the sixteen names | **dropped** |
| none | dropped | dropped |

**Background is dropped a rung above foreground**, and that asymmetry is the one thing here worth arguing with. §5.1 already records that an ANSI background is a solid block rather than a tint: at sixteen colours the darkest available green behind a line of code is a slab, and a slab destroys the syntax colours sitting on it. Worse than no tint. So the row wash stops at 256 while the text goes down to sixteen, and below that the diff signal narrows back to the sigil column exactly as §11.1 recorded before #11.

Detection is a precedence chain, first answer wins: `VIGIA_COLOR`, then `NO_COLOR`, then `TERM=dumb`, then `COLORTERM` claiming 24-bit, then **Windows with `WT_SESSION`**, then `TERM` containing `256color`, then **Windows**, then sixteen.

**Windows sits above `TERM`, and that ordering was learned rather than designed.** Git Bash and MSYS export `TERM=xterm-256color`, so a chain that read `TERM` first sent the most common shell for this repo to 256, where a subtle colour has nowhere to land: the xterm cube's darkest axis levels are 0 and 95 with nothing between, so a row wash of `#1b3d29` quantises to `#005f00`, a saturated primary rather than a tint. A terminal that names itself is better evidence than a variable describing a terminfo entry.

**Windows detects truecolour, and 256 was not a safe default but a different wrong one.** What it was protecting against is consoles older than Windows 10 1703, which has not been a supported target for years. A reader on something genuinely older says so with `VIGIA_COLOR`, which is what the override is for.

**`TERM` only ever promotes.** On a Unix it is the whole signal; on Windows it describes a world where sixteen was the floor, so reading it as a demotion would take colours away from consoles that have them. The floor is sixteen because it is the one answer that is never actively wrong.

**Modifiers survive every rung, including none.** `NO_COLOR` asks for no *colour*, and bold is not colour. On a monochrome terminal it is the only distinction left.

**Quantising happens once, at startup.** The palette is walked a single time and the result stored, so the frame path draws with values that already mean what this terminal can show and I9 never sees any of it.

**What a wider palette does not buy is a wider ramp.** Mapping a 24-bit colour onto sixteen entries cannot keep nine syntax classes distinct: `#ffa657` and `#e3b341` are both yellow, and that is arithmetic rather than a choice. `ansi` is the answer for a sixteen-colour terminal and hand-picks all nine. What may **never** collapse at any rung that has colour at all is the diff signal, because §5 spends it on colour, and that has its own gate.

**A theme that does not parse is reported before the screen is taken**, which is the rule B5 already states for a path that is not a repository and holds here for the same reason: an error painted inside a TUI that then hands the terminal back is an error nobody sees. An unknown key is **refused rather than ignored**, with the line it was found on, because a silently dropped key is a theme that does nothing and "it was discarded" is the one explanation a reader cannot arrive at by looking at their screen.

**A path that is not a repository exits before the screen is taken.** That is the first half of B5, and it shipped with the shell rather than being ruled first: `Worktree::discover` and the opening walk both run ahead of `Session::enter`, so the failure reaches a terminal the reader can still read. An error painted inside a TUI that then hands the terminal back is an error nobody sees. Recorded 2026-07-31, when [#40](https://github.com/breferrari/vigia/issues/40) found §11.2 still calling it proposed, which is this section's warning box in miniature.

### 11.2 Undecided

Each carries a recommendation marked **(proposed)**. None is settled until ruled on, and none may contradict §3 — if one does, §3 wins and the recommendation is wrong. A ruled item moves to §11.1 and leaves its number behind here, because the numbers are cited elsewhere and renumbering would silently repoint those citations.

> [!NOTE]
> **This heading used to read "these gate Phase 2"**
>
> It stopped being true the moment Phase 2 closed with B3 through B6 still open,
> and it stayed on the page for a whole phase after that, because a heading
> naming a phase goes stale on a date nobody is watching. Which phase a decision
> blocks belongs in `ROADMAP.md`, the file whose job is phases; what belongs here
> is only whether a thing is decided. Corrected 2026-07-31 by
> [#40](https://github.com/breferrari/vigia/issues/40), which found it while
> ruling B3.

**B1 — What happens to follow mode when the reader scrolls. Ruled 2026-07-30: the proposal stands. See §11.1.** `less +F` semantics, on at startup, disengaged by any manual scroll, re-engaged by `f`. Rationale, kept because it is the part a later reader will want to argue with: disengage-on-scroll is the only rule that never fights a reader mid-read, and a dedicated toggle beats overloading `G`/`End`, because "jump to the last file" and "resume following" are different intents that would otherwise be the same key.

*An earlier draft of this bullet proposed `G`/`End` as the re-engage key. That contradicted the mockup, which is public and predates the question. When a published artifact already answers an open question, it is the answer — the question is only whether to keep it. It was kept.*

**B2 — Which file wins when several change at once. Ruled 2026-07-30: the proposal stands. See §11.1.** Follow the file whose write landed **last** in the settled batch, and let §5's visual pulse carry the others. Rationale: it reads "newest" literally, it is stable rather than heuristic, and the pulse already exists to say "these moved too" without moving the viewport for each.

What ruling it exposed, and what §11.1 now records: "last in the batch" is only affordable because the filesystem event names the path. Deriving it instead would mean `stat`-ing every changed file, which is [#19](https://github.com/breferrari/vigia/issues/19)'s breach, so the cheap answer and the correct one happened to coincide here rather than by design.

**B3 — The empty state. Ruled 2026-07-31: the proposal stands, with its wording corrected. See §11.1.** Zero changes is not an edge case; it is the state the tool sits in most of the time, and it is the **first** thing anyone sees when they open it beside an agent that has not written yet. A blank pane is indistinguishable from a hang.

Rationale, kept because it is the part a later reader will want to argue with: name it, with the repository, the branch, "no changes", and an explicit statement that it is watching. All four are drawn and two of them are the **header's**, which is why this and the mode word were ruled in one pass rather than separately: the mode word is what makes "and it is watching" sayable in zero extra rows, and the empty state is the screen that makes the mode word worth having.

What was corrected is the wording. The proposal said "no changes" and the shell said `working tree clean`, and both are looser than the diff underneath them: this one compares the working tree against the **index**, so a fully staged worktree has no changes here and plenty for git.

**B4 — Is the file list navigable, and is it a region? Ruled 2026-08-01: the list is a region, and it is not navigable. See §11.1.** ([#66](https://github.com/breferrari/vigia/issues/66).) Both halves, because the question as written only ever asked one of them.

*The smaller half stands unchanged.* **Not navigable**: no selection, no focus, no second mode, for the rationale below, which is kept because it is the part a later reader will want to argue with.

*The larger half is ruled the other way from the proposal.* **The file list is a region of the screen**, pinned above the diff with a rule between them, and it scrolls. What settled it is not taste: three of §5's four glance elements ride a file heading, so a single stream makes the differentiator visible only at scroll position zero, and the picture has drawn two regions since before the question existed. The list is ordered like the stream, costs one `Frame::diff` per **visible** row, tracks the diff on its own and takes `J`/`K` to move; §11.1 carries all of it, including why the diff's scrollbar is row-exact.

*(proposed, and this was the wording that hid the second half)* **Not navigable in v1** — one continuous scroll, list as map. Rationale: selection implies focus, focus implies a second mode, and modes are reviewer-class (§2). The pane is 40 columns beside an agent, not a full-screen client.

> [!IMPORTANT]
> **The question above is the smaller half, and the proposal settles the larger one silently**
>
> Filed 2026-08-01 as [#66](https://github.com/breferrari/vigia/issues/66). *Navigable* is
> downstream of **whether the file list is a region of the screen at all**, and
> "one continuous scroll" answers that second question without asking it.
>
> `assets/preview.svg` draws two regions: a summary block of every changed file,
> pinned above a rule, with one file's diff beneath. The proof is in the picture
> rather than in an intention — `src/engine/watch.rs` is drawn **twice**, once in
> the block and once as the diff heading, which one stream never does. The shell
> draws one stream: [`Row`](crates/vigia/src/view.rs) is a flat enum and
> `Painter::body` is a single loop over `View::rows`, so the sparkline, the
> counters and the heat strip ride a heading **inside** the scroll.
>
> Nothing there is a defect and every element is specified in §5.1, built and
> gated. What has no ruling is the container they were drawn for, and the cost of
> that is not cosmetic: three of §5's four glanceability elements are visible only
> while their own heading is on screen, so **the glance surface disappears exactly
> while the pane is being read**. Also note "list as map" presupposes a list to be
> a map of; there is none, there are headings.
>
> Two constraints belong to §3 rather than to taste, and they are why this is not
> a rendering choice. Ranking a pinned list by diff size needs every changed
> file's diff, which is [#49](https://github.com/breferrari/vigia/issues/49)'s
> argument against a repository-wide total arriving somewhere else; ranking it
> from `History` costs nothing, since that store is already bounded and fed from
> the watch. And a heat strip needs the whole-file line count on `FileDiff`, so a
> pinned row for a file the frame did not diff cannot draw one without breaching
> I4 the same way. Whichever way it is ruled, **§5.1's own rule applies to the
> picture too**: a published artifact answering a question is the answer, so one
> answering it differently from the code is a wrong specification and not a stale
> asset.
>
> **Resolved 2026-08-01 towards the picture**, which is the direction that costs
> something: the region is built rather than the mockup corrected away. What the
> picture got wrong is smaller and is corrected in it — it drew a summary block
> of *every* changed file, and an automatic unbounded changed set cannot have
> one, so the region caps and scrolls and says so with a bar.
>
> **They agree on the layout, which is what B4 asked. They do not agree cell for
> cell, and the remaining gaps are recorded in §5.1 rather than left to be
> rediscovered**: the element split across the two regions, and the order the
> mockup places a row's right-hand side in. Neither is a promise about a region,
> which is what this question was about.

**B5 — Not a git repository, and submodules.** Neither appeared anywhere in this spec. **Half ruled 2026-07-31**, and the halves are separated here rather than one number being retired for the sake of tidiness, because only one of them is decided.

**Not a repository: ruled, and it had already shipped. See §11.1.** Exit non-zero with one line, **before** entering the alternate screen — an error painted inside a TUI that then restores the terminal is an error nobody reads. The proposal stands unchanged; what is worth recording is that `run` has ordered `Worktree::discover` ahead of `Session::enter` since the shell was built, so this sat here marked `(proposed)` for two phases while the code did it. Found by [#40](https://github.com/breferrari/vigia/issues/40) while ruling B3, which is exactly the drift §11's warning box describes and which `take-next`'s pre-flight cannot see, since it compares invariant tokens rather than behaviour.

**Submodules: still open.** *(proposed)* Out of v1, shown as an opaque directory and said so, because recursing into them costs the incremental guarantees in I2a.

**B6 — CLI surface and configuration. Ruled 2026-07-31 with theming, which is what it said it was waiting for, and amended the same day. See §11.1.** The CLI stays at one positional path and gains no flags. Configuration is **a theme file and one environment variable**.

The proposal above said configuration lands *with* theming, and its own rationale is what decided the shape: *a config file with one thing in it invites a second thing*. That argument does not stop at files. A monitor is launched from the shell rc that opens the pane and then left alone for days, so the natural home for a setting made once is the line that already starts it, not a dotfile the process has to find, parse, and decide whether to watch. A file also costs a discovery rule across three platforms and a TOML parser that `SPEC.md` does not name, for a surface with two settings in it.

**Amended within the day, and the correction is worth more than the original.** The first ruling was *two environment variables and no file*, on the argument that a config file with one thing in it invites a second thing. Reading it back against a real reader: a variable has to be re-declared per shell, and the instruction for making that permanent is `$PROFILE` or `.bashrc` or `.zshrc`, so the *mechanism* is portable while the *instruction* is not. A file is set once.

What the original ruling got right is that not everything belongs in one. The split is by **what the setting is about**, not by convenience: a palette is a preference about you, and a colour depth is a fact about the terminal in front of you. The second genuinely differs between two panes on one machine, which is what a variable is for.

What is still left open, and is the intended pressure: there is nowhere to put a setting that is neither of those. §2 makes the reference `btop`, where you never configure anything, and every option added here is a behaviour that owes this section a line.
