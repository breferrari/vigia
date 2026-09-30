# vigia — Rulings ledger

**What this file is.** `SPEC.md` is the contract: what must hold now, read before code every session. This file is the ledger. It records how each ruling was reached: the measurements, the corrections that replaced earlier corrections, and the alternatives rejected with their numbers. It was split out 2026-08-05, because the contract was drowning in its own history. The I4 entry alone had grown to three generations of correction, and every session read the whole history to use one table. The move superseded nothing here. At the split, every word was moved verbatim from `SPEC.md` §3 as it stood. Each entry's anchor is stable, because §10 and the test suite cite these histories by name.

The rule for what lives where: **an active constraint belongs in `SPEC.md`; the evidence trail that earned it belongs here.** A new ruling lands in the spec first. Its measurement history moves here when the next reader no longer needs it to apply the rule, and not before. A correction callout over a body that still says the old thing is not a correction.

---

## I1 — the loop already wakes on pointer motion, and the row's measure cannot see it

> [!NOTE]
> **Found 2026-08-15 while ruling [#123](https://github.com/breferrari/vigia/issues/123). It corrects what I1 was understood to claim, not what it says**
>
> I1's row reads *"Redraw is **event-driven**, never a fixed timer. No filesystem event and no git index change means no work."* Its budget cell is *"**0 wakeups** while idle"* and its measure cell is *"CPU sampled over a 60s idle window; assert no render calls"*. Nothing written down said that the process **is woken, and draws, for a class of event the row never mentions**. It has done so since the mouse was taken in Phase 2.

**The mechanism.** `crossterm`'s `EnableMouseCapture` is a bundle, not a switch. It writes `\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h`. `1000` is press and release, `1002` is motion during a drag, and `?1003h` is **any-event tracking**: motion whether or not a button is held. Nothing in this program consumes `1003`. A pointer that crosses the pane delivers one event **per character cell it crosses**. Any-event mode keeps the cell granularity of button-event mode, so a sub-cell nudge delivers nothing, and cells bound the rate. Each event arrives as `Wake::Input`. `crates/vigia/src/lib.rs` then calls `Shell::regions` (a `Copy` field read: no syscall, no allocation), calls `action_for`, gets `None`, and does `continue`. The comment on that arm named the concern before this ruling existed: *"Redrawing for a key release or a mouse move would make the idle cost non-zero for a reason nobody asked for."*

**That comment describes the arm, not the frame, and the first draft of this entry was wrong about it.** `continue` leaves the wake, not the batch. When `for wake in batch.drain(..)` closes, `sample_memory`, `shell.draw` and `record_frame` all run **unconditionally**. The paint's own comment says why: *"Once per batch, not once per wake… only the paint is shared"*. So a batch of pure pointer motion does a memory read (a `/proc/self/status` read on Linux, a syscall on the other two tier-1 targets), a whole-frame collect and paint, and records a frame into the p99 that the status bar draws. The claim that a motion wake "performs no render" was read off the arm, and it is false of the loop.

**So this is a gap in the row's letter, not a mis-measurement.** *"No filesystem event and no git index change means no work"*: a pointer nudge is neither, and a paint is work. The **measure** survives intact. A sixty-second *idle* window never has a pointer moving in it, so the gate is silent here by construction, in both directions. I1's distinction, work done and not packets received, is still the right one. The watch engine turned on the same distinction: an idle tree is not a tree the kernel is silent about, and the assertion moved from events delivered to **changes accepted** (`vigia-core` has no renderer, so `stats.ticks` and `stats.filtered` are its form of that move). On this axis the answer comes out the other way. The size is unmeasured and deliberately not guessed. `ratatui` diffs, so an unchanged buffer writes no bytes to the terminal, and the real cost is work performed, not output. [#154](https://github.com/breferrari/vigia/issues/154) tracks it.

**What held §11.2 B10, and still holds it after the reversal**, is `crates/vigia/tests/input.rs::pointer_motion_over_a_laid_out_screen_is_still_no_action`. It asserts that motion over a *laid-out* screen produces no action. The fixture matters. The older list hands `action_for` a `Regions::default()`, and against that every region-gated arm returns `None`, whatever it does in production. A hover written like the click arm would have left that test green. It is an *actions* gate, not a paints gate. The suite has no paints gate, because the true count today is one paint per motion batch, not zero. **It survives B10's reversal because hover is view state, not a keymap entry.** The assertion written to catch a hover *being built* is the one that keeps hover *out of the keymap*. A gate that forbids a mechanism outlives the ruling that motivated it. A gate that forbids an outcome does not.

**It cannot be given back.** The obvious repair is to request `1000`, `1002` and `1006` and leave `1003` off. That keeps click and drag and loses only the wake. It is not portably available. On Windows, `EnableMouseCapture::is_ansi_code_supported()` is `false`, and `execute!` routes the bundle through the console API, which writes **zero bytes**. So hand-written DEC modes work on Unix and do nothing on Windows. The repair costs the mouse on a tier-1 target, or a second mechanism inside I8's takeover. Both are set against a wake that is a channel receive and two pure calls. It is recorded, not repaired.

**What it changed.** §5.3 had priced a hover highlight as *"a wake class I1 currently never pays"*, and §11.2 B10 was opened to weigh that trade. The trade did not exist. The wake is sunk either way, so B10 was decided on what hover would *show* instead. `crates/vigia/src/terminal.rs::every_command_is_the_escape_sequence_it_is_named_for` asserts the bundle byte for byte. If `?1003h` ever stops being requested, this entry stops being true loudly, not quietly.

> [!NOTE]
> **B10 was reversed 2026-08-16 and hover is adopted. Nothing measured in this entry changes; one clause of it is superseded.** Everything here is about a cost that is sunk in *both* directions, so it reads the same whichever way the ruling goes: adopting hover adds no wake, no paint and no frame. The superseded clause is the last one of the paragraph above: *"so B10 was decided on what hover would show instead"*. It was decided on that, and then the second reason turned out false too, for reasons about other people's software, not about I1. §11.2 B10 carries the reversal, and the **B10** section at the end of this file carries its trail.

---

## I1 — a warm that finishes is a fourth sender, and the row does not reach it

Ruled 2026-08-21 with [#129](https://github.com/breferrari/vigia/issues/129). The frame path stopped parsing under grammars not yet compiled, so a hunk can be on screen in plain text with its colour still owed. Something has to tell a loop blocked on `recv` that the colour arrived.

**I1, quoted before it is cited.** The row: *"Redraw is **event-driven**, never a timer that runs unbidden. No filesystem event and no git index change means no work."* Budget: *"**0 wakeups** while idle."* Measure: *"CPU sampled over a 60s idle window; assert no render calls"*, plus `nothing_held_means_no_timer_at_all` on the untimed wait.

**It does not reach a warm.** A warm exists only because a file was written, or because a diff was already on screen when the process opened. On a tree nobody touches, nothing is spawned and nothing is sent, so *no filesystem event means no work* holds literally. A warm is bounded by the number of distinct grammars a session meets, not by time, so it cannot repeat. A warm also leaves the wait untimed, so the structural half of the measure is untouched: `Shell::patience` still returns `None` with nothing held. A warm that arrives is a `recv` that returns, not a `recv_timeout` that expires.

**The licence sentence about clocks was deliberately not stretched to cover it.** That sentence is about a clock a gesture holds open, and this is not a clock at all. Stretching it would repeat the mistake of [#166](https://github.com/breferrari/vigia/issues/166) and §11.2 B10 from the other direction: citing I1 at a case its budget would never have measured.

**Structurally, the third sender becomes a fourth.** `crates/vigia/src/lib.rs` already describes the signal handler as *"a **third wake source on the same channel** rather than a new mechanism"*. This is the same move. `Highlighter::warm_ahead` takes a callback. The shell hands it one that sends `Wake::Warmed`, and the arm for it does **nothing**, because the paint after the batch is the whole response.

**The bound is `Shell::request_warm`: one warm in flight.** A demand raised while a warm is running is not queued. The running warm ends with a wake, that wake paints, that paint raises the demand again if it still holds, and the next warm starts. The loop terminates because the warmer marks every grammar it *had a run at*, including one whose file vanished before it opened it. Without that, a hunk drawn from a diff whose file is gone would be demanded on every frame forever: a livelock with a wake attached. `a_path_that_vanished_does_not_leave_the_frame_asking` covers that case, and it was watched failing.

---

## I7 — the residual table that made #51 decline more than it had to

Corrected 2026-08-21 with [#129](https://github.com/breferrari/vigia/issues/129).

**What #51 recorded.** Two rejections. The first was a per-grammar warmth predicate that the frame path could act on, *"because compilation is per pattern: warming on one Rust file leaves a sibling paying 41.41ms, Markdown 95.04ms, HTML 201.20ms"*. The second was per-hunk deferral, *"the only exact fix"*. It was rejected because it would add a colour lag to every scroll into new territory, thousands of times a session, to remove a cost paid once per grammar.

**The second reason still holds, and #129 did not reopen it.** #129 defers once per **grammar** per session, not once per hunk. A scroll into new territory under a grammar already met parses inline, as it does today.

**The first reason rested on a number measured at the wrong scale.** Those residuals are **whole-file** parses. Each carries a large parse beside the compile it was meant to isolate, and a frame parses one screenful. Re-measured at frame scale, with twenty-four lines, a release build and a fresh `SyntaxSet` per case:

| | cold | after the warmer read one real 64KB sibling | floor |
|---|---|---|---|
| `.rs` | 123.98ms | **2.40ms** | 2.40ms |
| `.md` | 694.75ms | 89.77ms | 90.65ms |
| `.toml` | 15.26ms | 0.43ms | 0.40ms |

The middle column **is** the floor. One real sibling pays the compile in full. The old table reported the cost of parsing another whole file.

**One half of #51's finding survives, and it decides the implementation.** A *small* sibling is not enough. Over a 2.5KB hand-written sample the residual is real: `.js` 80.49ms above floor, `.html` 40.10ms, `.cpp` 37.55ms. So the warmer reads `WARM_BYTES` of a real file. A fixture would not be enough.

**The claim the frame path acts on is not the one #51 rejected.** *This grammar is warm* is unavailable at any price, and nothing asserts it. *Nothing has ever parsed under this grammar* is exact. `syntect` holds every pattern in its own `OnceCell` and offers no way to fill one except a parse. So the two places in `vigia-core` that build a `ParseState` are the whole population. This was checked against the source: `SyntaxReference::contexts` is private, `ContextId`'s fields are `pub(crate)`, and `Regex::try_compile` compiles a throwaway instead of filling the set's own cell. There is no eager path.

**The cliff is flat in content size.** That separates a compile from a parse, and nothing recorded it before. A 594-byte Markdown screenful costs **631.46ms cold and 0.97ms warm**, a 650x penalty on half a kilobyte.

**The warm stops at three grammars.** The measurement over this repository warmed one grammar at a time and read RSS after each. The baseline was **6.73 MiB**, and ten grammars later it was **64.73 MiB**. Rust added 12.43 MiB and Markdown 19 to 35 MiB. I3's budget is drift, not a plateau, so this is a bad trade, not a breach. The languages a repository *leads* with are different. The agent is near-certain to write the language the repository is mostly made of, so those megabytes are spent within seconds either way, and the sweep only spends them earlier. The tail is the speculative part, and the cap removes it.

**"Leads with" counts a grammar, and the index cannot see grammars.** A tally of `.git/index` can only key on the extension, because §6 keeps `syntect` out of `worktree.rs`. That proxy is sound for one path and unsound for the *selection*. A repository whose YAML is split evenly across `.yml` and `.yaml` counts each spelling separately, at the one moment the counts decide which three grammars compile. So it loses to a smaller language with one extension. The first implementation ranked and truncated in `worktree.rs`. The altitude review found that, with no failing test behind it. The tally now comes back complete and unranked, and the merge happens in `highlight.rs`, where a `Scope` is available. `a_language_spelled_two_ways_is_counted_once` is the gate, watched failing against the per-extension ranking.

**A second finding turned up beside this one, and it is a different problem.** A 16.8KB Markdown screenful costs **117.00ms with every pattern already compiled**. That is the opposite shape to the cliff above: it appeared to track bytes on screen, not stay flat, and it survived every warm. It is a long-line parse cost. It belongs to I2b and I4, not I7, and it is [#261](https://github.com/breferrari/vigia/issues/261). **Closed 2026-08-22, and the shape recorded here is falsified.** Cost does *not* track bytes on screen. 24 empty lines inside a fence cost 25.3ms for 28 bytes of content. The same 10,288 bytes reflowed from 24 long lines to 138 short ones cost 5.47ms against 5.85ms. What tracked was the number of **block starts**. Markdown's block-start lookahead ran an exponential table-row test on lines that can never be table rows. The I9 entries below answer this finding, and they supersede the mechanism guessed at here.

---

## I1 — a held mouse button is not an event, so a gesture-bounded clock is licensed

> [!IMPORTANT]
> **Reversed 2026-08-15, the same day it was written. The entry is kept whole because its measurement is still why the feature is hard.** The first ruling refused hold-to-repeat. It was overruled as a product decision: a scrollbar button that does not repeat while held is not a scrollbar button, and every desktop toolkit has offered repeat since the 1980s. Everything below about the protocol still holds, and any implementation has to work around it. The conclusion changed, and `SPEC.md` §11.1 carries it: **the clock is allowed because the reader's finger bounds it.**
>
> The reversal turns on the distinction the correction below had already found. I1's budget is *0 wakeups while **idle***, and a held mouse button is not idle. Every other timer this spec refuses would run while nothing is happening. This one cannot start on its own and cannot outlive the release. `Held::wait` returns `None` with nothing held, so the loop's receive is untimed exactly as before. **"Cannot outlive the release" was not true when it was written, and [#186](https://github.com/breferrari/vigia/issues/186) found out why.** A release is not the only way a gesture ends. If the window loses focus while a button is down, no `Up` arrives. `Held::ends` had no arm for `Event::FocusLost`, so the repeat kept stepping and repainting a pane nobody was looking at. The Windows console always delivered that event, so the hole was open there from the day the repeat shipped. On Unix it became reachable only when the takeover started asking for focus reporting. The condition is unchanged. The list of what ends a gesture was short by one. I1's row now carries that qualifier, so it does not have to be re-derived.
>
> The entry is kept, not rewritten, for two reasons. Anyone who wonders why this feature took a ruling needs the protocol facts. And the first draft cited a budget that would never have caught the thing it refused.
>
> **Corrected again 2026-08-16, on the shape of the amendment, not on the ruling.** The reversal was first written into I1 as an *exception*: *"the one clock this program owns runs only between a press on a scrollbar's step button and its release"*. That is the instance, not the rule, and an enumerated exception failed in two ways at once. First, it **blocks the next case even where the argument is identical**, and two were already in the tracker. A selection dragged past the edge of its region wants to scroll ([#177](https://github.com/breferrari/vigia/issues/177)), and a press held on the **track** is the page-repeat every desktop scrollbar has. Neither is a step button, and both are clocks bounded by a gesture. Under the exception, each would need its own reversal. Second, it **disagreed with the code it was written for**. `Held` repeats whatever action it is armed with, and `Action::repeated` is an exhaustive match so that a later held control inherits the mechanism. The spec licensed one control while the code offered a facility. I1 now states three conditions: the clock must not start on its own, it must not outlive the gesture that armed it, and the idle path must be untimed by construction. The step buttons are an instance of them.
>
> **When a ruling is reversed under pressure, the amendment tends to be as narrow as the case that forced it**, because that case is the one in front of you. At that moment, ask what the argument actually proved, not what it was invoked for. Here the argument proved something about *bounded* clocks and was written down as something about *scrollbars*.


> [!NOTE]
> **Measured 2026-08-15 while building [#166](https://github.com/breferrari/vigia/issues/166).** The request was for ordinary step buttons on the scrollbar, and the first question a button raises is whether holding it repeats. The protocol cannot report it: there is no event to hang the repeat on. This is not this program's design.

**The mechanism, read from `crossterm`'s source.** `MouseEventKind` has exactly eight variants: `Down(button)`, `Up(button)`, `Drag(button)`, `Moved`, and the four `Scroll*`. No variant means *the button is still down*, and no lower layer carries one. The entry above records that the bundle sets `?1003h`, which is the most the terminal reports, and `?1003h` reports **motion**. A finger resting on a button, with the pointer still, produces one `Down` and then nothing until the pointer moves or the button is released.

**So repeat has to come from a clock, and the reversal above allows building one.** There are only two implementations: a timer that fires while a flag says a button is held, or a loop that re-reads the button's state on a schedule. Both are a fixed cadence.

**The obvious citation from I1 is the wrong one.** The first draft of this entry said the clock is "what I1 exists to refuse". I1's *budget* does not refuse it at all. The budget is **0 wakeups while idle**, measured over a sixty-second idle window. A timer that runs only while a button is held is not idle, and nobody holds a mouse button through a sixty-second window, so the gate stays green whatever the timer does. The entry above records the same structural blindness for pointer motion, where the measure is *"silent here by construction"*. A refusal resting on that measure is checkable, and wrong. What does reach a held-button clock is **I1's first sentence**, which is about mechanism, not idleness: *"Redraw is event-driven, never a fixed timer."* A repeat clock is a fixed timer that produces redraws, whoever holds what, and no measure has to catch it for the sentence to apply.

**The phrase is still correct everywhere else it appears.** *"The timer I1 forbids"* is this repo's own shorthand. The pulse decay, the header's idle word, the memory readout, the poll loop `lib.rs` rejects, and §10's highlight tail are all clocks that would run **while nothing is happening**. That is exactly the state the budget measures, and exactly where it bites. A held-button repeat is the one clock bounded by an active gesture, so it slips under the measure and still fails the sentence.

§5.3 refused the same thing for animation (*"snap, never ease"*). The reader **reversed that 2026-08-27** on this entry's own ground: a clock that cannot start on its own never touches I1's idle measure.

**The first ruling made one step per click the affordance, because every repeat needs a clock.** That half was right: no protocol feature gives repeat for free, so there is nothing to look for. The ruling was wrong to treat "it needs a clock" as the end of the argument. The next question was *which* clock, and whether I1's budget was written against a clock bounded by a press. It was not. See the callout at the top of this entry.

**The reversal did not make the button a travel affordance.** The wheel, `j`/`k`, `d`/`u`, `n`/`p`, the digits, `g`, `G` and a draggable thumb are for travel. The button is for the single step none of them gives a pointer. So the repeat holds a constant rate and does not accelerate: acceleration serves travel and costs precision, and this control is for precision.

**The same finding decides the drag.** `input.rs` is a pure function of an event and a layout, with no state between calls. So it cannot know that a drag *began* on a button and not on the thumb. A `Drag` over a button row then has two candidate meanings, and both are wrong. Stepping would walk the view a row for every twitch of a press. Clamping to the end would jump the view there. So a drag over a button does nothing. That costs nothing real, because the last track row already reaches the last window. Keeping it stateless keeps the whole map a table test, and it is why the difference between `Down` and `Drag` is ruled, not overlooked.

**`crates/vigia/tests/input.rs::a_drag_onto_a_step_button_is_inert` holds it.** It asserts that the same cell answers a press and refuses a drag, so the two gestures are told apart and the row is not dead. The drag ruling survived the reversal unchanged, because it never rested on the clock. It rests on `input.rs` having no state, and the repeat's state lives in the loop, not in that module.

**A different gate holds the reversal, and it is the one to break first if anyone revisits this.** `nothing_held_means_no_timer_at_all` asserts that `Held::wait(None, _)` is `None`, which keeps the loop's receive untimed on an idle monitor. Everything else about the repeat is a decision of feel. That gate is the invariant. A version that returned some large timeout instead would look harmless, pass every other gate in the file, and quietly put this program on a poll loop.

---

## I1 — the window ages, and the clock that ages it stops when the window empties

**Ruled 2026-08-22 for [#243](https://github.com/breferrari/vigia/issues/243). #243 filed itself as an I1 amendment, and it is one.** The request came from use: *the graph should age*. `History::roll` had a single caller, the tick path, so a quiet worktree left the window frozen.

**The problem is correctness, not staleness.** The window's axis is time. A frozen window keeps its newest sample at the right edge, so a burst from ninety seconds ago draws as *just now*. `Recency` froze with it, so a file that went quiet kept its pulse. The monitor was incorrect with zero interaction, which breaks I5. Two product-class claims pull against each other here. Neither is given up for convenience.

**This clock fits the licence's purpose. It does not widen it.** The entry above records why the first clock was admitted: *every other timer this spec refuses would run while nothing is happening*. This clock cannot. It runs only while a burst is still decaying through the window. The window empties `HISTORY_WINDOW` after the last write, so a monitor left open overnight has an empty window and an untimed wait. This was measured, not assumed. At `t+119s` the window holds one live sample and the path is tracked. At `t+120s` it holds none and the path is gone, because `roll` clears every track once the whole window turns over.

**The second condition is restated, not dropped.** It read *"it may not outlive the gesture that armed it"*. `SCROLL_LINGER` is `now + 220ms`, so the direction arrows' clock always outlived the gesture that armed it, and nobody called that an amendment. The condition always meant a *bounded* outliving. #243 changes two things. A change in the worktree can now arm a clock, as well as a reader's gesture. The bound is the window, not a release.

**Three measurements.** An ageing wake costs **165µs**, measured in release at the 256-path cap with a sample boundary crossed on every round. I9's budget is 16ms, so this is 1% of one frame budget. An ordinary tick on the same fixture costs **529µs**. The difference is the status walk, which an ageing wake skips because it is not a filesystem event. The drain is bounded at `HISTORY_SAMPLES`, so ageing one burst to nothing costs about **19.8ms of CPU spread over two minutes**.

**These figures correct the ones this ruling was first written with. The correction is why the gate over them was rebuilt.** The first figures were 89.9µs against 458µs, on a twenty-path fixture whose rounds all landed inside one second. No sample boundary was crossed, so neither arm paid the projection a real ageing wake causes. The store also held a twelfth of the paths the walk is priced at. The conclusion did not change. The margin narrowed from a fifth to a third.

**Declined on a number: a period derived from the drawn cell.** A band cell at 109 columns covers 1.1 seconds and a sparkline bucket covers five, so a coarser tick would skip wakes that change no pixel. It is refused because it would cap a cost measured at 165µs, and `CLAUDE.md` holds a cap to the same bar as a refusal. The period is `HISTORY_SAMPLE`, the finest interval at which any drawn cell can change.

**Declined on a reason: running the clock only while the masthead is up.** #243 proposed it, on the ground that `m` is a gesture. The sparklines and the pulse are drawn whether or not the masthead is, and they read the same window. A clock gated on the band would leave the two elements disagreeing about the time, which [#234](https://github.com/breferrari/vigia/issues/234) exists to forbid. One store and one roll keep them coherent by construction.

**What it cost the pulse.** *Reversed 2026-08-26 by [#345](https://github.com/breferrari/vigia/issues/345). The mark is now `History::newest` and has no decay of its own. This paragraph is kept because the reader did notice the cost. Everything below is true of the row's* ink*, and was true of the `●` for four days.* `Recency::Pulse` means a path was named by the newest burst and has ink in the newest sample. With the window ageing on its own, the mark expired at the next sample boundary, instead of surviving until something else was written. Its life was uniform on `(0, HISTORY_SAMPLE]`: half a second on average, and arbitrarily short for a write landing just before a boundary. This was preferred to both alternatives below. **Amended 2026-08-25 by [#313](https://github.com/breferrari/vigia/issues/313). The "arbitrarily short" clause was wrong, not the shape.** It was reported from a live pane: the dot no longer showed up. Measured across the grid, a write at +0ms into a sample pulsed for 1s, at +500ms for 500ms, at **+990ms for 10ms** and at **+999ms for 5ms**. An agent saving files continuously lands anywhere in a sample, so the mark was a coin toss and the reader was losing it. The ruling had recorded *"arbitrarily short"* as a weighed cost. In fact the element failed to exist about half the time. **The mark now survives the newest `PULSE_SAMPLES` samples, not the newest one.** Its life is `[HISTORY_SAMPLE, PULSE_SAMPLES x HISTORY_SAMPLE]`, closed at both ends, so the *worst* case is now what the best case was. **The refusal of a duration is unchanged, and it is still right.** A sample count needs no second clock, no wall time beside the grid, and no wake that was not already going to happen. The roll that ages the window is still the only thing that retires the mark. A count moved, not a mechanism. The first alternative, not expiring at all, is the freeze itself: it drew a two-minute-old file at full brightness beside a nearly drained band. The second, a duration of the mark's own, means a second clock per track on wall time beside an element on the sample grid. The two would disagree about how long ago *now* was, which #234 forbids. The mark means *there is ink in the newest cells*, and since #313 there are `PULSE_SAMPLES` of them, at a second each. **The frame a burst causes always keeps its pulse**, by construction. The loop reads one instant per turn, for both the tick and the roll, so a boundary cannot fall between a write and the paint it triggered. That was a real defect for one commit. Two review agents on disjoint remits found it, and the parameter that closed it is the gate.

**The gate** is `an_empty_window_and_nothing_held_means_no_timer_at_all`, written the way `nothing_held_means_no_timer_at_all` is. It asserts the *value* given to the loop's wait, not a behaviour observed around it. A version that returned a large timeout for an empty window would look harmless, and would put an idle monitor on a poll loop.

---

## I4 — narrowed 2026-08-01: counting a height is not summing content

> [!NOTE]
> **I4 was narrowed on 2026-08-01, and the measurement is why**
>
> It read *"first paint is independent of total diff size"*, full stop. [#49](https://github.com/breferrari/vigia/issues/49) had already refused a repository-wide `+`/`-` total on that basis. That refusal stands for a **sum over content**. It was then applied a second time, to the diff's **height**, and that was wrong. The two are different quantities, and the difference is measurable.
>
> A height is hunk boundaries and line counts. A [`FileDiff`] is those *plus* an owned `String` for every drawn line. So totalling a worktree through one allocates once per changed line and once per line of context. On the reference machine, in release, over a hundred files of five hundred rewritten lines: totalling through full diffs takes **442.71ms**, and counting the same answer takes **8.76ms**. `git diff --numstat` over the same shape takes 46ms. So the counting path is cheaper than our own mistake, and it is in the range of the tool everyone compares against.
>
> The narrowing had a cost. **The note below supersedes this paragraph as of 2026-08-04.** A tick then read every changed file's bytes, where before it read only the window's. The read happened **once per tick and not once per frame**. The count *was* cached until the next `Frame::advance` and dropped there, so scrolling paid nothing and a redraw still read zero. The note below is where that dropping became the defect. Diffs, highlighting and every allocation still follow the window. That half of I4 did the real work.
>
> Why it was worth it: a scrollbar that cannot say where the end is says nothing. The version that avoided this walk approximated the whole from the current file's height. It vanished on a short file, ballooned on a long one, and never reached the bottom. It was reported from use, and no gate caught it, for the fourth time. `what_a_row_exact_scrollbar_would_cost` is the diagnostic that holds the numbers above, so the next person re-runs them instead of re-arguing this.

## I4 — the walk became incremental 2026-08-04

> [!NOTE]
> **The walk is incremental as of 2026-08-04. Until then it re-read the whole worktree every tick**
>
> The narrowing above admits "counted for every changed file once per tick". That is not what shipped. `Frame::advance` dropped the span cache whole, because a span is derived from content and had no freshness check of its own. So every changed file the reader had **not** scrolled to was read from disk again on **every tick**, for as long as the process ran. Over the hundred-file fixture that is 94 files and 3.7 MiB a tick: **16.98ms p50 and 18.36ms p99 against I9's 16ms**. A reader is in that state one second after launch.
>
> **The fix applies I2a's own rule to the span.** The span gets the evidence a diff already carries (`Taken`: kind, index blob, and a settled fingerprint). It is revalidated with the same `reusable` function, not a second copy of the rule. A stat replaces a read. On the reference machine, in release, a hundred stats cost **1.29ms** against **12.90ms** to measure a hundred files. The ratio runs from 6.8x at 2000 files to 10.0x at 100. The tick above becomes **9.40ms p50, 10.67ms p99**, with zero files measured across a hundred ticks.
>
> **The fix has three costs.** First, first paint pays one extra `stat` per changed file, because a span can be carried only if it was fingerprinted when it was taken: **13.4-13.8ms before, 14.6-14.9ms after**, against I7's 50ms and I4's 100ms. The order of the sources matters. A file whose diff is already in hand needs no evidence at all. Asking for evidence first took `the_frame_budget_holds_through_a_bulk_rewrite` from 8.27ms p50 to 11.12ms, and from passing four local runs of four to passing two. `a_height_taken_from_a_diff_in_hand_costs_no_stat` is the structural gate that keeps that order.
>
> Second, the walk is incremental **outside** the settle margin, not inside it. A bulk rewrite of files nothing has drawn leaves every carried span unsettled at once. None can be proved, so the walk re-measures the whole changed set for the two seconds the margin lasts. This corner is also where a `.gitattributes` is most likely to arrive, and a fourth staleness rule covers that: see below. It is the pre-#101 cost, paid for a bounded window instead of forever. It adds one `stat` per file, for the fingerprint that makes the span carryable again once the margin passes. Measured over a hundred undrawn files rewritten at once, eight runs on a quiet machine: **p50 stable at 13.08-14.33ms**, and **p99 from 15.49ms to 44.70ms**.
>
> **That cost is reported, not asserted. The refusal to assert is the ruling.** Three instruments were tried. The first rewrote before every timed frame. The second discarded one frame after each rewrite. The third discarded twelve and partitioned frames by what they actually re-measured. None separated 1.7 MiB of fixture write-back from the subject. A stable p50 under a tail that moves 3x is the signature §7 names, not a number to gate on. So `what_a_bulk_rewrite_of_undrawn_files_costs` prints the distribution and asserts only what is exact: the corner was entered, the worktree stayed undrawn, and how many files a frame there re-measures. It prints the syscall count beside them. The same corner **is** gated as a count, by `a_tick_inside_the_settle_margin_stats_each_file_once` in `reads.rs`, which is the tier that works on a shared machine. The soak's drift gate follows the same rule for another invariant. An agent running a formatter is exactly this workload, so it gets its own gate and not a note: `what_a_bulk_rewrite_of_undrawn_files_costs`.
>
> Third, the fingerprint is **lazy**, which keeps that corner at one `stat` per file instead of two. `reusable` refuses an unsettled observation before it asks for a fresh print, so the pre-check costs nothing there. Taking it eagerly doubled the syscalls in exactly the window that can least afford them. A count holds this, not the p99, because the p99 has more headroom than the reorder costs: `reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`.
>
> **All three options #101 listed were rejected, for one reason: they were written against 93ms, and the number is 12ms.** They are recorded here, not in the issue, because the next person who finds this walk expensive will reach for them again. The number they were written against is 93.69ms. The walk measures **12.90ms** cold and **1.29ms** once incremental.
>
> *Parallelise the walk.* It buys back the read and nothing else, so after a 10x reduction it buys the same thing twice. It costs a thread pool or a hand-rolled scope on **every tick**. I3 is a claim about a process left open for days. A monitor that wakes several cores each time an agent saves a file is a different product from the one §2 describes, whatever its p99 says. Rejected on the product class first and the arithmetic second.
>
> *Stream the first paint.* It addresses a first-frame cost, and the first frame is 14.9ms against I4's 100ms, so there is nothing left to stream away. It also needs a wake on completion, which raises what I1 forbids, and reopening that to buy nothing is the wrong trade. It stays where §10 keeps it, with the non-streaming walk, in [#48](https://github.com/breferrari/vigia/issues/48).
>
> *Approximate the total.* Refused outright. #101 listed it so that it would be refused explicitly, not forgotten. It is the design the narrowing above replaced: a bar scaled from the current file's height vanished on a short file, ballooned on a long one and never reached the bottom. Of the three, it is the only one that costs a reader something, so it is the one to keep refusing.
>
> **What was taken instead is none of the three.** The walk did not need to be faster. It needed to stop repeating itself. I2a says that about diffs, and nothing had yet said it about heights.
>
> **What this did not fix**, recorded so the boundary is a decision: the height of a file whose diff *is* in hand was still taken by presence, not by proof. That is [#84](https://github.com/breferrari/vigia/issues/84). That branch was untouched, including the 20.71ms #84 records for proving it. [#101](https://github.com/breferrari/vigia/issues/101).
>
> **Fixed 2026-09-04 ([#412](https://github.com/breferrari/vigia/issues/412)), which makes the line above false.** Every fresh diff now puts its height beside it, and the walk asks only the span, with one stat. A print that moved inside the settle margin keeps its height until the file settles, and the walk reads it once then. Re-reading inside the margin was #84's 20.71ms breach of I9. So the in-margin re-measure above became a deferral: one stat per file per tick, and one read when the file settles. That frame is asserted against I9.

## I2 — why it is two numbers

> [!NOTE]
> **Why I2 is two numbers**
>
> I2 was written as one invariant, reading "re-highlighting is incremental". That merged two invariants with **different dependencies and different phases**. Incremental re-*diffing* needs only `gix` and is Phase 1. Incremental re-*highlighting* needs `syntect`, which Phase 1 does not include, so Phase 1 could not close while one number meant both. It was split on purpose, per the drift rule. The measurement that forced the split: re-diffing every changed file costs **18.58ms p99** on a 100k-line diff, against **3.27ms** for a single file. So I2a is load bearing, not an optimisation. Issues [#2](https://github.com/breferrari/vigia/issues/2) and [#4](https://github.com/breferrari/vigia/issues/4).

## I8 — why it no longer says SIGINT

> [!NOTE]
> **Why I8 no longer says `SIGINT`**
>
> I8 read "restored exactly on exit — including `SIGINT` and panic". The `SIGINT` half assumed something the shell proved false. **Raw mode removes the signal.** `enable_raw_mode` clears `ISIG` on Unix and `ENABLE_PROCESSED_INPUT` on Windows, so Ctrl-C is never translated. It arrives as an ordinary key event, and the key map handles it. That is why `Session` never needed a handler, and why no test of the clause as worded could ever be written.
>
> That leaves one real gap: a signal nobody at this keyboard sent, such as `kill -INT` or `-TERM` from another pane. It runs neither `Drop` nor the panic hook. `std` has no signal API, so closing the gap is a **dependency decision**, not an implementation detail. The single-platform fix (`signal-hook` on Unix, with `SetConsoleCtrlHandler` needed separately on Windows) gives a guarantee whose meaning differs by tier-1 platform. [#16](https://github.com/breferrari/vigia/issues/16) already rejected that trade as worse than one guarantee stated the same everywhere. The gap is tracked as [#24](https://github.com/breferrari/vigia/issues/24), and the invariant now states its own limit.

## I3 — why the scheduled soak is not twenty-four hours long

> [!NOTE]
> **Why the scheduled soak is not twenty-four hours long**
>
> The budget is a claim about a day, and it stays one. The proof column changed, because its number could not be run: a **GitHub-hosted job is terminated at six hours** of execution time, and a self-hosted one gets five days. Verified against GitHub's published limits, 2026-07-31.
>
> So the scheduled run is 30 minutes a week on Linux, over the 20 x 200 fixture, which settles in about thirty seconds. *Ruled 2026-09-29, reader.* The four-hour daily run before it used 100 x 500, which settles in about eight hours, and 27 of 39 Linux runs failed on the climb, with no leak traced. The full 24h runs by `workflow_dispatch` on a runner with no cap. The sample **count** is fixed, so the statistic is computed the same way at any window.
>
> The warmup does not scale down. Every process climbs to an allocator plateau before it is flat. A window that is all warmup can only measure warmup, so the gate refuses to assert there. §7 carries that as a rule.

---

## B10 — the terminal survey the hover reversal turned on

> [!NOTE]
> **Read 2026-08-16 while reversing [#123](https://github.com/breferrari/vigia/issues/123).** This is the first entry filed under a B-number instead of an invariant, because it supports a §11.2 ruling, not a row of §3. The decline's surviving reason was that *"nothing would ever tell a hover highlight to turn off"*, and it rested on the clause *"the takeover does not enable focus reporting"*. That clause described this repository's own `TAKEOVER` array but was written as a fact about terminals. Both halves were checked. The entry sits at the end of the file, not beside the I1 pair above, so that *"`RULINGS.md`'s I1 section"*, which three call sites cite, still means one contiguous block.

**`crossterm` has shipped the mechanism for years.** `EnableFocusChange` writes `?1004h` and `DisableFocusChange` writes `?1004l`. `Event::FocusGained` and `Event::FocusLost` already exist, and the keymap already answers both with `None`. `crates/vigia/tests/input.rs::nothing_a_reader_did_not_ask_for_becomes_an_action` asserts that by listing them among the events that do nothing. So the missing piece was one step in `TAKEOVER`, never a capability.

**Focus reporting is portable by the *opposite* route to the mouse bundle.** Do not reason about it from `terminal.rs`'s module header. `EnableMouseCapture` overrides `is_ansi_code_supported` to `false` on Windows, so `execute!` diverts it to the console API and writes zero bytes. `EnableFocusChange` has **no such override**, so on Windows with ANSI it does emit `?1004h`. Where ANSI is unavailable, its `execute_winapi` is a deliberate no-op, commented *"Focus events are always enabled on Windows"*, because `event/source/windows.rs` maps `InputRecord::FocusEvent` to the two events without being asked. Two mouse-adjacent commands in one crate have two different platform stories. Do not generalise from the one this repository used first.

**The Windows half is true, but its obvious citation is a code comment, and a code comment is not evidence about Windows. Microsoft's own reference says the opposite**: `FOCUS_EVENT_RECORD` is documented as *"used internally and should be ignored"*, with `bSetFocus` marked *"Reserved"*. The evidence is in conhost. `microsoft/terminal`'s `InputBuffer::WriteFocusEvent` pushes a synthesized focus event unconditionally when the console is **not** in VT input mode. `crossterm`'s raw mode clears only the line, echo and processed-input flags and never sets `ENABLE_VIRTUAL_TERMINAL_INPUT`, so this program is on exactly that branch. The contradiction is recorded openly. A reader who finds the Microsoft page first would otherwise conclude this entry is wrong. The finding is that the published doc is stale and the source supersedes it.

**None of the checked terminals is the gap. All six are named, because the count is the claim.** Alacritty is the one the reopening had not confirmed, and it does support the mode: its terminfo declares `XF, kxIN=\E[I, kxOUT=\E[O`, and `alacritty_terminal` carries `NamedPrivateMode::ReportFocusInOut`. WezTerm **implements** it (`wezterm-escape-parser`'s `FocusTracking = 1004`), which is the citable form. Its *docs* mention 1004 only in a 2020 changelog line, which adds its own caveat, *"local (not multiplexer) terminal sessions"*. xterm originated the mode, in patch #224 of 2007. iTerm2 handles `case 1004` in `VT100Terminal.m` and lists focus reporting in its published feature spec. kitty defines `FOCUS_TRACKING (1004 << 5)` in `modes.h` and declares `XF` in its terminfo. So the evidence is source and terminfo for four terminals and a published spec for two. An earlier draft claimed all six were *"confirmed against their own specifications"*, and two of them publish none.

**The list stops at six on purpose.** §11.1's colour ladder already rules that a terminal list is *"evidence about terminals someone checked rather than a claim about the ones nobody has"*. The first draft of the ruling said *every tier-1 terminal implements the mode*. That quantifies over a set this repository has never defined, since "tier-1" here means a build target. Terminal.app was not checked. It is the nearest unexamined case, and §11.1 already singles out `Apple_Terminal` as the entry that breaks the colour table. No `nsterm` entry in ncurses' terminfo uses `xterm+focus`. ncurses also records terminals that implement the mode *badly*. Its own comment says *"Some terminal emulators implement xterm focus in/out, but do it incorrectly, interfering with user applications"*, with notes against xterm.js, mlterm and st. None of that changes the ruling, because these terminals land on the ladder's bottom rung, which is the residual. It changes only what can be *said*.

**Why Alacritty's changelog is silent is not known, and the first explanation offered was wrong.** The draft said *"a feature present since before a changelog started is invisible in it"*. That does not fit. Alacritty's changelog begins in 2018, and the terminfo capabilities cited above were added in 2023 without a mention. The useful half survives without the mechanism: **an absence in a changelog is not evidence about a feature.** Treating it as evidence nearly kept Alacritty on the unsupported list.

**The gap is a multiplexer default.** tmux's `focus-events` defaults to **off** (`options-table.c`, `.default_num = 0`), and `tty.c::tty_update_features` gates the enable sequence on that option. That is the direct evidence that under a default tmux, the outer terminal's focus reports are neither requested nor forwarded. This does not affect every reader. §1 asks for a pane beside an agent and names no multiplexer. A terminal's own split has the rung, and tmux without that setting does not.

**A second tmux default may matter more. It concerns the mouse this program already uses, not hover.** Read in `server-client.c`: `server_client_reset_state` takes its mode from the **active** pane. The loop that unions `MODE_MOUSE_ALL` across every pane sits inside a guard on the session's `mouse` option, and that option defaults to off (`tmux.h`, `TMUX_MOUSE 0`). If this reading is right, a default tmux never requests `?1003h` for a pane that is not active. Then `vigia`, sitting beside the agent the reader is typing in, receives no motion at all: no wheel, no click, no hover. **This was read from source and not reproduced.** One `tmux new-session` can settle it. It is filed as [#188](https://github.com/breferrari/vigia/issues/188), not stated in a ruling, because it is a claim about shipped behaviour that predates B10.

**One citation is weaker than it looks, and it is stated at its real strength.** tmux issue 4909 reports terminal-level focus-out being absorbed instead of relayed, with several panes and `focus-events on`. It was reported against 3.4, and the reporter said it reproduced on master. It was **closed for lack of the requested logs, never diagnosed and never reproduced by a maintainer**. So it is a report, not evidence. It is cited only so that a reader who follows the link and finds a closed issue does not quietly downgrade the whole survey.

**The decline's first clause was false too.** DEC Locator's DECEFR reports a pointer leaving a filter rectangle, so a mouse protocol does report a pointer leaving. It is out of reach: `crossterm` exposes no DEC Locator, and a second mouse mechanism inside I8's takeover is not on offer. But the decline asserted an absolute about the outside world and never checked it.

**What the survey changed in the ruling.** It did not restore the decline: inside the pane, `?1003h` reports at cell granularity and retires the mark without any of this. It bounded the residual to one case: the pointer leaves the pane while the window stays focused, on an idle tree. It also showed that this case is *ordinary* for one common setup, not exotic. That forced the constraint the ruling turns on: the mark must be quiet enough that a stale one costs nothing. A clean survey would have produced a weaker ruling.

## B10 — what the mark is drawn in, and the contradiction it shipped with

> [!NOTE]
> **Read 2026-08-16 while ruling [#193](https://github.com/breferrari/vigia/issues/193).** The entry above asks whether a hover mark can be *cleared*. This one covers what it is *drawn in*, which the adoption pass got wrong in a way no gate could see. It is here and not in `SPEC.md` because contrast numbers date.

**§5.3 shipped two sentences that contradict each other, written one day apart in the same section.** B10's derivation rules that a mark about an input device *"must be the quietest thing still visible in that region"*. Its reason is *"a glance has to reach the worktree first and the pointer never"*, because the mark can go stale where a recency cannot. The colour paragraph two below it ruled that `Theme::path_hover` sits *"above all three"* recency weights, *"brighter than `Theme::path`'s pulse weight"*. The shell implemented the second sentence, so the pane's most perishable claim was its loudest text. Nothing failed. Every gate over it asserted **separation** from the recency ladder, and the loud form satisfies that perfectly. So eleven green assertions missed the defect, and one reader looking at the screen saw it.

**The correction keeps the sentence that had a reason and drops the one that only had a placement.** Quietness follows from staleness. *Above all three* followed from anti-collision, and the brightness was never what prevented collision. `SPEC.md` §5.3 already named the real channel in the same paragraph: *"it underlines, which is what keeps the two apart where colour runs out"*. It also observed that on `ansi` the brightest path *"has nowhere further to go"*. That observation was treated as a difficulty the brightness had to survive. It is the proof that the brightness was not doing the work.

**The value is `Theme::bar_hover`'s, so the pointer reads as one mark, not two.** A step button, a thumb and a listed path are three surfaces one gesture crosses. Until this ruling, they answered it in two visual languages.

**Measured, not adjusted by eye**, on the same instrument `tests/palette.rs` uses:

| palette | value | against the pane | for comparison |
|---|---|---|---|
| `dark` | `#a8b1bb` | **8.71:1** on `#0d1117` | `path_live`'s `#e6edf3` is brighter, `path_cold`'s `#7d8590` dimmer |
| `light` | `#3d4650` | **9.59:1** on white | quieter here means *lighter* than `path`'s `#1f2328`, which is the same rule pointed the other way |
| `ansi` | `Gray` | not measurable, and that is the point | it **equals** `path_cold`'s foreground, so the underline is the whole separation |

Read the `ansi` row carefully. The sixteen names hold nothing between colour 8 and `Gray`, so a quiet mark on that palette is exactly the cold rung's colour. A reader hovering an already-cold file sees only the underline change. That is a real narrowing, and it is accepted openly. It is the case §5.3 names the underline for, and the alternative is the loud form that ranked the pointer above the worktree.

**This is the third time B10 teaches the same lesson, in a new shape.** The first two times, a *reason* expired. This time, two reasons never agreed. A section can contradict itself while every test passes, because tests assert against the implementation, and the implementation can follow only one of the sentences. **When a document rules twice on one thing, read the second ruling as a claim to check, not as a restatement.**

## I9 — #261's stated cause was wrong in every part, and what it cost to find out

Recorded 2026-08-22, closing [#261](https://github.com/breferrari/vigia/issues/261). It is here and not only in the issue because each wrong explanation is plausible and was believed for a while. The next reader who meets a slow Markdown frame will reach for them in roughly this order.

**What was claimed.** A screenful of this repository's prose cost 117ms fully warm. The issue was filed undiagnosed on purpose, with a guess attached. The guess: the prose here is one line per paragraph, Markdown parses at roughly 7ms/KB against Rust's 2ms/KB, so a 24-line screenful of `SPEC.md` is 16.8KB of real work. The suggested remedy was a bound on what a frame parses.

**What was true.** Bytes do not predict the cost at all. Twenty-four *empty* lines inside a fence cost 25.3ms for 28 bytes of content. The same 10,288 bytes of `SPEC.md`, reflowed from 24 long lines to 138 short ones, cost 5.47ms against 5.85ms, so bounding display lines would not have helped either. Per-byte cost across this repository's own Markdown varies 100x from line to line, so "7ms/KB" is not a rate. The real mechanism is Markdown's block-start lookahead. Its last alternative tests for a table row, and both branches of that test require a literal `|`, so a line without one can never match. The engine only learned that by first exploring the embedded inline-content alternation, at roughly 4x per code span, until `fancy-regex`'s backtrack limit cut it off.

**The title was right about one thing, for the wrong reason.** One-line-paragraph prose really is the shape that hurts, but line length is not why. Markdown runs the block-start lookahead only on a block's **first** line. A continuous paragraph pays once, and a screenful of one-line paragraphs pays on every row. This was measured while building the gate, and it is why the gate's fixture separates its lines with blank lines. Eleven rows of one continuous paragraph measured **cheaper in the frame (15.30ms) than a single row of the same content parsed alone (16.88ms)**, which shows that ten of them never reached the pattern. A fixture written the obvious way would have passed against the very defect it was built for, and it did pass, twice.

**Four proposed mechanisms, all refuted.** Re-running them is pure cost.

| proposed | refuted by |
|---|---|
| Bound the parse per frame, leave the tail for the next | Never converges under editing: at roughly 8ms a fenced line it takes about 24 self-driven frames to colour one screenful, repainting continuously. `Shell::draw`'s docblock records the `while` that spun at 100% CPU |
| Rewrite the code-span sub-pattern, which appears about twelve times in the alternation | Ablated: 13.00ms against 12.83ms. Not the blowup |
| Use Sublime's `branch_point` / `branch` / `fail` to cut the search | `syntect` implements none of it and ignores the keys silently |
| Switch to `syntect`'s default syntax set, which looks 8x faster | It does not highlight embedded code at all: 0 shell scopes inside a ```` ```sh ```` fence against 26 for ours |

**One instrument trap matters more than the fix.** The first three hours went into a scratch crate that did not match this workspace's `[profile.release]`. Identical code, identical dump and identical content measured **13x apart** from the real thing. §7 already says an absolute gate on a shared machine is a weak instrument. This is the same lesson one level down: here the *build*, not the machine, made the number meaningless.

## I9 — a profile is shared by four targets, and this one was tuned by accident

Same pass. `codegen-units = 1` had been in `[profile.release]` since the budgets were first written, on the usual reasoning that fewer codegen units optimise harder.

**On Windows it made `fancy-regex` compilation roughly 6x slower.** `syntect` compiles patterns lazily on first use, so the cost landed on frames a reader was waiting for: a 24-line `sh` fenced block cost 286.91ms of parse at 1 and 22.39ms at 2. It is a cliff at 1, not a gradient, and `lto` makes no difference either way.

**On Linux it does nothing at all.** Re-measured 2026-08-22, interleaved over three rounds against three separately built binaries, `codegen-units` 1, 2 and 16 sit within 3% on every fixture. The cold parse, where a compile would show, is 11.999ms at 1 against 12.113ms at 2, within 1%. The setting did apply: the binary went from 3,493,720 to 3,604,896 to 4,077,632 bytes. Nobody has ever measured macOS.

Two lessons. The second is the general one:

- **A number measured on one target is a claim about that target.** The original write-up was going to record this as a property of the profile, and that would have been wrong on two of the three tier-1 targets. `SPEC.md` §9 ships four. A `[profile.release]` key is shared by all of them, and a measurement is not.
- **A gate calibrated against a platform-specific artefact makes that artefact a requirement.** `warm.rs` asserted a 10x cold-to-warm ratio, sized against a Windows cold parse that was mostly the codegen penalty. The first time the suite ran on Linux it failed, at 5.70x, and it failed identically at `codegen-units` 1 and 2. So 10x had never been this platform's number: nothing had ever run it here. Lowering the constant would have kept the shape and only moved the edge. The gate now asserts what `warm` actually claims, in absolute terms no codegen setting can invalidate again: the warmed parse fits inside a frame, and warming removed a frame's worth of work from the frame behind it.

## B13 — the sheet's height axis dropped gestures in silence, and the width axis still can

The ruling is `SPEC.md` §11.2 B13, and the shell's behaviour is in §11.1. This entry holds the measurement the ruling rests on.

**Before, on `main` at 0.25.0.** Every width from 20 to 140 was swept against every height from 3 to 40. The sweep counted the gestures painted inside the sheet's own rect, not anywhere on the pane:

| pane width | most gestures reachable, at any height |
|---|---|
| 24 to 25 | 3 of 16 |
| 26 to 31 | 4 to 5 of 16 |
| 32 to 34 | 9 of 16 |
| 35 to 44 | 11 of 16 |
| 45 and up | 16 of 16 |

This table shows two things that [#286](https://github.com/breferrari/vigia/issues/286)'s own table did not. First, the height floor drew **4** of 16, not 3. `SHEET_KEEP` is a keep-count, and the floor rung had room for one more. Second, the loss came as much from **width** as from height. At 40 columns, the width I6 is named for, the ceiling was 11 at every height, and the five missing gestures were the whole mouse group. No height reached them. The tight one-column sheet with the mouse group was 43 columns wide, and a 40-column pane has 40.

**One string made it 43.** `MOUSE`'s longest tight verbs were `scroll what you point at` (24) and `one row, repeats held` (21). The keyboard group's longest was 18, and the longest key was `click a track` (13). So the wheel alone set the table's verb field. With those two verbs at 17 and 19, the field is 19 and the sheet is `13 + 2 + 19 + 4 = 38`, which fits 40 columns with two to spare.

**After.** Every pane of 38 columns and up reaches all sixteen, at every height that draws a sheet. 35 to 37 reach eleven, 32 to 34 reach eight, and 30 to 31 reach four. Below 30 nothing is drawn. The narrowest sheet went from 24 columns to 30, because every rung charges the page counter's widest spelling, so the ordinals never run into the close control.

**That is the second reason recorded for this charge, and the first was false.** The first reason was that the charge keeps a centred box the same size between pages. A mutation that removed the charge left `the_box_does_not_resize_between_pages` green and turned two width gates red, the opposite of what the claim predicted. `sheet_fields` measures over the whole row set, and every page of a pane shares that set, so the width never depended on the page.

**The two-column rung moved with the new verbs, and the plan did not predict it.** `sheet_beside` measures the same mouse cells, so the tight rung went from 76 to 71 columns and its arrival from 78 to 73. The change is additive: panes from 73 to 77 columns drew eleven gestures before, and draw all sixteen on one page after. (**B13's counts in this section are B13's own and are not current.** `r` and then `s` were added later and moved every one of them. The current counts are in `SPEC.md` §11.1.) This is recorded as a deviation and not folded in, because this ledger exists to catch an unpredicted number.

**A gate found what the counter cost.** The first `sheet_counter_floor` asked the formatter for `(16, 16)` and got the *short* spelling, ten columns instead of thirteen, because the range form never draws that pair. Every rung was then three columns narrower than the counter it had to fit, and the sheet drew at 27 where the ruling says 30. The fix takes the maximum over the pairs the planner can actually return, once per process. Deriving the width by arithmetic was rejected, because it repeats the same defect one layer over: two expressions that agree about a sum only by hand.

## B13 — what the audit found that the ruling had shipped

Both defects belong to B13. The change that made the sheet page introduced both, and the suite that shipped it could see neither.

**The close control advanced the sheet.** A click on `✕` returned `Action::ToggleSheet`, the action `?` sends. Once `?` meant *advance*, the control advanced too. On a six-page pane, leaving took six clicks, and a pointer has no `?` to fall back on. `SPEC.md` §11.1 and `Action::ToggleSheet`'s own docblock both said the opposite. That was the fourth false claim in this element's documentation in one pass.

The gate meant to catch it could not fail. `the_close_control_dismisses_and_the_sheet_swallows_the_rest` asserts that `action_for` on the control's cell returns the dismissing action, on an eighty by twenty-four pane. That pane is **one page**, so there is nothing to advance to and the two actions look the same. The test also asserts the action's **identity** and never applies it, so the effect on state is not checked. An identity is not an outcome.

**The last page's box moved.** `paged_fit` sized the frame from `take`, which is a remainder on the last page, and `sheet_plan` centres the box on its height. So the last page shrank by the remainder and slid down by half of it. The close control moved with it, and the row it left fell through to a scrollbar the reader could not see. The box is now `capacity + SHEET_FRAME` on every page, with the tail blank inside the frame.

`the_box_does_not_resize_between_pages` recorded `(left, width)`, the two edges that did not move. It now records all four. A second gate covers the tail's frame. `the_sheet_is_a_closed_box_at_every_rung` sweeps 3,400 panes but reads only **page one** on each, because its scaffold toggles once and paints, so it cannot reach the blank tail.

**Twenty mutations, twenty killed.** Four are the defects above and the gates for their fixes. The rest cover the ladder, the clamp, the counter, the drop order and the drain. Two survived their first run, and both were instrument failures, not gaps. One ran against a test binary that did not contain the test. The other patched a file that an earlier `git checkout` had already reverted. A mutation that never applied and a mutation the suite failed to kill report the same result, and they need opposite responses.

## B14 — the rail arrived on its own, and one number is the whole argument

The ruling is `SPEC.md` §11.2 B14, and the shell's behaviour is in §11.1. This entry holds the trade. The reversal is narrow, and the part that was *not* reversed is the part most likely to be argued again.

**What was reversed.** Not the width. [#252](https://github.com/breferrari/vigia/issues/252) derived 134; it did not choose it, and the derivation stands. Both regions read one glance ladder, so splitting a pane costs each half the width the whole had. A split costs no rung only where both halves, and the undivided pane one column narrower, sit on the same plateau. The other plateau needs a 328-column pane, so 134 is the only answer. What was reversed is that **crossing 134 was automatic**.

**One number decides it, and §11.1 already stated it.** At 133 the diff plans against 129 columns. At 134 it plans against 60. Widening a terminal past a threshold nobody chose more than halved the region this tool exists to show. §11.1 called that "the feature rather than a defect". That is true *for a reader who asked*. For a reader who did not ask, the same sentence is why this reopened.

**Why opt-in, not opt-out.** Both need the same discovery path, because the gestures sheet names the key either way. So the only question is which default a reader who never opens the sheet gets. The answer is the default that changes nothing.

**The picture stopped being an exception.** `assets/preview.svg` is a 109-column render. §5.1 could only say that the picture and the code "describe the same pane" by noting that the picture sits below the arrival width. With the rail asked for, they describe the same pane at every width.

**The cost fell on the sheet, not the pane.** A key is a row. The gestures table went from eleven keyboard rows to twelve, and every row count in §11.1 moved: the one-column rung to eighteen table lines in a twenty-row box, the two-column rung to `104 x 15` and `71 x 15`, and the roomy rung to `68 x 30`. (**Those numbers are B14's own.** §11.1 has the current ones.) **No width moved**, which kept this to one issue. The cells of `r` are `r` and `show or hide the left rail` (25 columns) or `the left rail` (13). Both fit inside the existing maxima: 22 and 28 wide, 13 and 18 tight. The plan predicted every one of these before the run, and every prediction held.

**The keep-set did not move, and the first reason written for that was false.** `r` is a fourth gesture a reader cannot guess, beside `f`, `m` and `?`. `SHEET_KEEP` keeps three, so one of the four goes first. `r` is given up at rank eight of `DROP_ORDER`, two before `f`, with `s` between them since B16.

The reason first recorded was that the drop order binds at 30 to 34 columns while a rail needs 134, so `r` cannot fire on the pane that drops it. **No such pane exists.** At 30 to 34 columns the rung is `from = 7`, and `r` is *kept*. This repository's `NARROW` table asserts that by name. The rank that drops `r` is `from >= 9`, which needs a width below thirty, and below thirty no sheet is drawn.

So the reorder is **unreachable on every pane that draws a sheet**. It is a defensive ordering of the tables, not an observable behaviour. `sheet_tables` asserts that the keep-set is `f`, `m` and `?`. Without the reorder, the original order would drop `f` instead. If a rung ever reaches that depth, `r` is the right one to lose, because it is the only one of the four that needs 134 columns. The audit found this. It is the fifth false claim in this element's documentation in two passes, and the second that could have been checked against a table in the same repository.

**An instrument note from the pass.** `cargo test --workspace` stops at the first failing binary. So a grep for `FAILED` over its output reports only that binary's failures, and it reads as green once that binary passes. Two counts in this pass were taken that way, and both were wrong. The first missed a *compile* failure, which prints no `FAILED` line. The second hid twenty-nine failures in later binaries. The flag is `--no-fail-fast`.

## B15 — the arrows were free, and the only thing they cost is a spelling on the sheet

The ruling is `SPEC.md` §11.2 B15, and the behaviour is in §11.1. This entry holds the trade and a correction.

**The issue's own body made a false claim, and I wrote it.** [#296](https://github.com/breferrari/vigia/issues/296) was filed on 2026-08-24. It said [#272](https://github.com/breferrari/vigia/issues/272) *"would want those arrows if horizontal reading ever lands"*, and offered that as the thing to rule against. #272 asks for **`w`**, a wrap toggle, and needs no arrows. The real conflict is with a **horizontal pan**, which §11.1 declined in the sentence after the one that ruled a long line clipped, not wrapped. (The first draft of this section said "the same sentence". It is the next one.) So the arrows were contested by a rejected alternative, not by an open row, and the ruling was cheaper than the issue suggested. A premise written into an issue reads as settled the next day, and the issue's author is the least likely person to re-check it.

**The one measured cost is the tight spelling of one keys cell.** The sheet's tight keyboard keys field is eleven columns, set by `Space  PgDn`. The arrowed cell `n  →  /  p  ←` is thirteen. Carried at the tight spelling, it would move the keyboard-only rung from **35 columns to 37**, so panes of 35 and 36 would fall to the next rung down and lose their twelve gestures. The whole-table rung does not move either way, because the mouse group's `click a track` is already thirteen.

Losing gestures on two columns of pane for an **alias** is the wrong trade, so the arrows appear at the wide spelling only. That matches the existing convention: `q  Esc  Ctrl+C  Ctrl+D` becomes `q  Esc`, and `g  Home  /  G  End` becomes `g  /  G`. `j  k  ↓  ↑` keeps its arrows because there they cost nothing. The test is the same, and it gives a different answer there.

**No row was added, which is why this diff is small and #295's was large.** An alias goes into an existing cell. So `KEYBOARD` stayed at twelve rows, the counter still counted to seventeen, and every rung height and reachability boundary in §11.1 was untouched. #295 added a row and moved all of them. (**Those numbers are B15's own and are not current.** B16 added `s` the next day and moved them again. The current ones are in §11.1.)

## B16 — the pin makes the frame path cheaper, and the guard that would have made it dearer

The ruling is `SPEC.md` §11.2 B16, and the behaviour is in §11.1. This entry records one defect found by reading, one premise checked instead of inherited, and what a thirteenth key did to the sheet.

**The defect is a literal that stopped meaning what it says.** `View::collect` backs a short screen up so that the diff's last row rests on the bottom. It skips that when the position is already the first one the walk can reach. The test is spelled `view.top != Position::default()`, and `Position::default()` is *file zero, row zero*. That was correct while the walk always started at the first changed file. Under a pin it is wrong, because the first position a pinned walk can reach is the pinned file's own row zero. So on any pinned file but the first, the guard never fires. A pinned file shorter than the pane is `short` on every frame and restarts on every frame. It pays what the guard's own paragraph records: **three walks and six `Frame::diff` calls a frame, against two**. That lands on the file an agent is writing to, which `Frame::diff` re-reads inside the settle margin by design.

Both walks resolve to the same position and draw the same rows. Nothing on screen shows it and no snapshot moves. The only instrument is the frame's own read count, which `tests/single.rs::a_pinned_file_shorter_than_the_pane_walks_once` asserts. It was found by reading the guard's docblock while working out what the pin had to bound. That was luck, not method: nothing was going to go red.

**The general lesson.** A bound written as a type's `default()` reads as *the floor*, but it means *the floor of the old walk*. When a feature narrows what a walk can reach, every such literal is part of the change. Reviewers rarely look twice at the ones spelled `Default::default()`.

**B14's premise was checked, not inherited.** B14 ranks `r` outside the sheet's keep-set, and its **first** stated reason was false: it said `r` cannot fire on the pane that drops it, when no drawable pane drops it at all. The correction is in B14. Ranking `s` at nine raised the same question. This time the answer came from drawn output, not from B14's conclusion. Swept over every width from 20 to 45, the deepest rung a drawable pane reaches is still `from = 7`. So the `NARROW` table now shows both `r` and `s` **kept** at thirty columns, and both reorders remain defence, not behaviour.

**What a key costs, measured twice.** `r` and `s` are the same shape of change with the same shape of cost: every row count moves and no width does. The four reachability boundaries sit at 30, 32, 35 and 38 columns, and neither key moved them. The counts behind them went from 16, 11, 8, 4 to 17, 12, 9, 5, then to 18, 13, 10, 6. Both times they were re-derived from a swept pane, not incremented, because only that notices a boundary that *did* move.

**One number goes the other way.** Every feature added to this pane before had cost the frame path something. A pin removes one of I4's two exceptions from the frames it is on. That exception is the diff's height, counted for every changed file once per tick, and it is the only one a frame with nothing anchored meets. A pinned frame reads its total from the pinned file's span instead. The gate asserts that the **unpinned** frame counted something before it asserts that the pinned frame counted nothing, because a zero over a fixture with nothing to count is not evidence. That is the two-fixture rule §7 states for every other cost claim here.

## B16 — eight audit rounds, one mechanism

The ruling is `SPEC.md` §11.2 B16. Eight audit rounds ran over it. **Every serious finding after the first had one cause: a claim that stopped being true when the code moved under it.**

| round | what it found |
|---|---|
| one | three gates green with the feature deleted, and a key nothing proved was bound |
| two | a round-one fix that never reached the shell, and a latent panic a docblock hid |
| three | a round-two fix that reached two of three call sites, and a coverage claim nothing enforced |
| four | a round-three fix sized against a chrome its own side effect invalidates, and a claim that overstated round three's fix |
| five | the rule stated in two canonical places and inverted in one |
| six | the pin's clamp gated on a flag the pin did not set |
| seven and eight | a reverted fix leaving its description behind in two places |

**Round one: three gates passed against a `vigia` with the pin removed.** Each was vacuous for its own reason, so finding one would not have found the others. *The toggle gate discarded the middle draw.* It pressed `s`, wrote `let _ = draw(...)`, pressed `s` again and compared the ends. That only asserts that doing nothing twice does nothing. *The follow gate followed the last file.* Nothing comes after the last file, so a screen resting in it draws one file whether or not anything is pinned. *The file-changing gate never left row zero.* There `n`, `p`, a digit and a click all land on a heading, and every fixture file is taller than the body. **A fourth gap had no cover at all: nothing proved `s` was bound to a key.** Deleting the `KeyCode::Char('s')` arm left the whole workspace green, because every gate built `Action::ToggleSingle` directly. B16 could have shipped a gesture no keyboard could reach. [#295](https://github.com/breferrari/vigia/issues/295) had closed exactly that hole for `r` with a gate of its own, but the lesson did not carry over.

**The one behavioural defect in round one was in input, not drawing.** The shell drains actions in a batch and paints once at the end, so `G` and a held `k` arrive together with no frame between them. Under a pin, `G` wrote the pinned file's whole height and let `View::collect` clamp it on the way to the screen. The right rows were drawn, but the stored *position* could not be moved from, so every `k` in the same batch clamped to the same screen. Nine keystrokes were swallowed on a 22-row file at a 13-row body. `Action::Bottom` now writes the resting row. The cost is a staleness correction it used to get free from the clamp: a file that grew since the bar was drawn rests slightly short of its true bottom for one tick. That is invisible and corrects itself. Swallowed keystrokes are neither.

**Rounds two to four are one defect moving outward through the layers.** `Action::needs_height` classified `Bottom` as reading no height. That was true while `G` jumped to a heading. It became false when B16 made `G` rest the pinned file's last row on the bottom. `crate::run` passes a zero to any action the predicate calls false, so `span.saturating_sub(0)` is the whole span. The round-one fix was still shipping broken while its own gate was green. **The gate was green because a test called `App::apply` with a height a shell never passes. The suite modelled the function, not the program.** Round three found that the fix reached two of three call sites. The third was the held-repeat path, harmless only because `Regions::step_at` yields no height-reading action. Round four found the fourth layer. `Shell::diff_rows_for` builds the chrome to size the body *before* `App::apply` runs, and `Bottom` is a manual scroll. So `apply` turns follow off underneath it, and `Footer::plan` sizes its rungs from a `Chrome::following` that is about to be false. Between **31 and 40 columns** that decides a one-line footer against a two-line one, so the region drawn is thirteen rows while `span - height` was computed against twelve. All of these live inside `crate::run`, the loop no test enters. So **the fix is one function that every site calls, not a better gate.** Then one place can be wrong, instead of three answers that can drift.

**A coverage claim made this possible.** `only_the_action_that_reads_the_height_is_given_one` is the gate written for exactly a wrongly classified height. Its docblock said *"every action, so a new variant reaches this list by failing to be in it"*. Nothing made that true. It was a plain array naming eight of the seventeen variants that existed then, and one of the nine it left out was the second wrong height this branch found. **That is how `Bottom` shipped misclassified in the first place.** The gate is now exhaustive by construction, in two steps prose cannot satisfy: a `tag` function that matches every variant, so a new variant is a **compile error**, and a count assertion under it.

**Round six was the last behavioural finding: the pin's clamp was gated on a flag the pin did not set.** `View::collect`'s back-up rests a short screen's last row on the bottom. It fired only for `anchored || landed_inside`, because a position placed by a *jump* is a claim about the top row. `ToggleSingle` is not a manual scroll, so it inherited whatever set the position, and `App::diff_to` sets `anchored` to false. A reader who dragged the bar into the middle of a tall file and pressed `s` got a short screen with trailing blanks, which jumped upward on the next `j`. **Three places said the opposite, including this ruling's own §11.2 text.** The claim held only for a reader who arrived by scrolling, and every straddle in `tests/single.rs` was reached with `Action::Scroll`, which anchors. **A suite that reaches one state four ways, all of them the same way, cannot see the fifth.** The permission is now a term in the guard, `anchored || landed_inside || single`, not a flag written from the toggle's arm. The flag outlives the pin. The term does not.

**Rounds seven and eight are why this entry exists instead of a quiet fix.** Round seven replaced round six's mechanism and corrected `app.rs`, `view.rs`, `reads.rs` and `scroll.rs`. It left `SPEC.md` §11.2 B16 and this file's own round-six section describing the mechanism it had just removed. Code and `SPEC.md` disagreeing is a stop condition in `CLAUDE.md`, and it went one round undetected.

**The rule this branch earned.** No gate can read a comment, so the only check is a reader holding the sentence against the code. Each of these was found only after the previous one was fixed, because **a wrong docblock answers the question a reader would otherwise have gone and checked**. There were always more places that *state* a rule than places that *implement* it: one arm had three prose sites, and one guard had two rulings. So **when a behaviour moves, grep for the places that state the rule it obeyed, not only the places that implement it.** When a fix is *reverted* instead of extended, grep again. A revert leaves behind a description of something that no longer exists, and it reads exactly like a description of something that does.

## B6 — the amendment the ruling predicted, and the one-line defect that shaped it

`SPEC.md` §11.2 B6 is the ruling and §11.1 is the behaviour. This entry says why the result is two files, not three more keys in one.

**B6 predicted this amendment and named the test it must pass.** Its closing line reads *"there is nowhere to put a setting that is neither of those… the next setting that is neither a preference about you nor a fact about the terminal in front of you will find it again."* A view default is not in that gap. It is B6's **first** kind, a preference about you, and B6 already rules that those live in a file. So the amendment **applies** the taxonomy and does not widen it. B7 is still the only candidate that has ever been in the gap.

That kept the change cheap. Widening B6 would have needed the argument B7 was refused for. Applying it needed only a place to look.

**A defect decided the shape, and the defect is one line of `theme::from_env`.** That function resolves `VIGIA_THEME` first, and a built-in name wins outright:

```rust
match Theme::named(named) {
    Some(built_in) => built_in,
    None => load(Path::new(named))?,
}
```

So `VIGIA_THEME=dark` **never opens the theme file**. If the view keys sat beside the colours, a reader naming a palette for one session would silently lose their view defaults, through a gesture unrelated to the settings it discards. The theme parser already refuses unknown keys to avoid that class of failure: a setting that does nothing, with no way to find out why.

The tidiness argument points the same way and is the weaker one: a file called `theme` holding `rail on` reads as a mistake. It is recorded second because it would not be enough on its own.

**The two files share everything that costs something**: one format, one discovery rule (`HOME` then `USERPROFILE`, each checked for emptiness before the next is tried), one error path, and one report-before-the-takeover order. They do not share a subject. So the amendment adds a place to look and three keys to look for. The `VIGIA_GLYPHS` amendment had the same shape: it added a detector, not a surface.

**`follow` is excluded because of I5.** *Correct with zero interaction* is a promise about the program. A file that could turn following off would make it a promise about one reader's configuration. Every combination of the three included keys is a legitimate pane. A pane that has stopped following is not.

**No variable joins them.** A variable exists to say *not this time*, which is the whole job of `VIGIA_THEME`. Here `m`, `r` and `s` already say it, one press each, named on the gestures sheet. A variable would be a second spelling of something the pane says better. B6's count of one variable is unchanged. Only its count of files moved.

---

## B19 — the neighbour that already solved this, and the unit split the total could not survive

`SPEC.md` §11.2 B19 is the ruling and §11.1 is the behaviour. This entry is the trail.

**The field was read, not remembered, on 2026-08-22.** Five tools were checked against their own source or their own `--help` on this machine, and one of them changed the proposal.

- **`delta`** wraps, and `--wrap-max-lines` defaults to **2**. A session here read that as a cap to adopt, and the reader removed it the next day: *"How often a line should be wrapped if it does not fit. Zero means to never wrap. Any content which does not fit after wrapping will be truncated."* That answers §11.1's objection directly, and it is where the cap came from. `--wrap-left-symbol` is `↵`, `--wrap-right-symbol` `↴` and `--wrap-right-prefix-symbol` `…`. So delta marks the wrap at the end of the line it leaves, not at the start of the one it enters.
- **`ov`** binds `[w]`, `[W]` to a character-based wrap toggle and `[Alt+w]` to word wrap. That confirmed the key.
- **`bat`** reserves the gutter and leaves it **blank** on continuation rows, so the missing line number is itself the signal. It spells the opposite state `-S` / `--chop-long-lines`.
- **`less`** wraps unless `-S`, and toggles the state at runtime.
- **Neovim** keeps both axes and gives each its own motion, `gj` against `j`. It indents continuations with `'breakindent'`: *"Every wrapped line will continue visually indented… thus preserving horizontal blocks of text."*
- **Every one of them wraps by default, and this one does not.** That settled the default: `bat --wrap` is `auto`, `less` wraps unless told otherwise, `ov` wraps, and `lazygit` ships `wrapLinesInStagingView: true`. Those tools get the whole terminal. This one gets half of it, beside an agent.

**The neighbour that had to build wrapping says why it is cheaper here.** [dandavison/delta#657](https://github.com/dandavison/delta/issues/657) is the request that produced delta's wrapping. Its problem statement is that the pager is the wrong layer: *"delta uses one of many various pagers, usually some form of less, which supports line-wrapping, but this breaks lots of things (like delta's line numbers)"*. This tool owns its painter and its gutter, so the hard part delta built around does not exist here. The same thread calls the horizontal-scroll alternative *"tedious"* and says it *"disrupts the viewing experience"*. That is second-hand evidence for wrapping over the pan that §11.1 named as the rejected alternative.

**The total was tried the way the issue proposed, and abandoned on a fact.** The issue asks for a wrapped height threaded through `view::rows_of`, measured on the counting pass and cached per text width. Two objections kill it, and only the second is decisive.

The weaker objection is cost. `vigia_core::FileSpan` is four numbers today, and a wrapped height depends on the text **and** the text width. So the span would need either a per-width summary or a per-file re-measure. That is an argument about size, and a measurement would have answered it.

The decisive objection is that the dependency is **circular**. `render::gutter_width` sizes the gutter from *the largest line number on screen*, so the text width depends on the drawn rows. A wrapped height depends on the text width. The total depends on the wrapped heights. Which rows are drawn depends on the total. No order evaluates all four, so there is nothing to compute.

**So the unit is split, and each unit has one owner.** A **logical row** is a row of the diff's model. A **display row** is a row of the terminal. The bar, every jump, every clamp and every counting twin stay logical. Display rows exist only inside the viewport. The design record already names this shape: when one quantity splits into two, every site that treats them as interchangeable becomes a defect at once, so the quantity gets one owner, not a spelling at each site.

**The split reached five sites, four of them outside the walk.** `App`'s pinned `G`, its drag and its page steps subtracted a display height from a logical span. `view::landing_of` could judge a change visible below the fold. The walk's own `short` read a display-full pane as short. Each now measures in display rows. The bar's far end stays out: it asks for a row past the end and lets the walk clamp, because placing the end of the diff is the walk's job.

**The split showed a units bug at once.** `View::last_screenful` and the overshoot branch in `View::collect` both compared a **logical** span against the **display** height. With wrapping off the two numbers are equal and nothing can see it. With wrapping on, the top lands too far back and the last rows of the diff fall off the bottom, so the gesture meant to reach the end of the diff cannot reach it. `crates/vigia/tests/wrap.rs::the_bottom_of_the_diff_is_reachable_when_lines_wrap` fails if that comes back. `the_wrapped_bottom_survives_the_frame_after_the_gesture` beside it fails if the clamp holds for one frame and not the next. The first draft of the fix fired on the frame the gesture produced and on no frame after it, so the end of the diff stayed visible only until anything repainted.

---

## B21 — the alternatives refused on measurement, and the facts the ruling corrected

`SPEC.md` §11.2 B21 is the ruling. This is the trail, read and measured on 2026-09-05.

**`rmcp` 3.2.0 costs 28 crates, not a dozen.** `cargo tree` on `default-features = false, features = ["server", "transport-io"]` resolves 49 crates, and 28 of them are absent from this lock file. `tokio`, `futures` and `schemars` are among them, and a `syn` 3 sits beside the `syn` 2 already here. That feature set is pure Rust, with no `aws-lc-rs` and no `ring`. A hand-rolled server over `serde_json` adds nothing the lock file does not already hold. The switch reopens when elicitation or channels come to matter.

**`ratatui-textarea` 0.9.2 costs one crate in the lock file and six in the binary.** The same command on the `crossterm` feature alone resolves `ratatui-core`, `ratatui-crossterm`, `ratatui-widgets`, `unicode-segmentation` and `unicode-width`, and all of them are already in the lock. **Corrected 2026-09-06 when the build landed ([#419](https://github.com/breferrari/vigia/issues/419))**: `cargo tree -p vigia` over the build graph gained six crates on Windows and seven on musl, including `time` and its tree. The crate declares `ratatui-widgets` with that crate's defaults, and `calendar` is one of them. The lock file lists optional dependencies whether or not anything enables them, and the count above read the lock file. §6 carries the number that ships. The `tui-widgets` workspace was read and none of it was taken. `tui-scrollview` renders the whole content into a buffer of its full size on every frame, and I4 forbids that. `tui-popup` floats, and the ask is inline. `tui-prompts` is one line.

**Channels and `claude -p --resume` are not taken.** A channel server needs `--channels` on every session, and while the preview lasts it shows a full-screen warning for a server outside Anthropic's list. So the server is built one capability away from being a channel. `claude -p --resume <id>` interleaves two writers in one transcript and costs a `claude` process per note.

**Three facts in the issue were corrected against the documentation.** Messaging is on from v2.1.224 on macOS and Linux and from v2.1.234 on native Windows, not 234 and 243. Delivery into an idle interactive session is documented, so only the payload after the auth line is left for #420's probe. Own-child verification uses process evidence on Linux, process evidence or the token on macOS, and the token alone on Windows. The pane is not a child, so it passes that check only through the token.

**`.git` was refused as the store's home, on the writes gate and on the linked-worktree split.** A linked worktree's git dir is not its common dir, so one store per worktree under `.git` would be two places. The writes gate promises the tree is *left byte for byte as it was found*, and that promise outweighs the convenience.

## 11.1 — the two widths of a diff row

Measured 2026-09-09, [#474](https://github.com/breferrari/vigia/issues/474). The two widths agree wherever a bar is drawn. Where none is drawn, they differ by 2 below forty-four columns and by 1 up to seventy-nine. The alternatives, and the 2026-08-10 deferral they answer, are on the issue.

## 11.1 — one pulse mark

Ruled 2026-09-29, session ([#362](https://github.com/breferrari/vigia/issues/362)). Rejected: marking every path of the burst, because a 31-file write marked every row. Rejected: a cap, because it needs an *n* nobody chose.

## 11.1 — the list-alone state took a key of its own rather than a third state of `s`

Ruled 2026-09-10, reader, from the pane ([#493](https://github.com/breferrari/vigia/issues/493)). Making `s` cycle through whole diff, one file and no diff would keep the keymap the size it is. It was refused because of the config file, not the keymap. `config.rs` accepts exactly `on` and `off`, and `Config` is a struct of `bool`. A three-state `single` would change the file's grammar, and that is a §11.2 B6 change, not an implementation detail. A new key costs one row of the gestures sheet, one `Config` field, one `Action` and one `Place` arm. It also keeps `README.md`'s claim that each key means one thing. `l` was refused for the reason the keymap gives for refusing `h`: it is a vi motion everywhere else, and this pane has no horizontal scroll.

## The written layer — a ledger and a prose file cannot obey one ceiling

Ruled 2026-09-04, reader ([#374](https://github.com/breferrari/vigia/issues/374)); implemented 2026-09-10. `WRITTEN_LAYER_BUDGET` gave six documents a byte ceiling each, and each ceiling equalled its file's size on the commit that set it. In practice, the written layer could never grow. That is right for prose and wrong for a ledger.

`WRITTEN_LAYER_BUDGET` and `LEDGERS` in `crates/vigia/tests/package.rs` carry the standing rule and its reason. This entry holds the evidence the rule was kept and dropped on.

**What the ceiling bought over 2026-09-03/04, which is why prose keeps it.** A dependency paragraph was compressed. Two sentences that a change made false were deleted. A clause was restored once bytes were free again. Two ceilings were *lowered* when prose came out. Its value is not the CI failure. Its value is that a pass has to stop and ask whether a paragraph earns its place.

**`REVOCATIONS.md` is the worse of the two ledgers to cap, for a governance reason, not a size reason.** It records a session being overruled, so its growth is evidence that the process works. The ceilings that limited it were set by sessions: 1701, then 3217, then 3192. The reader set none of them.

**`WRITTEN_LAYER_TOTAL` is deleted, on one measurement.** Its job was to let a ceiling rise only while another fell by at least as much, so that a ruling moving between files could be told apart from a ceiling edited to fit. Across six merges it moved by exactly the per-file delta every time: #400 at −9 and −9, #395 at +906 and +906, #399 at +600 and +600, #398 at +157 and +157. A sum check cannot fail when the sum is edited to match. On [#402](https://github.com/breferrari/vigia/pull/402), a reviewer reading the diff line caught the lockstep edit, not the arithmetic. The total could never have worked while a ledger row forced the raise anyway. By the end its docblock held nine paragraphs, one per raise, and had reached the length at which `register.rs` caps a docblock.

**The objection.** Without the total, a prose ceiling can be raised whenever it is convenient. That was already true. Visibility is the enforcement either way, because a raise stays a visible, reviewed line in a diff. The fact that most undercuts this ruling: `SPEC.md` grew 1,538 bytes over the same two days the ratchet was working. Every one of those bytes was argued for and paid for. The alternative is unbounded, so the ceiling stays, but it is softer than it looks, and that number shows it.

`crates/vigia/tests/package.rs::no_ledger_carries_a_byte_ceiling` stops a ledger from being quietly returned to the budget. It names the two ledgers and fails with the reason, not with a number.

## §5 — following config and info/attributes

**Stat both files every tick, rather than the two other options #111 named.** Asking `gix` whether the attribute state it rebuilt differs from the last one is the correct question, but `gix` exposes no cheap identity for "the attribute state", and building one means hashing the resolved stack on every tick. Accepting the limit and documenting it was what shipped before, and it left a stale diff on screen until the file was touched again. Two stats a tick measured as noise (3.43ms against 3.46ms p50 over this repository, three interleaved runs each), so the cheapest option that works was taken.

**No settle check on the two files, unlike the attributes files in the changed set.** A new repository's config is young for its first seconds, so requiring a settled modification time dropped the caches on every tick, and two reuse gates in `tests/frame.rs` failed. The cost is the one change it misses: a rewrite of the same length inside one modification-time granule.
