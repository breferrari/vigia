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

**What held §11.2 B10, and still holds it after the reversal**, is `crates/vigia/tests/input.rs::pointer_motion_over_a_laid_out_screen_is_still_no_action`, which asserts that motion over a *laid-out* screen produces no action. The fixture matters: the older inert list hands `action_for` a `Regions::default()`, against which every region-gated arm returns `None` whatever it does in production, so a hover written the way the click arm is written would have left it green. That is an *actions* gate and not a paints gate, and nothing in the suite is the latter, because the honest count today is one paint per motion batch rather than zero. **The reason it survives B10 going the other way is worth one line, because it looks like luck and is not**: hover is view state rather than a keymap entry, so the assertion written to catch a hover *being built* is the assertion that keeps hover *out of the keymap*. A gate that forbids a mechanism outlives the ruling that motivated it; one that forbids an outcome does not.

**It cannot be given back.** The obvious repair is to request `1000`, `1002` and `1006` and leave `1003` off, keeping click and drag and losing only the wake. That is not portably available. On Windows `EnableMouseCapture::is_ansi_code_supported()` is `false` and `execute!` routes the bundle through the console API, writing **zero bytes**, so hand-written DEC modes would work on Unix and do nothing on Windows. The cost of the repair is the mouse on a tier-1 target, or a second mechanism inside I8's takeover, against a wake that is a channel receive and two pure calls. It is recorded rather than repaired.

**What it changed.** §5.3 had priced a hover highlight as *"a wake class I1 currently never pays"*, and §11.2 B10 was opened to weigh that trade. The trade did not exist: the wake is sunk either way, so B10 was decided on what hover would *show* instead. `crates/vigia/src/terminal.rs::every_command_is_the_escape_sequence_it_is_named_for` asserts the bundle byte for byte, so if `?1003h` ever stops being requested, this entry stops being true loudly rather than quietly.

> [!NOTE]
> **B10 was reversed 2026-08-16 and hover is adopted. Nothing measured in this entry changes; one clause of it is superseded.** Everything here is about a cost that is sunk in *both* directions, so it reads the same whichever way the ruling goes: adopting adds no wake, no paint and no frame. What is superseded is the last clause of the paragraph above, *"so B10 was decided on what hover would show instead"*. It was, and then that second reason turned out to be false too, for reasons that are about other people's software rather than about I1. §11.2 B10 carries the reversal and the **B10** section at the end of this file carries its trail.

---

## I1 — a warm that finishes is a fourth sender, and the row does not reach it

