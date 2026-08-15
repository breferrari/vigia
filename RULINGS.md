# vigia — Rulings ledger

**What this file is.** `SPEC.md` is the contract: what must hold now, read before code every session. This file is the ledger: how each ruling was reached — the measurements, the corrections that superseded earlier corrections, and the alternatives rejected with their numbers. Split out 2026-08-05, because the contract was drowning in its own history: the I4 entry alone had grown to three generations of correction, and every session paid the whole archaeology to read the one table it needed. Nothing here is superseded BY the move — every word is verbatim from `SPEC.md` §3 as it stood, and each entry's anchor is stable because §10 and the test suite cite these histories by name.

The rule for what lives where: **an active constraint belongs in `SPEC.md`; the evidence trail that earned it belongs here.** A new ruling lands in the spec first and its measurement history moves here when the next reader no longer needs it to apply the rule — not before, because a correction callout over a body that still says the old thing is not a correction.

---

## I1 — the loop already wakes on pointer motion, and the row's measure cannot see it

> [!NOTE]
> **Found 2026-08-15 while ruling [#123](https://github.com/breferrari/vigia/issues/123), and it is a correction to what I1 was understood to claim rather than to what it says**
>
> I1's row reads *"Redraw is **event-driven**, never a fixed timer. No filesystem event and no git index change means no work."* Its budget cell is *"**0 wakeups** while idle"* and its measure cell is *"CPU sampled over a 60s idle window; assert no render calls"* — three separate cells, quoted separately here because splicing them reads as one sentence the table does not contain. What was not written down anywhere is that the process **is woken, and draws, for a class of event the row never mentions**, and has been since the mouse was taken in Phase 2.

**The mechanism.** `crossterm`'s `EnableMouseCapture` is a bundle, not a switch: it writes `\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h`, and `?1003h` is **any-event tracking**, motion whether or not a button is held. `1000` is press and release, `1002` is motion during a drag; `1003` is the one nothing in this program consumes. A pointer crossing the pane therefore delivers an event **per character cell it crosses** — any-event mode inherits button-event mode's cell granularity, so a sub-cell nudge delivers nothing and the rate is bounded by cells rather than by samples. Each one arrives as `Wake::Input`, and `crates/vigia/src/lib.rs` calls `Shell::regions` (a `Copy` field read: no syscall, no allocation), calls `action_for`, gets `None`, and `continue`s. The comment on that arm named the concern before this ruling existed: *"Redrawing for a key release or a mouse move would make the idle cost non-zero for a reason nobody asked for."*

**And that comment describes the arm rather than the frame, which is where the first draft of this entry was wrong.** `continue` leaves the wake, not the batch. Once `for wake in batch.drain(..)` closes, `sample_memory`, `shell.draw` and `record_frame` all run **unconditionally** — the paint's own comment says why, *"Once per batch, not once per wake… only the paint is shared"* — so a batch containing nothing but pointer motion performs a memory read (a `/proc/self/status` read on Linux, a syscall on the other two tier-1 targets), a whole-frame collect and paint, and a frame recorded into the p99 the status bar draws. The claim that a motion wake "performs no render" was read off the arm and is false of the loop.

**So this is a gap in the row's letter, not a mis-measurement of it.** *"No filesystem event and no git index change means no work"*: a pointer nudge is neither, and a paint is work. What survives intact is the **measure**, because a sixty-second *idle* window never has a pointer moving in it, so the gate is silent here by construction and in both directions. The distinction I1 is written on — work done rather than packets received — is still the right one, and it is the same distinction the watch engine already turned on, where an idle tree is not a tree the kernel is silent about and the assertion had to move from events delivered to **changes accepted** (`vigia-core` has no renderer, so `stats.ticks` and `stats.filtered` are its form of the same move). It is only that on this axis the answer comes out the other way. The size is unmeasured and deliberately not guessed at: `ratatui` diffs, so an unchanged buffer writes no bytes to the terminal, and what is real here is work performed rather than output. [#154](https://github.com/breferrari/vigia/issues/154) tracks it.

**What holds §11.2 B10** is `crates/vigia/tests/input.rs::pointer_motion_over_a_laid_out_screen_is_still_no_action`, which asserts that motion over a *laid-out* screen produces no action. The fixture matters: the older inert list hands `action_for` a `Regions::default()`, against which every region-gated arm returns `None` whatever it does in production, so a hover written the way the click arm is written would have left it green. That is an *actions* gate and not a paints gate, and nothing in the suite is the latter, because the honest count today is one paint per motion batch rather than zero.

**It cannot be given back.** The obvious repair is to request `1000`, `1002` and `1006` and leave `1003` off, keeping click and drag and losing only the wake. That is not portably available. On Windows `EnableMouseCapture::is_ansi_code_supported()` is `false` and `execute!` routes the bundle through the console API, writing **zero bytes**, so hand-written DEC modes would work on Unix and do nothing on Windows. The cost of the repair is the mouse on a tier-1 target, or a second mechanism inside I8's takeover, against a wake that is a channel receive and two pure calls. It is recorded rather than repaired.

**What it changed.** §5.3 had priced a hover highlight as *"a wake class I1 currently never pays"*, and §11.2 B10 was opened to weigh that trade. The trade did not exist: the wake is sunk either way, so B10 was decided on what hover would *show* instead. `crates/vigia/src/terminal.rs::every_command_is_the_escape_sequence_it_is_named_for` asserts the bundle byte for byte, so if `?1003h` ever stops being requested, this entry stops being true loudly rather than quietly.

---

## I4 — narrowed 2026-08-01: counting a height is not summing content

> [!NOTE]
> **I4 was narrowed on 2026-08-01, and the measurement is why**
>
> It read *"first paint is independent of total diff size"*, full stop, and [#49](https://github.com/breferrari/vigia/issues/49) had already refused a repository-wide `+`/`-` total on the strength of it. That ruling stands for a **sum over content**. It was then applied a second time, to the diff's **height**, and that was wrong: the two are not the same quantity, and the difference is measurable.
>
> A height is hunk boundaries and line counts. A [`FileDiff`] is those *plus* an owned `String` for every drawn line, so totalling a worktree through one allocates once per changed line and once per line of context. Measured on the reference machine, release, over a hundred files of five hundred rewritten lines: totalling through full diffs is **442.71ms**, and counting the same answer is **8.76ms**. `git diff --numstat` over the identical shape is 46ms, so the counting path is not merely cheaper than our own mistake, it is in the range the tool everyone compares against occupies.
>
> What the narrowing costs, stated rather than buried, and **superseded on 2026-08-04 by the note below**: a tick then read every changed file's bytes, where before it read only the window's. It is **once per tick and not once per frame** — the count *was* cached until the next `Frame::advance` and dropped there, so scrolling paid nothing and a redraw still read zero. The note below is where that dropping became the defect. Diffs, highlighting and every allocation still follow the window, which is the half of I4 that was doing the real work.
>
> Why it was worth it: a scrollbar that cannot say where the end is says nothing. The version that avoided this walk had to approximate the whole from the current file's height, and it vanished on a short file, ballooned on a long one, and never reached the bottom. Reported from use rather than caught by a gate, which is the fourth time. `what_a_row_exact_scrollbar_would_cost` is the diagnostic that holds the numbers above, so the next person re-runs them instead of re-arguing this.

## I4 — the walk became incremental 2026-08-04

> [!NOTE]
> **The walk is incremental as of 2026-08-04, and it was re-reading the whole worktree every tick until then**
>
> "Counted for every changed file once per tick" is what the narrowing above admits, and it is not what shipped. `Frame::advance` dropped the span cache whole, on the reasoning that a span is derived from content and had no freshness check of its own, so every changed file the reader had **not** scrolled to was read from disk again on **every tick**, for as long as the process ran. Over the hundred-file fixture that is 94 files and 3.7 MiB a tick, **16.98ms p50 and 18.36ms p99 against I9's 16ms**, in the state a reader is in one second after launch.
>
> **The fix is I2a's own rule applied to the span**: give it the evidence a diff already carries (`Taken`: kind, index blob, and a settled fingerprint) and revalidate it with the same `reusable` function rather than a second copy of the rule. A stat replaces a read. Measured on the reference machine, release: a hundred stats is **1.29ms** against **12.90ms** to measure a hundred files, and the ratio runs 6.8x at 2000 files to 10.0x at 100. The tick above becomes **9.40ms p50, 10.67ms p99**, with zero files measured across a hundred ticks.
>
> **Three costs, all stated rather than buried.** First paint pays one extra `stat` per changed file, since a span is only carryable if it was fingerprinted when it was taken: **13.4-13.8ms before, 14.6-14.9ms after**, against I7's 50ms and I4's 100ms. The order of the sources matters more than it looks: a file whose diff is already in hand needs no evidence at all, and asking for one first took `the_frame_budget_holds_through_a_bulk_rewrite` from 8.27ms p50 to 11.12ms and from passing four local runs of four to two. `a_height_taken_from_a_diff_in_hand_costs_no_stat` is the structural gate that keeps that order.
>
> And the walk is incremental **outside** the settle margin and not inside it. A bulk rewrite of files nothing has drawn leaves every carried span unsettled at once, so none can be proved and the walk re-measures the whole changed set for the two seconds the margin lasts. It is also the corner where a `.gitattributes` is most likely to arrive, and a fourth staleness rule covers that: see below. That is the pre-#101 cost, paid for a bounded window rather than forever, plus one `stat` per file for the fingerprint that will make the span carryable again once the margin passes. Measured over a hundred undrawn files rewritten at once, over eight runs on a quiet machine: **p50 stable at 13.08-14.33ms**, and a **p99 ranging 15.49ms to 44.70ms**.
>
> **That is reported and not asserted, and the refusal is the ruling.** Three instruments were tried: rewriting before every timed frame, discarding one frame after each rewrite, then discarding twelve and partitioning frames by what they actually re-measured. None separated 1.7 MiB of fixture write-back from the subject, and a stable p50 under a tail that moves 3x is the signature §7 names rather than a number to gate on. `what_a_bulk_rewrite_of_undrawn_files_costs` therefore prints the distribution and asserts only what is exact: that the corner was entered, that the worktree stayed undrawn, and how many files a frame there re-measures. The syscall count is printed beside it. The same corner **is** gated, as a count, by `a_tick_inside_the_settle_margin_stats_each_file_once` in `reads.rs`, which is the tier that works on a shared machine. A gate that can only say "no regression" on a quiet disk has not been tested, which is the rule the soak's drift gate already follows one invariant over. An agent running a formatter is exactly this workload, so it gets its own gate rather than a note: `what_a_bulk_rewrite_of_undrawn_files_costs`.
>
> The **lazy** fingerprint is what keeps that corner at one `stat` per file rather than two. `reusable` refuses an unsettled observation before it asks for a fresh print, so the pre-check costs nothing there; taken eagerly it doubled the syscalls in exactly the window that can least afford them. Held by a count rather than by that p99, which has more headroom than the reorder costs: `reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`.
>
> **The three options #101 listed were all rejected, and the reason is the same for all three: they were written against 93ms and the number is 12ms.** They are recorded here rather than in the issue because the next person to find this walk expensive will reach for them again. The number they were written against is 93.69ms; the walk measures **12.90ms** cold and **1.29ms** once incremental.
>
> *Parallelise the walk.* It buys the read back and nothing else, so against a 10x reduction already taken it is buying a second time. It costs a thread pool or a hand-rolled scope on **every tick**, and I3 is a claim about a process left open for days: a monitor that wakes several cores each time an agent saves a file is a different product from the one §2 describes, whatever its p99 says. Rejected on the product class first and the arithmetic second.
>
> *Stream the first paint.* It addresses a first-frame cost, and the first frame is 14.9ms against I4's 100ms; there is nothing left here to stream away. It also needs a wake on completion, which is a question about what I1 forbids, and reopening that to buy nothing is the wrong trade. It stays where §10 already keeps it, with the non-streaming walk in [#48](https://github.com/breferrari/vigia/issues/48).
>
> *Approximate the total.* Refused outright, and #101 listed it to be refused explicitly rather than forgotten. It is the design the narrowing above replaced: a bar scaled from the current file's height vanished on a short file, ballooned on a long one and never reached the bottom. It is the only one of the three that costs a reader something, which makes it the one to keep saying no to.
>
> **What was taken instead is none of the three, and that is the finding.** The walk did not need to be faster; it needed to stop repeating itself, which is what I2a says about diffs and what nothing had yet said about heights. A cost measured once and then re-derived every tick reads as an expensive computation and is a missing cache.
>
> **What this does not fix**, so the boundary is a decision and not an oversight: the height of a file whose diff *is* in hand is still taken by presence rather than by proof, which is [#84](https://github.com/breferrari/vigia/issues/84). That branch is untouched, including the 20.71ms #84 records for proving it. [#101](https://github.com/breferrari/vigia/issues/101).

## I2 — why it is two numbers

> [!NOTE]
> **Why I2 is two numbers**
>
> It was written as one, reading "re-highlighting is incremental", and that conflated two invariants with **different dependencies and different phases**. Incremental re-*diffing* needs only `gix` and is Phase 1. Incremental re-*highlighting* needs `syntect`, which Phase 1 does not include, so Phase 1 could not close while one number meant both. Split deliberately rather than absorbed silently, per the drift rule. Measurement that forced it: re-diffing every changed file costs **18.58ms p99** on a 100k-line diff against **3.27ms** for a single file, so I2a is load bearing rather than an optimisation. Issues [#2](https://github.com/breferrari/vigia/issues/2) and [#4](https://github.com/breferrari/vigia/issues/4).

## I8 — why it no longer says SIGINT

> [!NOTE]
> **Why I8 no longer says `SIGINT`**
>
> It read "restored exactly on exit — including `SIGINT` and panic", and the `SIGINT` half encoded an assumption the shell falsified. **Raw mode removes the signal.** `enable_raw_mode` clears `ISIG` on Unix and `ENABLE_PROCESSED_INPUT` on Windows, so Ctrl-C is never translated: it arrives as an ordinary key event and is handled by the key map, which is why `Session` never needed a handler and why no test could ever have been written for the clause as worded.
>
> What that leaves genuinely uncovered is a signal nobody at this keyboard sent: `kill -INT` or `-TERM` from another pane, which runs neither `Drop` nor the panic hook. `std` has no signal API, so closing it is a **dependency decision** rather than an implementation detail, and the single-platform version of it (`signal-hook` on Unix, with `SetConsoleCtrlHandler` needed separately on Windows) ships a guarantee whose meaning differs by tier-1 platform. That is the same trade [#16](https://github.com/breferrari/vigia/issues/16) already rejected as worse than one stated uniformly. Tracked as [#24](https://github.com/breferrari/vigia/issues/24) rather than assumed away, and the invariant above now states its own limit instead of overselling it.

## I3 — why the scheduled soak is not twenty-four hours long

> [!NOTE]
> **Why the scheduled soak is not twenty-four hours long**
>
> The budget is a claim about a day and it stays one. What changed is the proof column, because the number in it was unrunnable: a **GitHub-hosted job is terminated at six hours** of execution time, where a self-hosted one gets five days. Verified against GitHub's published limits, 2026-07-31.
>
> So the scheduled run takes the longest window that fits under the cap, and the full 24h is reached by `workflow_dispatch`, which carries the duration and the runner label, on a machine with no cap. The shape of the measurement does not change with the window: the sample **count** is fixed, so the cadence is exactly the five minutes above at 24h and proportionally tighter below it, and the statistic is computed identically either way.
>
> What does not scale down is the warmup. Every process climbs to an allocator plateau before it is flat, so a window short enough to be all warmup can only measure warmup, and the gate refuses to assert there rather than reporting a number it cannot stand behind. §7 carries that as a rule.
