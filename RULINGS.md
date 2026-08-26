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

**What it costs the pulse, recorded because a reader will notice it.** `Recency::Pulse` means a path was named by the newest burst and has ink in the newest sample, so with the window ageing on its own the mark now expires at the next sample boundary instead of surviving until something else is written. Its life is uniform on `(0, HISTORY_SAMPLE]`: half a second on average, and arbitrarily short for a write landing just before a boundary. Preferred to both alternatives. **Amended 2026-08-25 by [#313](https://github.com/breferrari/vigia/issues/313), and it is the "arbitrarily short" clause that was wrong rather than the shape.** Reported from a live pane as the dot no longer showing up. Measured across the grid: a write at +0ms into a sample pulsed for 1s, at +500ms for 500ms, at **+990ms for 10ms** and at **+999ms for 5ms**. An agent saving files continuously lands wherever it lands, so the mark was a coin toss and the reader was losing it — and *"arbitrarily short"* was written down here as a cost that had been weighed, when what it actually described was the element failing to exist about half the time. **The mark now survives the newest `PULSE_SAMPLES` samples rather than the newest one**, so its life is `[HISTORY_SAMPLE, PULSE_SAMPLES x HISTORY_SAMPLE]`, closed at both ends and the *worst* case is what the best case used to be. **What is untouched is this paragraph's refusal of a duration**, which was and is the right refusal: a sample count needs no second clock, no wall time beside the grid, and no wake that was not already going to happen — the roll that ages the window is still the only thing that retires the mark. What moved is a count, not a mechanism. Not expiring at all is the freeze itself, and it drew a two-minute-old file at full brightness beside a nearly drained band. Giving the mark a duration of its own means a second clock per track running on wall time beside an element running on the sample grid, so the two would disagree about how long ago *now* was, which is what #234 forbids. The mark means *there is ink in the newest cells*, and since the amendment above there are `PULSE_SAMPLES` of them at a second each. **The frame a burst causes always keeps its pulse**, held by construction: one instant per turn of the loop, read by both the tick and the roll, so a boundary cannot fall between a write and the paint it triggered. That was a real defect for one commit, found by two review agents on disjoint remits, and the parameter that closed it is the gate.

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
eleven gestures and drew sixteen on one page after it. (**B13's counts throughout
this section are B13's own and are not current**: `r` and then `s` were added
after it and every one of them moved. The current ones are in `SPEC.md` §11.1.)
Recorded as a deviation rather
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

## B13 — what the audit found that the ruling had shipped

Both defects are B13's own, both were introduced by the change that made the
sheet page, and neither was visible to the suite that shipped it.

**The close control advanced.** A click on `✕` returned `Action::ToggleSheet`,
which is the action `?` sends, and once `?` meant *advance* the control did too.
On a six-page pane a reader needed six clicks to leave, and the pointer has no `?`
to fall back on. `SPEC.md` §11.1 and `Action::ToggleSheet`'s own docblock both
stated the opposite while it did this, which makes it the fourth false claim in
this element's documentation inside one pass.

The gate that should have caught it is worth recording precisely, because it looks
adequate. `the_close_control_dismisses_and_the_sheet_swallows_the_rest` asserts
that `action_for` on the control's cell returns the dismissing action, on an
eighty by twenty-four pane. Two things make that unable to fail here: the pane is
**one page**, so there is nothing to advance to and the two actions are
indistinguishable on it; and the test asserts the action's **identity** and never
applies it, so what the action does to the state is not in the assertion at all.
An identity is not an outcome.

**The last page's box moved.** `paged_fit` sized the frame from `take`, which is a
remainder on the last page, and `sheet_plan` centres the box on its height, so the
final page shrank by the remainder and slid down half of it. The close control went
with it, and the row it vacated fell through to a scrollbar the reader could not
see. The box is `capacity + SHEET_FRAME` on every page now, with the tail blank
inside the frame.

`the_box_does_not_resize_between_pages` recorded `(left, width)` and those are
exactly the two edges that did not move. It now records all four, and a second gate
covers the tail's own frame: `the_sheet_is_a_closed_box_at_every_rung` sweeps 3,400
panes and reads **page one** on every one of them, because its scaffold toggles
once and paints, so the blank tail is outside its reach by construction.

**Twenty mutations, twenty killed.** Four of them are the four above and the
fixes' own gates; the rest cover the ladder, the clamp, the counter, the drop
order and the drain. Two survived their first run and both were instrument
failures rather than gaps: one ran against a test binary that did not contain the
test, and one applied to a file a previous iteration's `git checkout` had already
reverted. A mutation that never applied and a mutation the suite failed to kill
report identically, and they call for opposite responses.

## B14 — the rail arrived on its own, and one number is the whole argument

The ruling is `SPEC.md` §11.2 B14 and what the shell does is §11.1. This is the
trade it rests on, kept here because the reversal is narrow and the part that is
*not* reversed is the part most likely to be re-argued.

**What was reversed.** Not the width. [#252](https://github.com/breferrari/vigia/issues/252)
derived 134 rather than choosing it, and that derivation stands: both regions read
one glance ladder, so splitting a pane costs each half the width the whole had, and
a split costs no rung only where both halves and the undivided pane one column
below sit on the same plateau. There are two plateaus and the other needs a
328-column pane, so 134 is the only answer. What was reversed is that **crossing it
was automatic**.

**The number that decides it, and §11.1 already stated it.** At 133 the diff plans
against 129 columns; at 134 against 60. Widening a terminal past a threshold nobody
chose more than halved the region this tool exists to show. §11.1 called that "the
feature rather than a defect", and it is, *for a reader who asked*. The same
sentence describing a reader who did not is the reason this reopened.

**Why an opt-out was rejected rather than an opt-in.** Both need the same discovery
path: the gestures sheet names the key either way. Given that, the question is only
which default a reader who never opens the sheet gets, and the answer is the one
that changes nothing.

**And the picture stopped being an exception.** `assets/preview.svg` is a
109-column render, so §5.1 could only say the picture and the code "describe the
same pane" by noting the picture sits below the arrival width. With the rail asked
for, they describe the same pane at every width.

**What it cost, which is the sheet and not the pane.** A key is a row, so the
gestures table went from eleven keyboard rows to twelve and every row count in
§11.1 moved: the one-column rung to eighteen table lines in a twenty-row box, the two-column rung to `104 x
15` and `71 x 15`, the roomy rung to `68 x 30`. (**Those numbers are B14's own and
are not current**: B15 and then B16 moved them again. The current ones are in
§11.1.) **No width moved**, and that is what
kept this one issue rather than two: `r`'s cells are `r` and `show or hide the left
rail` (25 columns) or `the left rail` (13), inside the existing maxima of 22 and 28
wide, 13 and 18 tight. Every prediction in the plan held, which is worth recording
because the plan made them before the run rather than after.

**The keep-set did not move, and the reason first written for that was false.**
`r` is a fourth gesture a reader cannot guess at, beside `f`, `m` and `?`, and
`SHEET_KEEP` keeps three, so one of the four has to go first. It is given up at
rank eight of `DROP_ORDER`, two before `f`, with `s` between them since B16.

The reason recorded at the time was that the drop order binds at 30 to 34 columns
and a rail needs 134, so `r` could not fire on the pane dropping it. **That
describes a pane that does not exist.** At 30 to 34 columns the rung is `from = 7`
and `r` is *kept*, which this repository's own `NARROW` table asserts by name. The
rank that drops it is `from >= 9`, which needs a width below thirty, and below
thirty no sheet is drawn at all.

So the reorder is **unreachable on every pane that draws**, and it is a defensive
ordering of the tables rather than an observable behaviour: `sheet_tables` asserts
the keep-set is `f`, `m` and `?`, the untouched order would have dropped `f`
instead, and if a rung ever reaches that depth `r` is the right one to lose because
it is the only one of the four that needs 134 columns. Found by the audit, which is
the fifth false claim this element's documentation has produced in two passes and
the second where the sentence was checkable against a table in the same repository.

**One instrument note, from the pass rather than the ruling.** `cargo test
--workspace` stops at the first failing binary, so a grep for `FAILED` over its
output reports only that binary's failures and reads as green once it passes. Two
counts in this pass were taken that way and both were wrong: the first was a
*compile* failure with no `FAILED` line at all, and the second hid twenty-nine
failures in later binaries. `--no-fail-fast` is the flag, and a count that cannot
see a compile error is not a count.

## B15 — the arrows were free, and the only thing they cost is a spelling on the sheet

The ruling is `SPEC.md` §11.2 B15 and the behaviour is §11.1. This is the trade,
and the correction that came with it.

**A claim in the issue's own body was false, and it was mine.** [#296](https://github.com/breferrari/vigia/issues/296)
was filed on 2026-08-24 saying [#272](https://github.com/breferrari/vigia/issues/272)
*"would want those arrows if horizontal reading ever lands"*, and offering that as
the thing to rule against. #272 asks for **`w`**, a wrap toggle, and needs no arrows
at all. The real conflict is with a **horizontal pan**, which §11.1 declined in the
sentence after the one ruling a long line clipped rather than wrapped. (The first
draft of this section said "the same sentence"; it is the adjacent one, and being
precise about that is cheaper than being caught being loose about it.) So the arrows
were contested by a rejected alternative rather than by an open row, and the ruling
is cheaper than the issue that asked for it. Recorded because the pattern is now
familiar here: a premise written into an issue reads as settled the next day, and
the issue's author is the least likely person to re-check it.

**The one measured cost is the tight spelling of one keys cell.** The gestures
sheet's tight keyboard keys field is eleven columns, on `Space  PgDn`. The arrowed
cell `n  →  /  p  ←` is thirteen. Carrying it at the tight spelling would take the
keyboard-only rung from **35 columns to 37**, so panes of 35 and 36 would fall to
the next rung down and lose their twelve gestures. The whole-table rung is unmoved
either way, because the mouse group's `click a track` is already thirteen.

Two columns of pane losing gestures to an **alias** is the wrong trade, so the
arrows are named at the wide spelling only. That is also the established
convention: `q  Esc  Ctrl+C  Ctrl+D` becomes `q  Esc` and `g  Home  /  G  End`
becomes `g  /  G`. `j  k  ↓  ↑` is the exception and it keeps its arrows because
there they cost nothing, which is the same test applied and answered differently.

**No row was added, and that is why this diff is small where #295's was large.**
An alias goes in an existing cell, so `KEYBOARD` was still twelve rows, the counter
still counted to seventeen, and every rung height and reachability boundary §11.1
states was untouched. #295 added a row and moved every one of them. (**Those
numbers are B15's own and are not current**: B16 added `s` the next day and moved
them again. The current ones are in §11.1.)

## B16 — the pin makes the frame path cheaper, and the guard that would have made it dearer

The ruling is `SPEC.md` §11.2 B16 and the behaviour is §11.1. This is the
archaeology: one defect found by reading, one premise checked instead of
inherited, and what a thirteenth key did to the sheet.

**The defect is a literal that stopped meaning what it says.** `View::collect`
backs a short screen up so the diff's last row rests on the bottom, and it skips
that when the position is already the first one the walk can reach. That test is
spelled `view.top != Position::default()`, and `Position::default()` is *file
zero, row zero*. It has been correct for as long as the walk always started at
the first changed file. Under a pin it does not: the first position a pinned walk
can reach is the pinned file's own row zero, so on any pinned file but the first
the guard cannot fire, and a pinned file shorter than the pane is `short` on every
frame, restarts on every frame, and pays what that guard's own paragraph records
at **three walks and six `Frame::diff` calls a frame against two** — on the file
an agent is writing to, which `Frame::diff` re-reads inside the settle margin by
design.

Both walks resolve to the same position and draw the same rows. Nothing on screen
can see it, no snapshot moves, and the only instrument is the frame's own read
count, which is what `tests/single.rs::a_pinned_file_shorter_than_the_pane_walks_once`
asserts. It was found by reading the guard's docblock while working out what the
pin had to bound, which is the cheapest way this could have been found and was
luck rather than method: nothing would have gone red.

**The generalisable half**: a bound written as a type's `default()` reads as *the
floor* and means *the floor of the old walk*. When a feature narrows what a walk
may reach, every such literal is part of the change, and the ones that are
`Default::default()` are the ones no reviewer looks twice at.

**The premise checked rather than inherited is B14's.** That ruling ranks `r`
outside the sheet's keep-set and its **first** stated reason was false: it said
`r` cannot fire on the pane that drops it, and no drawable pane drops it at all.
The correction is in B14. Ranking `s` at nine raised exactly the same question, and
the answer was taken from drawn output rather than from B14's conclusion: swept
over every width from 20 to 45, the deepest rung a drawable pane reaches is still
`from = 7`, so the `NARROW` table now shows both `r` and `s` **kept** at thirty
columns, and both reorders remain defence rather than behaviour.

**What a key costs, measured twice now.** `r` and `s` are the same shape of change
and the same shape of cost: every row count moves and no width does. The four
reachability boundaries sit at 30, 32, 35 and 38 columns and have not moved
through either, and the counts behind them went 16, 11, 8, 4 to 17, 12, 9, 5 to
18, 13, 10, 6. They were re-derived from a swept pane both times rather than
incremented, which is the only way a boundary that *did* move would be noticed.

**The one number that goes the other way.** Every feature added to this pane so
far has cost the frame path something. A pin removes I4's single exception from
the frames it is on: the diff's height is counted for every changed file once per
tick and is the only thing in the frame path not bounded by the window, and a
pinned frame reads its total off the pinned file's span instead. The gate asserts
the **unpinned** frame counted something before it asserts the pinned frame
counted nothing, because a zero over a fixture that had nothing to count is not
evidence, and that is the same two-fixture rule §7 states for every other cost
claim here.

## B16 — the audit: three gates that were green with the feature deleted

The ruling is `SPEC.md` §11.2 B16. This is what its own audit found, and the
shape is worth more than any single finding: **every serious one was in the
tests rather than in the code.**

**Three gates passed against a `vigia` with the pin removed**, and each was
vacuous for its own reason, which is why noticing one would not have found the
others.

- *The toggle gate discarded the middle draw.* It pressed `s`, wrote
  `let _ = draw(...)`, pressed `s` again and compared the ends. That asserts only
  that doing nothing twice does nothing. It observes the pinned screen now.
- *The follow gate followed the last file.* Nothing comes after the last file, so
  a screen resting in it draws one file whether or not anything is pinned. It
  follows a middle file now, and scrolls off its end before it looks.
- *The file-changing gate never left row zero.* `n`, `p`, a digit and a click all
  land on a heading, and every file in the fixture is taller than the body, so
  one file is drawn either way. It scrolls into the new file before it looks.

**And a fourth gap was worse, because nothing covered it at all: `s` was never
proven bound to a key.** Deleting the `KeyCode::Char('s')` arm left the whole
workspace green, since every gate constructed `Action::ToggleSingle` directly.
B16 could have shipped a gesture no keyboard could reach. [#295](https://github.com/breferrari/vigia/issues/295)
closed exactly that hole for `r` with a gate of its own and the lesson did not
travel; `r` was also missing from `every_key_the_map_binds_is_named_on_the_sheet`,
whose hand-written list is [#288](https://github.com/breferrari/vigia/issues/288)'s
to fix, and both are in it now.

**The one behavioural defect the audit found is an input defect, not a drawing
one.** The shell drains actions in a batch and paints once at the end of it, so
`G` and a held `k` arrive together with no frame between them. `G` under a pin
wrote the pinned file's whole height and let `View::collect` clamp it on the way
to the screen: the right rows are drawn, and the *position* left behind is one
nothing can move from, so every `k` in the same batch walked the row down and
every one of them clamped to the same screen. Nine keystrokes swallowed on a
22-row file at a 13-row body. Unpinned the case cannot arise, because `G` there
resolves to row zero. `Action::Bottom` writes the resting row now, and pays for
it with a staleness correction it used to get from `collect`'s clamp for free:
a file that grew since the bar was drawn rests slightly short of its true bottom
for one tick, which is invisible and self-correcting where swallowed keystrokes
are neither.

**`View::fits` was deleted rather than fixed**, and the reason is the same
literal the defect above the fold is about. It compared `top == Position::default()`,
which under a pin can never hold, so it would have claimed every pinned file
overflows its pane. It was the **third** instance of that bound in this file and
the only one with no caller anywhere in the workspace: the other two were fixed
because something depended on them. A dead function that this change makes wrong
is the clearest case for removal there is, and keeping a public one alive to
avoid an API break in a library that exists to serve one binary is the worse
trade. The alternative, giving `View` a `first` field so the function could be
made correct, is adding surface to justify surface.

**The stale-number sweep found nine more in docblocks the diff never touched.**
`render.rs` and `tests/sheet.rs` carry the sheet's dimensions in prose beside the
code that draws it, and the diff re-derived every number it *stated* while
leaving the ones it merely *passed*. One was a live literal rather than prose:
`the_floor_is_a_rung_now...` computed the title bar's floor from `" 16-16 of 16 "`,
which has the same character count as `" 18-18 of 18 "`, so it went on computing
thirty and going green while describing a table two rows smaller than the one it
was measuring. That is the same shape as the counter's own reason for being
`KEYBOARD.len() + MOUSE.len()` rather than a literal, one layer down in the gate.

## B16 — round two: the fix that never reached the shell

The three round-one fixes were the round-two suspects, and one of them had not
been a fix at all.

**`Action::needs_height` classified `Bottom` as reading no height**, which was
true for as long as `G` was a jump to a heading. B16 made it rest the pinned
file's last row on the bottom, and *the bottom* is a height. `crate::run` hands a
zero to anything the predicate calls false, so `span.saturating_sub(0)` is the
whole span, and the swallowed-keystroke defect the ruling above records as fixed
was still shipping in the binary while the branch's own gate went green.

**The gate went green because a test calls `App::apply` with a height a shell
never passes.** That is the coverage-shape failure exactly: the suite modelled
the function, not the program. `only_the_action_that_reads_the_height_is_given_one`
was written for precisely this class and was blind twice over — every `App` in it
was unpinned, which is the one state where `Bottom` cannot read a height, and it
compared `View::top`, which is the position *after* the walk clamps, so two
different requests that draw one screen looked identical. It drives both
configurations now and reads the position the shell keeps.

**A drag under way was handed a zero too**, and that one predates the pin: the
first event of a gesture resolved through `action_for` with a real height and
every motion after it resolved without one, which maps the track onto the whole
rather than onto travel. The two ends of a track agree under both arithmetics and
only the middle does not, which is the same reason the row-exact bar's own
regression hid. There is one expression deciding the height now, `Shell::diff_rows_for`,
because the wiring inside the takeover loop is not reachable by any test: the
answer to an untestable call site is to leave one place that can be wrong rather
than two that can disagree. A mutation emptying that function survives, and it is
recorded here rather than papered over.

**And a latent panic that a sentence of mine was hiding.** `App::up` walks back a
file at a time asking each how tall it is, through `Frame::diff`, which indexes
the changed-file list directly. A position is the index that outlives the list it
was resolved against, so a tick carrying an agent's commit and a wheel-up in the
same drained batch reach it with no paint in between. It is pre-existing, and what
kept it unfound is that `App::pinned_file`'s docblock asserted its two callers
were *the only* gestures reaching the frame ahead of the walk. They were not.
**A docblock that overstates a guarantee is worse than none**, because it answers
the question a reader would otherwise have gone and checked, and this is the
second one in the same area on this branch: the first said `span_in` cost a
`stat`, and it cost a whole-file diff.

## B16 — round three: two of three call sites, and a claim of coverage nothing enforced

Round three found no blocker and nothing a reader could see, which is what an
audit converging looks like. What it found instead is the same defect one layer
further out, twice, and the pattern across the three rounds is worth more than any
of them individually.

**Round one found gates that were green with the feature deleted. Round two found
that a round-one fix never reached the shell. Round three found that round two's
fix reached two of the three call sites.** The third is the held-repeat path, and
it passed a literal zero for a height. It is benign today, because
`Regions::step_at` yields only `Scroll` and `ScrollList` and neither reads a
height, so the literal was right by accident rather than by rule. Every one of
those three findings lives inside `crate::run`, the loop no test enters, and that
is the class the whole audit is structurally blind to: the answer is not a better
gate but **one function every site calls**, so there is one place that can be
wrong instead of three answers that can drift.

**And a claim of coverage that nothing enforced.**
`only_the_action_that_reads_the_height_is_given_one` is the gate written for a
wrongly classified height, and its own docblock said *"every action, so a new
variant reaches this list by failing to be in it"*. Nothing made that true: it was
a plain array naming eight of the seventeen variants that existed then, and one
of the nine it omitted was
`DiffTo`, which is the second wrong height this branch found. **That is how
`Bottom` shipped misclassified in the first place**, and it is the exact shape
this file calls worse than no claim at all, in a gate whose subject is that shape.

It is exhaustive by construction now, in two steps that cannot be satisfied by
prose: a `tag` function matching every variant, so a new one is a **compile
error**, and a count assertion under it, so an added arm fails loudly until the
variant is actually driven.

**Three false cost claims about one function, in three places.** `span_in` was
described as "a `stat` against a span this tick has already proved" in its own
docblock, in `diff_to`'s, and in `Bottom`'s arm. Two were corrected in earlier
rounds and the third outlived both corrections, because nothing reads a comment
and no gate can. It is `block_rows`' cost quoted under a different call, and the
truth is that the lookup is free for the file the walk drew and a whole-file
measurement for one it did not, which is reachable after the changed set shrinks.

**The `needs_height` predicate is *reads it in some reachable state*, not *always
reads it*, and it now says so.** `Bottom` is measured in files unpinned and in
rows pinned. Answering `true` costs it a height it ignores half the time, which is
the conservative direction; the other reading hands it a zero in the state that
reads one, and `saturating_sub(0)` then quietly means *the whole file*.

## B16 — round four: the fourth layer out, and the claim that overstated the fix for the third

Round four found one live defect, and it is the fourth instance of a pattern the
three rounds before it each produced once. Stated plainly, because the pattern is
worth more than any of its instances:

| round | what it found |
|---|---|
| one | three gates green with the feature deleted, and a key nothing proved was bound |
| two | a round-one fix that never reached the shell, and a latent panic a docblock hid |
| three | a round-two fix that reached two of three call sites, and a coverage claim nothing enforced |
| four | a round-three fix sized against a chrome its own side effect invalidates, and a coverage claim that overstated round three's fix |

**The live one.** `Shell::diff_rows_for` builds the chrome to size the body
*before* `App::apply` runs, and `Action::Bottom` is a manual scroll, so `apply`
turns follow off underneath it. `Footer::plan` sizes its rungs from
`Chrome::following`, where `follow ▶  N/M` is thirteen columns against `N/M`'s
three, and between **31 and 40 columns** that decides a one-line footer against a
two-line one. So the region the frame draws is thirteen rows where `span - height`
was taken against twelve, the pinned file's last row rests one line above the
bottom, and `App::view` writes the position straight back every frame: it stays
wrong until the reader scrolls. That is
[#57](https://github.com/breferrari/vigia/issues/57)'s symptom on the arm written
to avoid it.

`anchored = true` is the fix, and it is a truthful statement rather than a
workaround. `anchored` means *reached by scrolling* and it licenses the walk's
back-up for a short screen; `G` under a pin is asking for exactly that, and it is
not a claim about what belongs on the top row, which is what a jump is and why
`jump_to` clears it. Computing the height after `apply` cannot work, because
`apply` is what needs it.

**And the claim that overstated round three.** That round made the height gate
drive all eighteen `Action` variants and wrote that "an added arm fails loudly
until the variant is actually driven". False: the assertion compared
`named.len()` with a hand-written `VARIANTS`, so a new arm tagged `18` and left
undriven keeps both at eighteen. It compares the *set* against `0..VARIANTS` now,
and says out loud that `VARIANTS` is hand maintained with the compile error in
`tag` as the prompt to bump it. **Three of the eighteen rows were vacuous besides**:
`ScrollList`, `ListTo` and `ListRow` move the list's window and nothing else, so a
gate reading only the diff's position could never fail on them, and `ListRow` was a
hard no-op because `App::list_rows` is zero until a view is drawn. Real coverage
was fifteen asserted as eighteen.

**The count of overstated guarantees on this branch is four**, in four places, and
every one was found only after the previous was fixed: three cost claims about
`span_in` and one about which gestures reach the frame before the walk clamps.
They share a mechanism worth naming: **a docblock answers the question a reader
would otherwise have gone and checked**, so a wrong one does not merely fail to
help, it actively stops the check. No gate can see a comment. What finally caught
each was an agent reading the sentence against the code beside it.

## B16 — round five: the rule stated in two places, inverted in one

Round five found nothing that runs and one thing that matters more than most of
what runs, which is what a converging audit looks like at the end.

**`anchored` is defined in two canonical places and both name `G` as the exemplar
of a jump that must *not* anchor.** `App::anchored`'s own field docblock, which is
*the* definition of the flag, and the short-screen guard's comment in
`View::collect`, which is its only reader. Round four made pinned `G` set it
**true**, wrote twenty-five lines of justification inside the arm, and left both
statements standing.

Nothing breaks today. What breaks is later: a session editing either site reads
the rule with `G` as its worked example, "restores" `false`, and reinstates the
stale-chrome short screen that round four fixed. Both are qualified now, and the
qualifier says out loud that it is load-bearing rather than pedantic.

**That is the fifth instance of one mechanism, and the mechanism is worth stating
once for the next reader rather than five times for this one.** Every serious
finding on this branch after the first round was a **claim that stopped being
true when the code moved under it**: three cost claims about `span_in`, one about
which gestures reach the frame before the walk clamps, one about a gate's own
coverage, and now one about which key anchors. No gate can read a comment, so
none of them could go red. Each was found by a reader holding the sentence against
the code beside it, and each was found only after the previous one was fixed,
because a wrong docblock **answers the question a reader would otherwise have gone
and checked**.

The practical form of that, for anyone changing this area: when a behaviour moves,
grep for the places that state the rule it obeyed, not only the places that
implement it. On this branch the implementation sites were always one and the
statement sites were always two or three.

## B16 — round six: the clamp that only worked for readers who had scrolled

Round six found one behavioural defect, and it is the last of the family: **the
pin's clamp was gated on a flag the pin did not set.**

`View::collect`'s back-up rests a short screen's last row on the bottom, and it
fired only for `anchored || landed_inside` at the time, because a position a
*jump* placed is
a claim about the top row and must not be moved off it. `Action::ToggleSingle` is
not a manual scroll, so it inherited whatever set the position, and `App::diff_to`
sets `anchored` **false**. So a reader who dragged the diff's bar into the middle
of a tall file and then pressed `s` got a short screen with trailing blanks, which
jumped upward on their next `j`.

**Three places said the opposite**, including this ruling's own §11.2 text: *"a
screen straddling two files comes to rest on the pinned file's last screenful"*.
The claim was true, but only for a reader who had arrived by scrolling, and every
straddle in `tests/single.rs` was reached with `Action::Scroll`, which anchors. A
suite that reaches one state four ways, all of them the same way, cannot see the
fifth.

`ToggleSingle` anchored on the way in, which is the same argument
`Action::Bottom`'s arm already makes: a pin is a claim about what the diff may
**reach**, not about what belongs on the top row, so when the position it inherits
overruns the pinned file the pager's answer is the right one.

**That fix leaked and round seven replaced it; this paragraph is kept because the
argument survived and only the mechanism changed.** `anchored` outlives the pin,
so a jump onto a short tail followed by `s` and `s` left an *unpinned* frame
anchored and backed the reader out of the file the jump was for. The licence is a
term in the guard now, `anchored || landed_inside || single`, which has no state
to leak and is excluded on a pinned jump for free.

**And the gate that sold the pin's one cheap property was vacuous.**
`a_pinned_frame_counts_no_height_at_all` asserted `measured == 0` on a second
frame over a tick whose spans the first frame had already proved, and an
*unpinned* second frame reports zero too, which the gate immediately above it
asserts by name. The stated control measured the first frame rather than the
counterfactual. The pinned half has a freshly advanced frame of its own now, with
its spans unproven, so the zero means what the ruling says it means.

Both are the same shape as everything else this audit found: **a claim that was
true for the cases anyone had tried.**

## B16 — rounds seven and eight: where a reverted fix leaves its description

Round seven replaced round six's mechanism and round eight found the two places
that still described the old one. Both are prose, neither runs, and the second is
the reason this is written down rather than fixed quietly.

**What changed.** The pin's back-up was licensed by writing `anchored` from the
toggle's arm; it is licensed by a **term** now, `anchored || landed_inside ||
single`. The argument did not move: a pin is a claim about what the diff may
reach rather than about what belongs on the top row, so a position that overruns
the pinned file gets the pager's answer. Only the mechanism moved, because the
flag outlives the pin and the term does not.

**Where the description stayed behind.** Round seven corrected `app.rs`,
`view.rs`, `reads.rs`, `scroll.rs` and one number in this file, and left
`SPEC.md` §11.2 B16 and this file's own round-six section saying the pin sets
`anchored`. Code and `SPEC.md` disagreeing is a stop condition in `CLAUDE.md`, and
it went one round undetected.

**That is the seventh instance of one mechanism and the last worth counting.**
Every serious finding on this branch after round one was a claim that stopped
being true when the code moved under it, and the count of *statement* sites has
been consistently larger than the count of *implementation* sites: one arm, three
prose sites; one guard, two rulings. No gate can read a comment, so the only
instrument is a reader holding the sentence against the code.

The rule this branch earns, for anyone changing this area: **when a behaviour
moves, grep for the places that state the rule it obeyed, not only the places that
implement it** — and when a fix is *reverted* rather than extended, grep again,
because a revert leaves a description of something that no longer exists and reads
exactly like a description of something that does.

## B6 — the amendment the ruling predicted, and the one-line defect that shaped it

`SPEC.md` §11.2 B6 is the ruling and §11.1 is the behaviour. This is why the shape
is two files rather than three more keys in one.

**B6 predicted this amendment and named the test it would have to pass.** Its
closing line reads *"there is nowhere to put a setting that is neither of those…
the next setting that is neither a preference about you nor a fact about the
terminal in front of you will find it again."* A view default is not in that gap.
It is B6's **first** kind without strain, a preference about you, and B6 already
rules that those live in a file. So the amendment **applies** the taxonomy rather
than widening it, and B7 remains the only candidate that has ever been in the gap.

That distinction is what kept this cheap. Widening B6 would have needed the
argument B7 was refused for; applying it needed only a place to look.

**The shape was decided by a defect, not by taste, and the defect is one line of
`theme::from_env`.** That function resolves `VIGIA_THEME` first, and a built-in
name wins outright:

```rust
match Theme::named(named) {
    Some(built_in) => built_in,
    None => load(Path::new(named))?,
}
```

So `VIGIA_THEME=dark` **never opens the theme file at all**. Had the view keys
gone in beside the colours, a reader naming a palette for one session would have
silently lost their view defaults, on a gesture with nothing to do with the
settings it discarded. That is the same class of failure the theme parser already
refuses unknown keys to avoid: a setting that does nothing, with no way to find
out why.

The tidiness argument points the same way and is the weaker half: a file called
`theme` holding `rail on` reads as a mistake. It is recorded second because it
would not have been enough on its own.

**What the two files share is everything that costs something**: one format, one
discovery rule (`HOME` then `USERPROFILE`, each checked for emptiness before the
next is tried), one error path, one report-before-the-takeover order. What they do
not share is a subject. The amendment therefore adds a place to look and three
keys to look for, which is the same shape the `VIGIA_GLYPHS` amendment took when
it added a detector rather than a surface.

**`follow` is excluded, and that is I5 doing work rather than an omission.**
*Correct with zero interaction* is a promise about the program; a file able to
turn following off would quietly make it a promise about one reader's
configuration. The three keys that are in have in common that every combination of
them is a legitimate pane, which is not true of a pane that has stopped following.

**And no variable joins them.** A variable exists for *not this time*, which is
`VIGIA_THEME`'s whole job. Here that sentence is already spoken by `m`, `r` and
`s`, one press each and named on the gestures sheet, so a variable would be a
second spelling of something the pane says better. B6's count of one is untouched;
its count of files is what moved.

---

## B19 — the neighbour that already solved this, and the unit split the total could not survive

`SPEC.md` §11.2 B19 is the ruling and §11.1 is the behaviour. This is the trail.

**The field, read rather than remembered, on 2026-08-22.** Five tools were checked
against their own source or their own `--help` on this machine, and one of them
changed the proposal.

- **`delta`** wraps, and `--wrap-max-lines` defaults to **2**: *"How often a line
  should be wrapped if it does not fit. Zero means to never wrap. Any content
  which does not fit after wrapping will be truncated."* That is a direct answer
  to §11.1's objection rather than a way around it, and it is where the cap came
  from. `--wrap-left-symbol` is `↵`, `--wrap-right-symbol` `↴` and
  `--wrap-right-prefix-symbol` `…`, so the wrap is marked at the end of the line
  it leaves rather than at the start of the one it enters.
- **`ov`** binds `[w]`, `[W]` to a character-based wrap toggle and `[Alt+w]` to
  word wrap, which is what confirmed the key rather than a preference for it.
- **`bat`** reserves the gutter and leaves it **blank** on continuation rows, so
  the absent line number is itself the signal, and spells the opposite state `-S`
  / `--chop-long-lines`.
- **`less`** wraps unless `-S`, and toggles the state at runtime.
- **Neovim** keeps both axes and gives each its own motion, `gj` against `j`, and
  indents continuations with `'breakindent'`: *"Every wrapped line will continue
  visually indented… thus preserving horizontal blocks of text."*

**And the neighbour that had to build it says why it is cheaper here.**
[dandavison/delta#657](https://github.com/dandavison/delta/issues/657) is the
request that produced delta's wrapping, and its problem statement is that the
pager is the wrong layer: *"delta uses one of many various pagers, usually some
form of less, which supports line-wrapping, but this breaks lots of things (like
delta's line numbers)"*. This tool owns its painter and its gutter, so the hard
part delta built around does not exist here. The same thread calls the
horizontal-scroll alternative *"tedious"* and says it *"disrupts the viewing
experience"*, which is a second-hand data point for wrapping over the pan §11.1
named as the rejected alternative.

**The total was tried the way the issue proposed and abandoned on a fact.** The
issue asks for a wrapped height threaded through `view::rows_of`, with the
measurement taken on the counting pass and cached per text width. Two things
kill it, and only the second is decisive.

The cheap objection is cost: `vigia_core::FileSpan` is four numbers today and a
wrapped height is a function of the text **and** of the text width, so the span
would have to carry either a per-width summary or a per-file re-measure. That is
an argument about size and could have been answered with a measurement.

The objection that ends it is that the dependency is **circular**.
`render::gutter_width` sizes the gutter from *the largest line number on screen*,
so the text width is a function of the drawn rows; a wrapped height is a function
of the text width; the total is a function of the wrapped heights; and which rows
are drawn is a function of the total. There is no order in which those four
evaluate. It is not an expensive computation, it is not one.

**So the split is made and one unit is given one owner.** A **logical row** is a
row of the diff's model; a **display row** is a row of the terminal. The bar,
every jump, every clamp and every counting twin stay logical. Display rows exist
inside the viewport only. That is the shape the design record already names: when
one quantity splits into two, every site that treats them as interchangeable
becomes a defect at once, so the quantity gets one owner rather than a spelling
at each site.

**What the split made visible immediately** is that `View::last_screenful` and
the overshoot branch in `View::collect` both compare a **logical** span against
the **display** height, which is exactly the units bug that shape predicts. With
wrapping off the two numbers are equal and nothing can see it; with wrapping on
the top lands too far back and the last rows of the diff fall off the bottom, so
the end of the diff becomes unreachable by the gesture that exists to reach it.
`crates/vigia/tests/wrap.rs::the_bottom_of_the_diff_is_reachable_when_lines_wrap`
is what fails when it comes back, and
`the_wrapped_bottom_survives_the_frame_after_the_gesture` beside it is what
fails when the clamp holds for one frame and not for the next: the first draft
of the fix fired on the frame the gesture produced and on no frame after it, so
the end of the diff was visible until anything repainted.