Ruled 2026-08-21 with [#129](https://github.com/breferrari/vigia/issues/129). The frame path stopped parsing under grammars nothing has compiled, which means a hunk can be on screen in plain text with its colour owed; something has to tell a loop blocked on `recv` that the colour has arrived.

**Quoted before it was cited, which is the rule this repository keeps having to relearn.** I1's row: *"Redraw is **event-driven**, never a timer that runs unbidden. No filesystem event and no git index change means no work."* Budget: *"**0 wakeups** while idle."* Measure: *"CPU sampled over a 60s idle window; assert no render calls"*, plus `nothing_held_means_no_timer_at_all` on the untimed wait.

**It does not reach a warm.** A warm exists only because a file was written or because a diff was already on screen when the process opened; on a tree nobody touches, nothing is spawned and nothing is ever sent, so *no filesystem event means no work* holds literally. It is bounded by the number of distinct grammars a session meets rather than by time, so it cannot repeat. And it leaves the wait untimed, so the structural half of the measure is untouched: `Shell::patience` still returns `None` with nothing held, and a warm arriving is a `recv` returning rather than a `recv_timeout` expiring.

**The licence sentence about clocks was deliberately not stretched to cover it.** That sentence is about a clock a gesture holds open, and this is not a clock at all. Reaching for it would have been the shape [#166](https://github.com/breferrari/vigia/issues/166) and §11.2 B10 both went wrong on from the other direction: citing I1 at a case its budget could never have measured.

**What it is, structurally, is the third sender becoming a fourth.** `crates/vigia/src/lib.rs` already describes the signal handler as *"a **third wake source on the same channel** rather than a new mechanism"*. This is the same move: `Highlighter::warm_ahead` takes a callback, the shell hands it one that sends `Wake::Warmed`, and the arm for it does **nothing**, because the paint after the batch is the whole response.

**The bound is `Shell::request_warm`, and it is one warm in flight.** A demand raised while a warm is running is not queued: the running warm ends with a wake, that wake paints, that paint raises the demand again if it is still true, and the next warm starts. The loop terminates because the warmer marks every grammar it *had a run at*, including one whose file had vanished by the time it opened it. Otherwise a hunk drawn from a diff whose file is gone would be demanded on every frame forever, which is a livelock with a wake attached; `a_path_that_vanished_does_not_leave_the_frame_asking` is that case, and it was watched failing.

---

## I7 — the residual table that made #51 decline more than it had to

Corrected 2026-08-21 with [#129](https://github.com/breferrari/vigia/issues/129).

**What #51 recorded.** Two rejections, both reasonable on their evidence: a per-grammar warmth predicate the frame path could act on, *"because compilation is per pattern: warming on one Rust file leaves a sibling paying 41.41ms, Markdown 95.04ms, HTML 201.20ms"*; and per-hunk deferral, *"the only exact fix"*, because it would add a colour lag to every scroll into new territory, thousands of times a session, to remove a cost paid once per grammar.

**The second reason still holds and was not relitigated.** #129 defers once per **grammar** per session, not once per hunk, so a scroll into new territory under a grammar already met parses inline exactly as it does today.

**The first reason rested on a number measured at the wrong scale.** Those residuals are **whole-file** parses, so each carries a large parse beside the compile it was meant to isolate, and a frame parses one screenful. Re-measured at frame scale, twenty-four lines, release, fresh `SyntaxSet` per case:

| | cold | after the warmer read one real 64KB sibling | floor |
|---|---|---|---|
| `.rs` | 123.98ms | **2.40ms** | 2.40ms |
| `.md` | 694.75ms | 89.77ms | 90.65ms |
| `.toml` | 15.26ms | 0.43ms | 0.40ms |

The middle column **is** the floor. The compile is fully paid by one real sibling, and what the old table was reporting was the cost of parsing another whole file.

**The half of #51's finding that survives, because it decides the implementation.** A *small* sibling is not enough: over a 2.5KB hand-written sample the residual is real, `.js` 80.49ms above floor, `.html` 40.10ms, `.cpp` 37.55ms. So the warmer reads `WARM_BYTES` of a real file, and a fixture would not have done.

**And the claim the frame path actually acts on is not the one #51 rejected.** *This grammar is warm* is unavailable at any price and nothing asserts it. *Nothing has ever parsed under this grammar* is exact: `syntect` holds every pattern in its own `OnceCell` and exposes no way to fill one but a parse, so the two places in `vigia-core` that build a `ParseState` are the whole population. Checked against the source rather than assumed: `SyntaxReference::contexts` is private, `ContextId`'s fields are `pub(crate)`, and `Regex::try_compile` compiles a throwaway rather than filling the set's own cell, so there is no eager path to reach for.

**The cliff is flat in content size**, which is the observation that separates a compile from a parse and which nothing had recorded: a 594-byte Markdown screenful costs **631.46ms cold and 0.97ms warm**, a 650x penalty on half a kilobyte.

**Why the population is warmed to three grammars and no further.** Measured over this repository, warming one grammar at a time and reading RSS after each: baseline **6.73 MiB**, ten grammars later **64.73 MiB**, with Rust +12.43 MiB and Markdown +19 to +35 MiB. I3's budget is drift rather than a plateau, so that is a bad trade rather than a breach. What a repository *leads* with is different in kind: the agent is near-certain to write the language the repository is mostly made of, so those megabytes are spent within seconds either way and the sweep only moves them earlier. The tail is the speculative part and the cap is what removes it.

**And what "leads with" counts is a grammar, which the index cannot see.** A tally of `.git/index` can only key on the extension, because §6 keeps `syntect` out of `worktree.rs`, and that proxy is sound for one path and unsound for the *selection*: a repository whose YAML is split evenly across `.yml` and `.yaml` has each spelling counted separately at the one moment the counts decide which three grammars are compiled, so it loses to a smaller single-extension language. The first implementation ranked and truncated in `worktree.rs` on the many-to-one-proxy argument, and the altitude review found it with no failing test behind it. The tally now comes back complete and unranked, the merge happens in `highlight.rs` where a `Scope` is available, and `a_language_spelled_two_ways_is_counted_once` is the gate, watched failing against the per-extension ranking.

**One finding turned up beside this one and is not it.** A 16.8KB Markdown screenful costs **117.00ms with every pattern already compiled**, which is the opposite shape to the cliff above: it tracks bytes on screen instead of being flat in them, and it survives every warm. That is a long-line parse cost, it is I2b and I4 territory rather than I7's, and it is [#261](https://github.com/breferrari/vigia/issues/261). **Closed 2026-08-22, and the shape recorded here is falsified.** Cost does *not* track bytes on screen: 24 empty lines inside a fence cost 25.3ms for 28 bytes of content, and the same 10,288 bytes reflowed from 24 long lines to 138 short ones cost 5.47ms against 5.85ms. What tracked was the number of **block starts**, because Markdown's block-start lookahead ran an exponential table-row test on lines that could never be table rows. See the I9 entries below, which are this finding's actual answer and supersede the mechanism guessed at here.

---

## I1 — a held mouse button is not an event, so a gesture-bounded clock is licensed

> [!IMPORTANT]
> **Reversed 2026-08-15, the same day it was written, and the entry is kept whole because the measurement in it is still the reason the feature is hard.** The first ruling refused hold-to-repeat. It was overruled as a product decision: a scrollbar button that does not repeat while held is not a scrollbar button, and every desktop toolkit has had this since the 1980s. What survives unchanged is everything below about the protocol, which is what any implementation has to work around. What changed is the conclusion, and `SPEC.md` §11.1 carries it: **the clock is allowed because it is bounded by the reader's finger.**
>
> The distinction the reversal turns on is the one the correction below had already found: I1's budget is *0 wakeups while **idle***, and a held mouse button is not idle. Every other timer this spec refuses would run while nothing is happening. This one cannot start on its own, cannot outlive the release, and `Held::wait` returns `None` with nothing held so the loop's receive is untimed exactly as before. **"Cannot outlive the release" was not true when it was written, and [#186](https://github.com/breferrari/vigia/issues/186) found out why**: a release is not the only way a gesture ends. A reader whose window loses focus while a button is down sends no `Up`, and `Held::ends` had no arm for `Event::FocusLost`, so the repeat went on stepping and repainting a pane nobody was looking at. On Windows the console has always delivered that event, so the hole was open there from the day the repeat shipped; on Unix it became reachable only when the takeover started asking for focus reporting. The condition is unchanged and the enumeration of what ends a gesture was short by one, which is the same shape as the amendment two paragraphs down: the argument proved something about *bounded* clocks and was written down as something about releases. I1's row now carries that qualifier rather than leaving it to be re-derived.
>
> Recorded rather than rewritten because a reader who finds this feature and wonders why it took a ruling deserves the protocol facts, and because the first draft's *reasoning* was wrong in a way worth keeping visible: it cited a budget that could never have caught the thing it was refusing.
>
> **Corrected again 2026-08-16, on the shape of the amendment rather than on the ruling.** The reversal was first written into I1 as an *exception*: *"the one clock this program owns runs only between a press on a scrollbar's step button and its release"*. That is the instance, not the rule, and an enumerated exception has two failure modes that both showed up immediately. It **blocks the next case even where the argument is identical**, and two were already in the tracker: a selection dragged past the edge of its region wants to scroll ([#177](https://github.com/breferrari/vigia/issues/177)), and a press held on the **track** is the page-repeat every desktop scrollbar has. Neither is a step button, both are clocks bounded by a gesture, and both would have needed their own reversal of a rule that had just been reversed. And it **disagreed with the code it was written for**: `Held` repeats whatever action it is armed with and `Action::repeated` is an exhaustive match specifically so a later held control inherits the mechanism, so the spec licensed one control while the code offered a facility. I1 now states three conditions (it may not start on its own, may not outlive the gesture that armed it, and the idle path must be untimed by construction) and the step buttons are an instance of them.
>
> The general lesson is worth more than the fix: **when a ruling is reversed under pressure, the amendment tends to be written as narrowly as the case that forced it**, because the case is what is in front of you. That is the moment to ask what the argument actually proved rather than what it was invoked for. Here the argument proved something about *bounded* clocks and was written down as something about *scrollbars*.


> [!NOTE]
> **Measured 2026-08-15 while building [#166](https://github.com/breferrari/vigia/issues/166).** The scrollbar's step buttons were asked for as ordinary buttons, and the first question a button raises is whether holding it repeats. It cannot, and the reason is not this program's design but the protocol's: there is no event to hang the repeat on.

**The mechanism, read from `crossterm`'s source rather than assumed.** `MouseEventKind` has exactly eight variants: `Down(button)`, `Up(button)`, `Drag(button)`, `Moved`, and the four `Scroll*`. There is no variant meaning *the button is still down*, and there is no lower layer carrying one either: the entry above records that the bundle sets `?1003h`, so the terminal is already reporting the most it reports, and `?1003h` is **motion**. A finger resting on a button with the pointer stationary produces the single `Down` and then nothing at all until the pointer moves or the button is released.

**So repeat has to come from a clock, and building one is what the reversal above authorises.** The only implementations available are a timer that fires while a flag says a button is held, or a loop that re-reads the button's state on a schedule, and both are a fixed cadence by another name.

**And which half of I1 refuses that is worth getting right, because the obvious citation is the wrong one.** The first draft of this entry said the clock is "what I1 exists to refuse", and I1's *budget* does not refuse it at all: **0 wakeups while idle**, measured over a sixty-second idle window. A timer that runs only while a button is held is not idle, nobody holds a mouse button through a sixty-second window, and the gate would therefore stay green whatever the timer did. That is the same structural blindness the entry above this one records for pointer motion, where the measure is *"silent here by construction"* — and a refusal resting on it would be checkable and wrong, which is worse than no citation. What actually reaches a held-button clock is **I1's first sentence**, which is a claim about mechanism rather than about idleness: *"Redraw is event-driven, never a fixed timer."* A repeat clock is a fixed timer producing redraws whoever is holding what, and no measure needs to catch it for the sentence to hold.

**This is not a correction to the phrase everywhere else it appears.** *"The timer I1 forbids"* is this repo's own shorthand and is used correctly in every other place it occurs: the pulse decay, the header's idle word, the memory readout, the poll loop `lib.rs` rejects and §10's highlight tail are all clocks that would run **while nothing is happening**, which is precisely the state the budget measures and precisely where it bites. A held-button repeat is the one instance where the clock is bounded by an active gesture, so it slips under the measure while still failing the sentence. Worth writing down because the shorthand is otherwise reliable, and a reader who has seen it used well five times will not stop to check the sixth.

§5.3 refuses the same thing one layer up for animation: *"snap, never ease."* A step button that repeats is an eased scroll with a mouse holding it. That half is a design ruling and needs no measure either.

**The first ruling made one step per click the affordance, on the grounds that there is no trick that is not a clock.** That half was right and is worth keeping: nobody should go looking for a protocol feature that would give repeat for free, because there is none. What the ruling got wrong was treating "it needs a clock" as the end of the argument rather than the beginning, when the question it should have asked next is *which* clock, and whether a clock bounded by a press is the thing I1's budget was written against. It is not. See the callout at the top of this entry.

**What the reversal did not change** is that the button is not the travel affordance. The wheel, `j`/`k`, `d`/`u`, `n`/`p`, the digits, `g`, `G` and a draggable thumb are, and the button is for the step none of them expresses with a pointer. That is why the repeat holds a constant rate instead of accelerating: acceleration serves travel and costs precision, and precision is what this control is for.

**The same finding decides the drag.** `input.rs` is a pure function of an event and a layout, with no state between calls, so it cannot know that a drag *began* on a button rather than on the thumb. Given that, a `Drag` over a button row has two candidate meanings and both are wrong: stepping makes a press-and-jiggle walk the view a row per twitch, and clamping to the end teleports it there. It is inert instead, which costs nothing real because the last track row already reaches the last window. Holding that reading stateless is what keeps the whole map a table test, and it is the reason the asymmetry between `Down` and `Drag` is a ruling rather than an oversight.

**What holds it** is `crates/vigia/tests/input.rs::a_drag_onto_a_step_button_is_inert`, which asserts the same cell answers a press and refuses a drag, so the two gestures are being told apart rather than the row being dead. The drag ruling survived the reversal untouched, because it never rested on the clock: it rests on `input.rs` having no state, and the repeat's state lives in the loop rather than in that module.

**What holds the reversal** is a different gate, and it is the one to break first if this is ever revisited: `nothing_held_means_no_timer_at_all` asserts that `Held::wait(None, _)` is `None`, which is what makes the loop's receive untimed on an idle monitor. Everything else about the repeat is a feel decision; that one is the invariant. A version that returned some large timeout instead would look harmless, pass every other gate in the file, and quietly put this program on a poll loop.

---

## I1 — the window ages, and the clock that ages it stops when the window empties

**Ruled 2026-08-22 for [#243](https://github.com/breferrari/vigia/issues/243), which parked itself as an I1 amendment and is one.** Asked for from use: *the graph should age*. `History::roll` has a single caller, the tick path, so a quiet worktree left the window frozen.

**The framing that carries it is not staleness.** The window's axis is time. A frozen window keeps its newest sample at the right edge, so a burst from ninety seconds ago draws as *just now*, and `Recency` freezes with it so a file that went quiet keeps its pulse. A monitor advertised as *correct with zero interaction* was incorrect with zero interaction. That is I5, and this is a case where two product-class claims pull against each other rather than a case where one of them is being spent for a convenience.

**Why it is the licence's own purpose rather than a widening of it.** The entry above records the distinction the first clock was admitted on: *every other timer this spec refuses would run while nothing is happening*. This one cannot. It runs while a burst is still decaying through the window, and the window empties `HISTORY_WINDOW` after the last write, so the state a monitor left open overnight is in is an empty window and an untimed wait. Measured rather than assumed: at `t+119s` the window holds one live sample and the path is tracked; at `t+120s` it holds none and the path is gone, because `roll` clears every track once the whole window has turned over.

**The second condition is restated, not dropped.** It read *"it may not outlive the gesture that armed it"*. `SCROLL_LINGER` is `now + 220ms`, so the direction arrows' clock has always outlived the gesture that armed it and nobody called that an amendment: the condition has meant a *bounded* outliving since the day it was written. What #243 changes is that the thing which arms a clock may be a change in the worktree as well as a reader's gesture, and the bound is the window rather than a release.

**Three measurements, because a ruling that touches I1 should carry them.** An ageing wake, in release, at the 256-path cap with a sample boundary crossed on every round: **165µs**, against I9's 16ms, which is 1% of one frame budget. An ordinary tick on the same fixture is **529µs**, and the difference is the status walk, which an ageing wake does not do because it is not a filesystem event. And the drain is bounded at `HISTORY_SAMPLES`, so ageing one burst to nothing costs about **19.8ms of CPU spread over two minutes**.

**Those figures are corrected from the ones this ruling was first written with**, and the correction is the reason the gate over them was rebuilt. They read 89.9µs against 458µs, measured on a twenty-path fixture whose rounds all landed inside one second: no sample boundary was crossed, so neither arm paid the projection an ageing wake actually causes, and the store held a twelfth of the paths the walk is priced at. The conclusion did not move and the margin narrowed from a fifth to a third.

**What was declined, and on which number.** A period derived from the drawn cell rather than from the store's sample: a band cell at 109 columns covers 1.1 seconds and a sparkline bucket covers five, so a coarser tick would skip wakes that change no pixel. It is refused because it would cap a cost measured at 165µs, and `CLAUDE.md` holds a cap to a refusal's bar. The period is `HISTORY_SAMPLE`, which is the finest interval at which any drawn cell can change.

**And what was declined on a reason rather than a number.** #243 proposed running the clock while the masthead is up, on the ground that `m` is a gesture. The sparklines and the pulse are drawn whether or not the masthead is and are on the same window, so a clock gated on the band would leave the two elements disagreeing about what time it is, which is the thing [#234](https://github.com/breferrari/vigia/issues/234) exists to forbid. One store, one roll, and coherence by construction.

**What it costs the pulse, recorded because a reader will notice it.** `Recency::Pulse` means a path was named by the newest burst and has ink in the newest sample, so with the window ageing on its own the mark now expires at the next sample boundary instead of surviving until something else is written. Its life is uniform on `(0, HISTORY_SAMPLE]`: half a second on average, and arbitrarily short for a write landing just before a boundary. Preferred to both alternatives. Not expiring at all is the freeze itself, and it drew a two-minute-old file at full brightness beside a nearly drained band. Giving the mark a duration of its own means a second clock per track running on wall time beside an element running on the sample grid, so the two would disagree about how long ago *now* was, which is what #234 forbids. The mark means *there is ink in the newest cell*, and the cell is a second wide. **The frame a burst causes always keeps its pulse**, held by construction: one instant per turn of the loop, read by both the tick and the roll, so a boundary cannot fall between a write and the paint it triggered. That was a real defect for one commit, found by two review agents on disjoint remits, and the parameter that closed it is the gate.

**What holds it** is `an_empty_window_and_nothing_held_means_no_timer_at_all`, written the way `nothing_held_means_no_timer_at_all` is: it asserts the *value* the loop's wait is given rather than a behaviour observed around it. A version that returned a large timeout for an empty window would look harmless and put an idle monitor on a poll loop.

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

---

## B10 — the terminal survey the hover reversal turned on

> [!NOTE]
> **Read 2026-08-16 while reversing [#123](https://github.com/breferrari/vigia/issues/123).** Filed under a B-number rather than an invariant, which is a first here, because what it holds up is a §11.2 ruling and not a row of §3. The decline's surviving reason was that *"nothing would ever tell a hover highlight to turn off"*, resting on the clause *"the takeover does not enable focus reporting"*. That is a claim about this repository's own `TAKEOVER` array written as though it were a claim about terminals, and both halves were checked. It sits at the end of the file rather than beside the I1 pair above so that *"`RULINGS.md`'s I1 section"*, which three call sites say, keeps meaning one contiguous thing.

**`crossterm` has shipped the mechanism for years.** `EnableFocusChange` writes `?1004h` and `DisableFocusChange` writes `?1004l`; `Event::FocusGained` and `Event::FocusLost` already exist and the keymap already answers both with `None`, which `crates/vigia/tests/input.rs::nothing_a_reader_did_not_ask_for_becomes_an_action` asserts by listing them among the inert events. So the missing piece was one step in `TAKEOVER`, never a capability.

**And it is portable by a route that is the *opposite* of the mouse bundle's**, which is the trap for anyone reasoning from `terminal.rs`'s module header. `EnableMouseCapture` overrides `is_ansi_code_supported` to `false` on Windows, so `execute!` diverts it to the console API and writes zero bytes. `EnableFocusChange` carries **no such override**, so on Windows with ANSI it really does emit `?1004h`; and where ANSI is unavailable its `execute_winapi` is a deliberate no-op, commented *"Focus events are always enabled on Windows"*, because `event/source/windows.rs` maps `InputRecord::FocusEvent` to the two events unasked. Two mouse-adjacent commands in one crate, two different platform stories. Do not generalise from the one this repo happened to take first.

**The Windows half is true and its obvious citation is a code comment, which is not evidence about Windows. Worse, Microsoft's own reference says the opposite**: `FOCUS_EVENT_RECORD` is documented as *"used internally and should be ignored"* with `bSetFocus` marked *"Reserved"*. What actually supports the claim is conhost: `microsoft/terminal`'s `InputBuffer::WriteFocusEvent` pushes a synthesized focus event unconditionally when the console is **not** in VT input mode, and `crossterm`'s raw mode clears only the line, echo and processed-input flags and never sets `ENABLE_VIRTUAL_TERMINAL_INPUT`, so this program is on exactly that branch. Recorded with the contradiction visible, because a reader who meets the Microsoft page first will otherwise conclude this entry is wrong, and *"the published doc is stale, here is the source that supersedes it"* is the finding rather than an embarrassment.

**No terminal that was checked is the gap, and the six are named because the count is the claim.** Alacritty was the one the reopening could not confirm and it does support the mode: its terminfo declares `XF, kxIN=\E[I, kxOUT=\E[O` and `alacritty_terminal` carries `NamedPrivateMode::ReportFocusInOut`. WezTerm **implements** it (`wezterm-escape-parser`'s `FocusTracking = 1004`), which is the citable form; its *docs* do not mention 1004 outside a 2020 changelog line that adds its own caveat, *"local (not multiplexer) terminal sessions"*. xterm originated it, in patch #224 of 2007. iTerm2 handles `case 1004` in `VT100Terminal.m` and lists focus reporting in its published feature spec. kitty defines `FOCUS_TRACKING (1004 << 5)` in `modes.h` and declares `XF` in its terminfo. So the evidence is source and terminfo for four of them and a published spec for two, which is worth saying because an earlier draft claimed all six were *"confirmed against their own specifications"* and two of them publish none.

**And the list stops there on purpose.** §11.1's colour ladder already rules that a terminal list is *"evidence about terminals someone checked rather than a claim about the ones nobody has"*, and the first draft of the ruling said *every tier-1 terminal implements the mode* — a quantifier over a set this repository has never defined, since "tier-1" here means a build target. Terminal.app was not checked and is the nearest unexamined case, given §11.1 already singles `Apple_Terminal` out as the entry that breaks the colour table; no `nsterm` entry in ncurses' terminfo uses `xterm+focus`. ncurses also records terminals that implement the mode *badly* (its own comment: *"Some terminal emulators implement xterm focus in/out, but do it incorrectly, interfering with user applications"*, with notes against xterm.js, mlterm and st). None of that changes the ruling, because the ladder's bottom rung is the residual and these terminals land on it; it changes what may be *said*.

**The Alacritty changelog silence is not explained, and the explanation first offered was wrong.** The draft said *"a feature present since before a changelog started is invisible in it"*. That does not fit: Alacritty's changelog begins in 2018, and the terminfo capabilities cited above were added in 2023 and are unmentioned in it. The useful half survives without the mechanism: **an absence in a changelog is not evidence about a feature**, and treating it as evidence is what nearly kept Alacritty on the unsupported list.

**The gap is a multiplexer default.** tmux's `focus-events` defaults to **off** (`options-table.c`, `.default_num = 0`), and `tty.c::tty_update_features` gates the enable sequence on that option, which is the direct evidence that under a default tmux the outer terminal's focus reports are neither requested nor forwarded. That is not every reader: §1 asks for a pane beside an agent and names no multiplexer, so a terminal's own split has the rung and tmux without that setting does not.

**A second tmux default may be larger than this one, and it is a question about the mouse this program already has rather than about hover.** Read in `server-client.c`: `server_client_reset_state` takes its mode from the **active** pane, and the loop that unions `MODE_MOUSE_ALL` across every pane sits inside a guard on the session's `mouse` option, which itself defaults to off (`tmux.h`, `TMUX_MOUSE 0`). If that reading is right, then under a default tmux a pane that is not the active one never has `?1003h` requested on its behalf, so `vigia` sitting beside the agent the reader is typing in would receive no motion at all: no wheel, no click, no hover. **This was read from source and not reproduced**, one `tmux new-session` would settle it, and it is [#188](https://github.com/breferrari/vigia/issues/188) rather than a sentence in a ruling, because it is a claim about shipped behaviour that predates B10 and is not this ruling's to assert.

**One citation is weaker than it looks and is stated at its real strength.** tmux issue 4909 reports terminal-level focus-out being absorbed rather than relayed with several panes and `focus-events on`. It was reported against 3.4 and the reporter said it reproduced on master; it was **closed for want of requested logs, never diagnosed and never reproduced by a maintainer**. So it is a report and not evidence, and it is kept only because a reader following the link would otherwise find a closed issue and quietly downgrade the whole survey.

**What that changed in the ruling, stated so nobody re-derives it as an objection.** It did not restore the decline: inside the pane `?1003h` reports at cell granularity and retires the mark without any of this. It bounded the residual to one case (the pointer leaves the pane with the window still focused, on an idle tree) and made that case *ordinary* for one common setup rather than exotic, which is what forced the constraint the ruling turns on: the mark must be quiet enough that a stale one costs nothing. A survey that had come back clean would have produced a weaker ruling.

## B10 — what the mark is drawn in, and the contradiction it shipped with

> [!NOTE]
> **Read 2026-08-16 while ruling [#193](https://github.com/breferrari/vigia/issues/193).** The section above is about whether a hover mark can be *cleared*. This one is about what it is *drawn in*, which the adoption pass got wrong in a way no gate could see, and it is here rather than in `SPEC.md` because the contrast numbers date.

**§5.3 shipped two sentences that contradict each other, one day apart in the same section.** B10's derivation rules that a mark about an input device *"must be the quietest thing still visible in that region"*, and gives the reason: *"a glance has to reach the worktree first and the pointer never"*, because the mark can go stale where a recency cannot. The colour paragraph two below it ruled `Theme::path_hover` to sit *"above all three"* recency weights, *"brighter than `Theme::path`'s pulse weight"*. The shell implemented the second, so the pane's most perishable claim was its loudest text. Nothing failed. Every gate over it asserted **separation** from the recency ladder, which the loud form satisfies perfectly, so the defect was invisible to eleven green assertions and visible to one reader looking at the screen.

**The correction keeps the half that had a reason and drops the half that had a placement.** Quietness is derived from staleness; *above all three* was derived from anti-collision, and anti-collision was never being done by the brightness. `SPEC.md` §5.3 already named the real channel in the same paragraph: *"it underlines, which is what keeps the two apart where colour runs out"*, and observed that on `ansi` the brightest path *"has nowhere further to go"*. That observation was treated as a difficulty the brightness had to survive; it is the proof the brightness was not load-bearing.

**The value is `Theme::bar_hover`'s, so the pointer reads as one mark rather than as two.** A step button, a thumb and a listed path are three surfaces one gesture crosses, and until this they answered it in two different visual languages.

**Measured rather than adjusted by eye**, on the same instrument `tests/palette.rs` uses:

| palette | value | against the pane | for comparison |
|---|---|---|---|
| `dark` | `#a8b1bb` | **8.71:1** on `#0d1117` | `path_live`'s `#e6edf3` is brighter, `path_cold`'s `#7d8590` dimmer |
| `light` | `#3d4650` | **9.59:1** on white | quieter here means *lighter* than `path`'s `#1f2328`, which is the same rule pointed the other way |
| `ansi` | `Gray` | not measurable, and that is the point | it **equals** `path_cold`'s foreground, so the underline is the whole separation |

The `ansi` row is the one to read twice. Sixteen names hold nothing between colour 8 and `Gray`, so a quiet mark on that palette is the cold rung's colour exactly, and a reader hovering an already-cold file sees only the underline change. That is a real narrowing and it is accepted rather than hidden: it is the case §5.3 nominates the underline for, and the alternative is the loud form that ranked the pointer above the worktree.

**The lesson is the same one B10 has now taught three times**, and it is worth the third telling because the shape changed: the first two were a *reason* that expired, and this is two reasons that never agreed. A section can be internally inconsistent and every test still pass, because tests assert against the implementation and the implementation can only follow one of the sentences. **Where a document rules twice on one thing, the second ruling is not a restatement and should be read as a claim.**

## I9 — #261's stated cause was wrong in every part, and what it cost to find out

Recorded 2026-08-22, closing [#261](https://github.com/breferrari/vigia/issues/261). It is here rather than only in the issue because the wrong explanations are plausible, they were each believed for a while, and the next reader who meets a slow Markdown frame will reach for them in roughly this order.

**What was claimed.** A screenful of this repository's prose cost 117ms fully warm. The issue was filed undiagnosed on purpose, with a guess attached: the prose here is one line per paragraph, Markdown parses at roughly 7ms/KB against Rust's 2ms/KB, and a 24-line screenful of `SPEC.md` is therefore 16.8KB of genuine work. The suggested remedy was a bound on what a frame parses.

**What was true.** Bytes do not predict the cost at all. Twenty-four *empty* lines inside a fence cost 25.3ms for 28 bytes of content; the same 10,288 bytes of `SPEC.md` reflowed from 24 long lines to 138 short ones cost 5.47ms against 5.85ms, so a display-line bound would not have helped either; and per-byte cost across this repository's own Markdown varies 100x line to line. "7ms/KB" is not a rate. The real mechanism is Markdown's block-start lookahead, whose last alternative tests for a table row: both of its branches require a literal `|`, so a line without one can never match, and the engine established that by exploring the embedded inline-content alternation first, at roughly 4x per code span until `fancy-regex`'s backtrack limit truncated it.

**The one thing the title got right for the wrong reason.** One-line-paragraph prose really is the shape that hurts, and not because of line length: Markdown runs the block-start lookahead only on a block's **first** line, so a continuous paragraph pays once and a screenful of one-line paragraphs pays every row. This was measured while building the gate, and it is why the gate's fixture separates its lines with blank lines. Eleven rows of one continuous paragraph measured **cheaper in the frame (15.30ms) than a single row of the same content parsed alone (16.88ms)**, which is the tell that ten of them never reached the pattern. A fixture written the obvious way would have been green against the very defect it was built for, and was, twice.

**Four mechanisms that were proposed and are refuted.** Re-running them is pure cost.

| proposed | refuted by |
|---|---|
| Bound the parse per frame, leave the tail for the next | Never converges under editing: at roughly 8ms a fenced line it takes about 24 self-driven frames to colour one screenful, repainting continuously. `Shell::draw`'s docblock records the `while` that spun at 100% CPU |
| Rewrite the code-span sub-pattern, which appears about twelve times in the alternation | Ablated: 13.00ms against 12.83ms. Not the blowup |
| Use Sublime's `branch_point` / `branch` / `fail` to cut the search | `syntect` implements none of it and ignores the keys silently |
| Switch to `syntect`'s default syntax set, which looks 8x faster | It does not highlight embedded code at all: 0 shell scopes inside a ```` ```sh ```` fence against 26 for ours |

**And one instrument trap worth more than the fix.** The first three hours went into a scratch crate that did not match this workspace's `[profile.release]`. Identical code, identical dump and identical content measured **13x apart** from the real thing. §7 already says an absolute gate on a shared machine is a weak instrument; this is the same lesson one level lower down, where the *build* rather than the machine is what makes a number meaningless.

## I9 — a profile is shared by four targets, and this one was tuned by accident

Same pass. `codegen-units = 1` had been in `[profile.release]` since the budgets were first written, on the ordinary reasoning that fewer codegen units optimise harder.

**On Windows it was making `fancy-regex` compilation roughly 6x slower**, and because `syntect` compiles patterns lazily on first use, that landed on frames a reader was waiting for: a 24-line `sh` fenced block cost 286.91ms of parse at 1 and 22.39ms at 2. It is a cliff at 1, not a gradient, and `lto` is irrelevant either way.

**On Linux it does nothing at all**, which is the part worth recording. Re-measured 2026-08-22 interleaved over three rounds against three separately built binaries, `codegen-units` 1, 2 and 16 sit within 3% on every fixture, and the cold parse (which is where a compile would show) is 11.999ms at 1 against 12.113ms at 2, within 1%. The toggle was applying: the binary went 3,493,720 to 3,604,896 to 4,077,632 bytes. macOS has never been measured by anyone.

Two lessons, and the second is the general one:

- **A number measured on one target is a claim about that target.** The whole of the original write-up was going to be entered as a property of the profile, and it would have been wrong on two of the three tier-1 targets. `SPEC.md` §9 ships four; a `[profile.release]` key is shared by all of them and a measurement is not.
- **A gate calibrated against a platform-specific artefact encodes that artefact as a requirement.** `warm.rs` asserted a 10x cold-to-warm ratio, sized against a Windows cold parse that was mostly the codegen penalty. The first time the suite ran on Linux it failed, at 5.70x, and it failed identically at `codegen-units` 1 and 2, so it had never been this platform's number: nothing had ever run it here. Lowering the constant would have kept the shape and moved the edge. It now asserts what `warm` actually claims, in absolute terms that no codegen setting can re-invalidate: the warmed parse fits inside a frame, and warming removed a frame's worth of work from the one behind it.

## B13 — the sheet's height axis dropped gestures in silence, and the width axis still can

The ruling is in `SPEC.md` §11.2 B13 and what the shell does is §11.1. This is the
measurement it rests on, kept here because a number in a ruling is a claim and a
number here is a trail.

**Before, on `main` at 0.25.0.** Every width from 20 to 140 swept against every
height from 3 to 40, counting the gestures actually painted inside the sheet's own
rect rather than anywhere on the pane:

| pane width | most gestures reachable, at any height |
|---|---|
| 24 to 25 | 3 of 16 |
| 26 to 31 | 4 to 5 of 16 |
| 32 to 34 | 9 of 16 |
| 35 to 44 | 11 of 16 |
| 45 and up | 16 of 16 |

Two things that table says and [#286](https://github.com/breferrari/vigia/issues/286)'s
own did not. The height floor drew **4** of 16 rather than 3, because `SHEET_KEEP`
is a keep-count and the floor rung had room for one more than it. And the residual
was as much **width** as height: at 40 columns, the width I6 is named for, the
ceiling was 11 at every height, and the five missing were the whole mouse group.
No pane height reached them, because the tight one-column sheet with the mouse
group was 43 columns wide and a 40 column pane has 40 to give.

**The one string that made it 43.** `MOUSE`'s tight verbs topped out at
`scroll what you point at` (24) and `one row, repeats held` (21); the keyboard
group's topped out at 18, and keys at `click a track` (13). So the table's verb
field was the wheel's alone. At 17 and 19 the field is 19, the sheet is
`13 + 2 + 19 + 4 = 38`, and 40 columns of room takes it with two to spare.

**After.** Every pane of 38 columns and up reaches all sixteen, at every height
that draws a sheet at all. 35 to 37 reach eleven, 32 to 34 eight, 30 to 31 four,
and below 30 nothing is drawn. The narrowest sheet the ladder draws went from 24
columns to 30, because every rung charges the page counter's widest spelling so the
ordinals can never run into the close control.

**And that reason is the second one this ledger has recorded for the same charge.**
The first was that it keeps a centred box the same size between pages. A mutation
removing the charge left `the_box_does_not_resize_between_pages` green and reddened
two width gates, which is the opposite of what the claim predicted: `sheet_fields`
measures over the whole row set and every page of a pane shares that set, so the
width was page-independent already. Both claims are about the same line and only
one of them is true.

**The two-column rung moved with the copy and the plan did not predict it.**
`sheet_beside` measures the same mouse cells, so the tight rung went 76 to 71 and
its arrival 78 to 73. Additive: the block of panes between 73 and 77 columns drew
eleven gestures and now draws sixteen on one page. Recorded as a deviation rather
than folded in, because a number that moves without being predicted is the thing
this ledger exists to catch.

**What the counter cost, found by a gate rather than by reading.** The first
`sheet_counter_floor` asked the formatter for `(16, 16)` and got the *short*
spelling, ten columns rather than thirteen, because that pair is the one case the
range form never draws. Every rung was then three columns narrower than the counter
it had to fit and the sheet drew at 27 where the ruling says 30. The fix is a
maximum over the pairs the planner can actually return, taken once per process.
Deriving the width arithmetically instead would have been the same defect one layer
over: two expressions agreeing about a sum by hand.
