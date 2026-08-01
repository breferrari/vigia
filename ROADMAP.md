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
| ✅ | The settle margin: measure before narrowing it | [#32](https://github.com/breferrari/vigia/issues/32) |
| ✅ | I3 flat resources over days (soak) — **the gate; not the window** ([§10](SPEC.md#10-open-questions)) | [#5](https://github.com/breferrari/vigia/issues/5) |

**The shell is in, so the rest of this phase has something to render into.** It draws the working-tree diff, follows the watch engine's ticks, scrolls by keyboard and wheel, and holds its own half of I4: one screenful reads only the files it draws, gated across two fixtures in `crates/vigia/tests/reads.rs`.

**The shell is now safe to leave running.** [#8](https://github.com/breferrari/vigia/issues/8) closed the last untested module: the takeover is data, giving it back is asserted as its exact inverse, and a half-finished `Session::enter` undoes exactly what it took. Seven mutations, each killed by a named test. It also settled what I8 can honestly promise: raw mode means Ctrl-C is a key event and never a signal, so an externally delivered `kill` is out of reach without a dependency `SPEC.md` does not name. That is [#24](https://github.com/breferrari/vigia/issues/24), on the shelf below.

**The phase's correctness half is done.** [#6](https://github.com/breferrari/vigia/issues/6) landed I5: the view moves itself to the file that just changed, with nothing pressed. It was blocked on a decision rather than on code, so **B1 and B2 were ruled first, in their own commit**, and the implementation was written against the ruled spec. Ruling second would have settled the question by accident inside a snapshot test.

What ruling it exposed is worth more than the ruling. "The newest change" needs a recency signal, and the obvious one — `stat` every changed file — is [#19](https://github.com/breferrari/vigia/issues/19)'s recorded breach of I9 at scale. The filesystem event already names the path, and the gitignore filter was already resolving it and throwing it away, so `Tick` now carries it and following costs no read, no `stat` and no diff. The cheap answer and the correct one coincided, which is luck rather than design and is why it is written down.

**I6 is in, and it turned out to be a layout question rather than a text-shortening one.** [#7](https://github.com/breferrari/vigia/issues/7) inherited the collision I5 left: at forty columns, follow on, the footer drew `q quit · f follow · jk scr`, a hint cut mid-word in the **default** state. The answer was to stop shortening. `SPEC.md` §11.1 now rules one sentence the whole layout follows from, **a thing made of items breaks, a thing made of characters marks its edge, and content is neither**, so the hint bar takes a second footer line rather than losing a letter, everything else says which end it lost (`…` on the left for a file path, `›` on the right for everything else), and a clipped diff line is marked rather than counted as a truncated label. That last one closes the "a diff line still loses its tail" question this file used to carry.

**The invariant is now assertable, which it was not before.** `crates/vigia/tests/legibility.rs` sweeps every width from 1 to 120 with twelve gates, because a snapshot records a width and asserts no rule. The 40 / 80 / 120 triple §3 names is complete for the first time; 120 was missing entirely. Two things the mutation pass turned up are worth keeping: the row helper was reading `TestBackend`'s `Hidden by multi-width symbols` note instead of the row whenever a wide glyph was on screen, and the mark can be **swallowed** by a two-column glyph landing on the final column, which is a clipped line drawn as one that ends. Only a gate over double-width content reaches that, and twelve mutants now die to a named test with none surviving.

**I2b is in, and the open question it carried is answered: `syntect` holds, and tree-sitter stays out.** [#4](https://github.com/breferrari/vigia/issues/4) landed incremental re-highlighting. One screenful of Rust is 1.53ms against the 16ms frame budget, loading the grammars is 318µs against I7's 50ms, and a real shell frame under continuous edits over the 100-file, 100k-line fixture is **10.52ms p99** against 6.97ms for the core frame path alone. The dependency's own defaults were the trap: `syntect` selects `regex-onig`, which is oniguruma, so `cargo add syntect` alone would have put `cc` in the graph and cost the static musl binary and Windows tier-1.

**The number that decided the design was the one for doing it the obvious way.** Parsing a hunk whole costs 60.97ms for the 1006-line hunk the budget fixture produces, 3.8x over budget and paid on *every* frame under I9's own shape. So a hunk is parsed forward only as far as the screen has asked, and what was parsed is kept: the same finding I2a made about re-diffing, arrived at independently. Revalidation is a hash of the hunk rather than a counter the frame path bumps, because inside the two-second settle margin the frame path re-diffs an untouched file every frame and a counter would re-highlight files nobody edited.

**The audit found a second breach of the same invariant, and it was worse.** Parsing forward is only cheap while a hunk is stable, and under continuous edits it never is: the file being read is the file being written. A changed hunk threw its whole parse away, so a reader who scrolled in to follow along paid the entire walk on every tick, with no input at all: 520 lines a frame for a 22-row body, **53ms p99**, sustained. A changed hunk now rewinds to the deepest parse position the new content still agrees with, which is exact rather than approximate, and a frame costs a screenful plus one stride at any depth: 40 lines and **11.47ms p99**. The gate that missed it measured at row zero, which is the cheapest position of the shape it was testing; `SPEC.md` §7 now says that out loud.

**What it cost the reader: the diff signal narrowed to the sigil column.** `SPEC.md` §11.1 rules that highlighting follows the mockup literally, which means added, removed and context lines are coloured identically. What the picture uses to keep them apart is a row background tint and a left bar, and sixteen foreground-only colours can draw neither, so until [#11](https://github.com/breferrari/vigia/issues/11) lands truecolour the `+` and `−` carry it alone. That is a real loss against §5's glance thesis, recorded rather than absorbed, and §5.1 gained the two rows the mockup never had.

**A defect found by using the tool rather than by testing it.** [#30](https://github.com/breferrari/vigia/issues/30): `vigia .` drew one frame and then ignored every filesystem event for the rest of the session, silently. `gix` returns the path it was discovered with, so a relative argument leaves the worktree root as `"."`, and no event path ever begins with that. Ten watch tests missed it because `Scratch::worktree` always discovers by an absolute path, so the relative case had never been run once. Folded into this phase's work deliberately rather than deferred, because a monitor that does not refresh cannot be used to check anything else.

**What it cost the frame path: one row, at narrow widths only.** `body_height` takes the chrome and the changed-file count now, since the footer's height varies, and both it and the renderer plan through one function so the caller's row budget and the layout cannot drift. The height deliberately does **not** depend on a notice: a transient error that grew the footer would jog the reader's diff down a row and back, which is what I5 already ruled out for a resize.

**The margin stays at two seconds, and the thing that was actually missing was a gate.** [#32](https://github.com/breferrari/vigia/issues/32) was untracked until this pass: `SPEC.md` §10 named it as a prerequisite for [#5](https://github.com/breferrari/vigia/issues/5) ("do this before the soak test") and no issue carried it, so the `take-next` pre-flight could not see it. Taking it meant measuring first, and **both halves of that bullet were wrong.**

The proposed fix was unsound. Narrowing the margin to the smallest positive difference between the modification times status already reports bounds the *smallest* granule observed, and soundness needs the *largest*, because granularity is not uniform within one volume: 10,324 same-length rewrites of one file on NTFS gave positive gaps from **502µs to 17,522µs**, a 34.8x spread, and a hundred-file bulk write left a smallest cross-path gap of **998µs**. A 998µs margin would leave a real 17.5ms granule uncovered, which is the stale diff the margin exists to prevent, and the 16ms §10 predicted is itself under the 17.5ms measured. Nothing passive does better, since a monitor never writes and only ever sees the gaps its user's tools left behind.

And the breach was the fixture rather than the product. The 18 to 21ms per frame was measured over the **core** frame path, whose fixture materialises every file so its own gate cannot pass vacuously. Same fixture, same bulk rewrite, 2.5s of frames, release: the core runs **98 of 182 frames over the 16ms budget** (p99 22.34ms), and the shell runs **0 of 1060** (p99 2.66ms), because I4 already makes the shell diff only what it draws, which over this fixture is about one file a frame. What is left is the core's own number, and removing it is [#19](https://github.com/breferrari/vigia/issues/19)'s job rather than the margin's.

**What let the question stay open for two phases is now the more useful finding.** Every structural gate in `crates/vigia/tests/reads.rs` calls `settle()` before it measures, and `one_screen()` measures a single cold frame, so the settle margin — the one window in which the frame path can prove nothing and recomputes by design — was the one window nothing gated, in *either* direction. Nothing would have caught the breach had it been real, and nothing showed that it was not. Both tiers now measure inside it, and `SPEC.md` §7 carries the general rule beside the one about position: a gate that settles before it measures has measured the cheapest state.

**I3 is in, and the first thing it cost was a line of the spec.** [#5](https://github.com/breferrari/vigia/issues/5) asked for a soak of 24 hours and a GitHub-hosted job is terminated at six. So the budget stayed a claim about a day and the *proof column* changed to name what runs: the scheduled job takes the longest window that fits under the cap, `workflow_dispatch` carries a duration for a machine without one, and the sample **count** is fixed rather than the interval, so a four-hour run and a one-day run produce the same statistic from the same code.

**The soak is the shell, not the engine.** A core-only soak would have proved I3 about a program nobody runs, so the harness is `vigia::run` with the terminal removed: a real watch thread, real coalescing, one `Frame::advance` per tick, follow and scroll through `App`, `View::collect` driving the highlighter, and `render` into a real buffer. It runs as two processes, because "zero temp files retained" cannot be asserted against a temp directory the rest of the machine is also writing to. The parent builds a private one, re-executes the test binary into it, and checks it is empty on both sides of the run; the child checks it was really redirected there, so the gate cannot pass against a directory nothing could write to.

**The record set the bar for what would count.** The I2b audit left this issue a finding rather than a free hand: `Highlighter::tracked()` counts entries and not the lines each entry holds, and checkpoints had made an entry heavier, so it is a correct bound and a weaker metric than it reads as. A soak assembled out of those counters would have re-shipped exactly that. So real RSS is the gate, and the scripted reader scrolls four hundred rows into one hunk, which is the case that makes an entry heavy.

**Measured, release, the same 100 x 500 fixture the budget gates use, one hour:** 73,265 frames, every one a full screen, from 70,086 write rounds and 14,018 files created and deleted. **RSS drift 2.18% against the 5% budget**, 25.6 MiB after warmup against 26.1 MiB at the end. At most 68 diffs held against 106 changed files, over 14,118 distinct paths ever changed, so the cache followed the diff rather than the session by a factor of two hundred. Zero temp files retained, zero failed frames, 2.0 GiB read and 1.59M lines highlighted.

**What has not run is the window in the budget.** An hour is what a session can measure; four hours is what CI will, nightly on Linux and weekly on all three targets; 24 hours needs a runner without the cap. The gate exists and fires, and the number it is standing on is an hour, which `SPEC.md` §10 records as open rather than closed.

So **the ✅ on I3's row means the gate, not the window**, and the row now says so. This file and the spec disagreed about I3 for a day — the row read done while §10 read open — and the rule at the top of this file is that when they disagree the issues win and this file is stale. The tick is right by that rule, since [#5](https://github.com/breferrari/vigia/issues/5) is closed and closing it was correct: what #5 owed was a soak that drives the real pipeline, and that exists and gates. What it could not deliver is a claim about a day, because the platform will not run one. Do not quote I3 as a proven day-long budget until the window it names has actually run; a gate that fires and a budget met at its own window are different claims, and only one of them is in hand.

**One thing the hour turned up is worth not overreading.** Its post-warmup quarter medians rise monotonically, 25.58 to 26.14 MiB, a least-squares +0.92 MiB/h that a naive extrapolation would put over budget in a day. The fifteen-minute run of the same code slopes **−0.70 MiB/h** with flat quarters, so the two disagree in sign, and both are about one percent of the **+81.3 MiB/h** a deliberate 1 KiB-per-frame leak produced. That is run-to-run variation at this sample size, not a trend, and the window that settles it is the one the budget already names.

**The runs also settled where the warmup ends, and that is why the drift gate refuses short windows.** RSS climbs to its plateau in the first thirty seconds and is flat afterwards. The same code over a fifteen-second window reports **10% drift** and is measuring nothing but that climb. So the gate asserts only at ten minutes and above, and prints its numbers with a note below that. `SPEC.md` §7 carries the rule it leaves behind: a drift gate over a window shorter than its own warmup is measuring warmup, and refusing to answer is the point rather than the fallback.

**Two things the workload got wrong, both found by running it rather than reading it.** Editing a cold file every round returned to each file inside the two-second settle margin, so nothing was ever provably unchanged: 380 diffs computed and **zero reused**, with the reuse path the soak exists to bound never taken once. And every fixture file is a single thousand-row hunk, so a screenful held one or two and "bounded by the viewport" was tested three entries away from a bound of forty. The cold rotation is now every fourth round, the hot file is rebuilt as one hunk every eight lines, and both are gated so neither can come back quietly.

**The hunk bound is compared against the right number now.** Against the body height it is loose by construction and no workload can close the gap: a hunk costs a header, a line and up to six rows of context, so forty rows cannot show more than about five of them. Against the hunks *this screen* could have asked for, the run sits at equality, and a deleted cache sweep reports 48 parses held on a screen that could ask for one.

**Phase 2 is closed.** The shell draws the working-tree diff, follows what changed with nothing pressed, scrolls by key and wheel, degrades to forty columns without cutting a hint in half, restores the terminal on every exit the process controls, re-highlights only the hunks that changed, and holds its resources flat over a run measured in minutes locally and hours nightly. Every invariant in `SPEC.md` §3 now has a test that fails when it is violated. What the phase turned up and did not fix is on the shelf below, with an issue and a milestone each.

## Phase 3 — glanceability

Milestone: [Phase 3](https://github.com/breferrari/vigia/milestone/3)

| | Task | Issue |
|---|---|---|
| ✅ | I10 bounded history, and the sparkline, gradient and pulse drawn from it | [#38](https://github.com/breferrari/vigia/issues/38) |
| ✅ | The heat strip, and the whole-file line count it needs | [#39](https://github.com/breferrari/vigia/issues/39) |
| ✅ | The header mode word, the mode set, and the empty state (B3) | [#40](https://github.com/breferrari/vigia/issues/40) |
| ✅ | Fast scrolling drops frames, and a drawn row costs its whole line | [#45](https://github.com/breferrari/vigia/issues/45) |
| ✅ | The header carries no changed-line total, and §10 closed with the reason | [#49](https://github.com/breferrari/vigia/issues/49) |
| ✅ | The status bar: frame time and RSS, on all three tier-1 targets | [#41](https://github.com/breferrari/vigia/issues/41) |
| ✅ | A viewport past the end of the diff drew one row and blanked the screen | [#57](https://github.com/breferrari/vigia/issues/57) |
| ⬜ | **On Windows every CRLF file reads as a full rewrite** | [#65](https://github.com/breferrari/vigia/issues/65) |
| ✅ | I3: the restart left a second screenful of hunk parses in the pass | [#64](https://github.com/breferrari/vigia/issues/64) |
| ✅ | Theming, with a 256-colour degradation path | [#11](https://github.com/breferrari/vigia/issues/11) |
| ✅ | Scrolling into the tail of a diff leaves the pane half empty | [#59](https://github.com/breferrari/vigia/issues/59) |

**[#10](https://github.com/breferrari/vigia/issues/10) was split before anything here was taken**, which this file had blocked the phase on. Reading `assets/preview.svg` as the specification it already is (`SPEC.md` §5.1) turned up eight distinct pieces of work behind two rows, and #10 alone carried four features that share no implementation.

The correction that mattered was not the count. **It is that this is not a rendering phase** (`SPEC.md` §5.2), and building the first child confirmed it: the diff was mostly `vigia-core`. The split follows what each element needs rather than what it looks like: history-backed (#38), whole-file-backed (#39), chrome (#40), self-measuring (#41).

**One of #10's four bullets had already shipped.** Per-file `+42 −7` counters are built in `view.rs` and drawn by `render.rs`, and §5.1 has said "Covered" for them since the mockup was specified: a file has to be diffed to be drawn, so they cost nothing and landed early and quietly. No issue was filed. This file also claimed the **key-hint bar** was "untracked entirely" when [#7](https://github.com/breferrari/vigia/issues/7) landed it with I6, and `follow ▶` alongside it under [#6](https://github.com/breferrari/vigia/issues/6). Both corrected here.

**Fast scrolling is fixed, and the measurement it started with disagreed with every guess in the issue.** [#45](https://github.com/breferrari/vigia/issues/45) was reported from use rather than found by auditing, and it opened with a measurement because [#32](https://github.com/breferrari/vigia/issues/32) had already closed with "measure before narrowing it". Three suspects were named. Each got a number, and the ranking they were named in was wrong.

**The first finding was that nothing could have caught this.** No budget gate in this repo had ever painted: every one timed `Frame::advance` plus `App::view` and stopped, so `render` sat outside both tiers on both crates, which is where a row's width is decided. And no fixture had a line wider than a pane, so over 34-column ASCII "a row costs the pane" and "a row costs the whole line" produce the same count and no gate could tell them apart. Both are `SPEC.md` §7 rules now, and `crates/vigia/tests/paint.rs` is the structural gate the second one was missing.

**The suspect the issue led with was real and small.** `printable` walked every character of a line and clipped afterwards, so a 22-row body of Japanese examined **8231 characters to fill 1600 columns, 5.1x**, unchanged by pane width. Bounded now, and the same body examines 1342. In wall clock that is 104µs to 60µs against a 16ms frame, which is not the bug and is fixed anyway: the shape is the one I4 forbids, and §10 already ruled that a known instance of it is either fixed or written down.

**The suspect that dominated was the one the issue ranked last.** A hunk re-entered after scrolling off was re-parsed from its first line, because the cache swept on exit: **26.39ms** against a 16ms budget, once per file, for an answer that had been in memory a frame earlier. `RETAINED_HUNKS` keeps four hunks after they leave the screen and it becomes **397µs**. Four is a constant added to the viewport bound rather than a second cache, so I3 sees a higher plateau and not drift, and the soak's tight bound moved by exactly that constant. A *first* entry from below still costs the whole walk (**25.01ms** on a 120-row hunk) and stays the cold path §7 carves out, because bounding it needs a redraw nothing schedules and I1 forbids a timer for one.

**And the largest half was not in the render path at all.** A trackpad reports one flick as a stream of scroll events and every one of them was a full frame, so the pane rendered every position the gesture passed through. The loop drains what is already queued and paints once, capped at 64. Nothing is dropped: every wake is still handled in arrival order, so a tick still records its paths for I10 and follow still lands where it should. A hundred notches drawn as one frame highlight four times fewer lines than the same travel drawn one at a time.

**The hypothesis about CJK was wrong, and worth recording as such.** The issue expected `fancy-regex` backtracking over wide characters. Measured against the identical bytes under an extension `syntect` has no grammar for, the parse is **0.32µs a byte** on 660-byte Markdown against **1.76µs a byte** on the 34-byte Rust fixture: wide content is cheaper per byte and slow only because the lines are nineteen times longer. The Japanese README was the reproduction, not the cause.

**The chrome child is in, and it repaid three debts rather than adding a feature.** [#40](https://github.com/breferrari/vigia/issues/40) ruled the mode set at **two**, `watching` and `not watching`, because settling and idle are both durations and a shell that wakes only when a file changes cannot draw one honestly. It ruled B3 and moved it to `SPEC.md` §11.1, which meant correcting `working tree clean`: that is git's phrase, git compares the index against HEAD as well, and a fully staged worktree was being told it was clean while `git status` said the opposite. And it wrote down two things the code had been deciding on its own, the header's deliberate departure from `assets/preview.svg` and B5's already-shipped half.

**It also found that `vigia .` drew `.`.** The invocation the tool is named after headered the screen with the one thing a reader already knows, because `gix` returns the workdir as given and `Path::new(".")` has no final component. Every fixture in the suite discovered by an absolute path, so the case had never been drawn. Same shape as [#30](https://github.com/breferrari/vigia/issues/30), on the display side instead of the watch side, and found the same way: by running it.

**I10 is in, and the thing it was blocked on turned out to be the thing it produced.** [#38](https://github.com/breferrari/vigia/issues/38) promoted proposed I10 into `SPEC.md` §3 with a budget of **256 paths and 120 seconds**: a bounded store in `vigia-core`, fed one coalesced tick at a time, evicted by window and by least-recently-changed. Ten thousand distinct paths leave it sitting exactly at the cap, and the soak says the same about the real process, reporting 256 tracked with 207 evicted over 359 paths at a 300-file fixture.

**The store had to be fed from the watch, not from the frame path.** A burst that saves twelve files has to record twelve, and `Tick` named only the last one because that is all follow mode needed. It now carries the whole set, capped at the same 256 so a bulk operation cannot make a tick expensive before the store has a chance to bound it, and `Tick::newest` became a method over that list rather than a second field holding the same fact.

**The decay the mockup asks for was an I1 question, not a rendering one.** §5.1 draws the pulse as a label that persists and fades, and a fade against a wall clock has to be *seen* fading, which needs a redraw nothing schedules and which I1 forbids inventing a timer to get. So the window stayed real time and the sampling became event-driven, and the top rung of the ladder is *named by the newest tick* rather than *within N seconds*. That is what keeps it honest on a quiet tree: the label sits on the file that really is the newest change instead of freezing a clock mid-count. The dimmed row and the label are the same ladder read once, which §5.1 demanded and which two clocks would have broken.

**What it cost the reader, again: the gradient is three steps rather than a fade.** Sixteen foreground-only colours have bold, plain and dim to spend, so recency is a ramp of three. That is the same loss §11.1 already records for the diff signal narrowing to the sigil column, and the same issue fixes both: [#11](https://github.com/breferrari/vigia/issues/11).

**The heat strip is in, and the expensive part of it did not exist.** [#39](https://github.com/breferrari/vigia/issues/39) was the whole-file-backed child, and `SPEC.md` §5.2 had it as the element that pulls hardest against I2a: locating change *within* a file needs the file's length, and measuring that per frame puts back the read I2a removed. §5.2 predicted a cache keyed on `(path, blob id)`.

**No cache was needed.** `hunk::compute` interns both sides to diff them at all, so the working-tree line count was already computed on every diff and thrown away. It is a field of `FileDiff` now, cached and invalidated with the diff itself, which is a *stricter* key than the one proposed: a blob id names the index side, and a working-tree edit does not touch it, so the predicted cache would have served a stale length for exactly the file a reader is watching being written. The byte counts in `reads.rs` did not move by one.

That is the third time the expensive-looking property turned out to be a by-product of work already being done: I5's follow target and I10's burst paths were the other two, both already resolved by the gitignore filter. §5.2 now says to look for the by-product before designing the cache.

**And the layout rule needed a third clause.** *A thing made of items breaks, a thing made of characters marks its edge, and content is neither* covers a list, a token and a line. A heat strip is made of items and is not a list: the set of its slices **is** the claim, so dropping the last six would draw half a file as though it were the whole of it and a reader would conclude the tail is untouched. A projection re-projects instead, summing adjacent slices, which at halves is exact. The gate reads cell **colours** over a file changed at both ends, because every slice draws the same block and that is the one shape where truncation and re-projection differ.

Two existing gates had to change for the same reason, and it is worth knowing before the next block-drawing element lands: a sparkline's top rung and every heat slice are both `█`, so counting glyphs stopped telling the two strips apart and one gate started reading eighteen buckets. Both match on colour now.

**And a rule went into `SPEC.md` §7 that the soak found on its own.** A bound is only evidence when something reached it. The per-commit soak window touches about eighty paths against a cap of 256 and never turns the window over, so `tracked <= 256` is satisfied there by a store nothing filled. The gate now refuses to assert when the run reached neither eviction rule and prints why, exactly as the drift gate does, and the deterministic proof runs in every `cargo test` instead.

**The status bar is in, on all three tier-1 targets, and the interesting part is that it nearly shipped on two.** [#41](https://github.com/breferrari/vigia/issues/41) filled §5.1's last two unspecified cells. Frame time is the p99 of the last 128 completed frames, where a frame is the whole turn of the loop rather than the diff; memory is one read per painted frame. Measured with both inside the timed frame: **p50 2.56ms, p99 3.00ms, 0 of 250 warm frames over the 16ms I9 budget**, and one memory read is **193ns**.

**The number that decided the design was 42.8ms**, which is what `tasklist` costs to spawn on the reference machine: 2.7x the whole frame budget for the read alone. That is why `soak.rs` samples RSS 288 times an hour and not sixty times a second, and it was very nearly the reason Windows shipped without a memory cell at all. The in-process answer costs 193ns and comes from crates `gix` already puts in each target's graph, so it added **two edges and zero packages**. The mistake and its correction are in the pull-forward log under [#56](https://github.com/breferrari/vigia/issues/56).

**A monitor readout has to be constant width or it moves what is beside it.** Both cells are, by construction: the number is right-aligned in a fixed field, so digits change and the unit does not move, and past a useless magnitude the value gives way to a sigil (`>1s`, `>1GiB`) rather than to a sixth column. Neither cell can change the footer's height either, which is §11.1's notice rule one element over and matters twice here: the frame cell does not exist on the first paint, and the memory cell would not exist on a platform with no cheap read.

**And the pane went blank while the header said two files had changed.** [#57](https://github.com/breferrari/vigia/issues/57), reported mid-pass by using the tool: thousands of changed files, scrolled into, then `git reset --hard`. `View::collect` rested the diff's last row at the *top* of the viewport instead of the bottom, so it drew one row over twenty-two blank ones. Pulled into this phase rather than shelved, on [#45](https://github.com/breferrari/vigia/issues/45)'s precedent: found by using it, and a pane that goes blank has stopped being monitor-class.

> [!warning] The gate that should have caught #57 asserted it instead
> `the_bottom_of_the_diff_is_content_rather_than_blank` read
> `assert_eq!(view.rows.len(), 1)` against a twenty-two row body, under that name
> and under a comment saying an empty pane is indistinguishable from a broken
> one. Every word around it was right and the one line that runs pinned the
> defect in place for two phases.
>
> Then the *replacement* gate did the same thing one level down: it asserted the
> right outcome about a situation that never reached the fixed code, because the
> shared fixture's four-row files cannot hold a deep enough row offset to
> overshoot. It passed against the unfixed code and only the mutation run found
> it. **A gate can be wrong about its situation as well as about its assertion,
> and reading cannot tell the two apart.** `SPEC.md` §7 carries the first half.

**Theming is in, and the shape of it is two axes rather than one ladder.** [#11](https://github.com/breferrari/vigia/issues/11) closed the phase. A **palette** decides what may be drawn and a **depth** decides how finely it can be expressed, and both have to allow an element before it appears. That is what makes a 256-colour degradation path a mechanism instead of a second hand-written palette: `dark` is authored once, in the colours `assets/preview.svg` actually uses, and every rung below is derived from it. The two disagree in a way worth knowing about: `ansi` refuses a row wash at *every* depth, because a wash has to assume a background and that palette's whole contract is that it assumes none.

**`ansi` stays the default, and the cost of that is real.** It is the only palette correct on a terminal whose background nothing has detected, and detecting one needs a tty round-trip this shell does not make. So the row tint the mockup promises is invisible until a reader names a theme, and §11.1's recorded loss stands on the default. `VIGIA_THEME=dark` is what draws the picture in the README, which the caption now says.

**The part that took three attempts was mapping 24-bit onto sixteen names.** A nearest-neighbour search is the obvious implementation and it sends the mockup's `#3fb950` addition green to **cyan**: the palette's green is `#008000` with no blue at all, the input has 80 of it, and cyan's 128 is nearer to 80 than zero is. Reweighting did not fix it, because the metric was never the problem. The sixteen entries are dark saturated primaries and modern palettes are light desaturated ones, so lightness decides before hue does, every time. A reader glancing at a diff reads hue. It picks hue first from three bits against the input's own chroma range, and lightness second.

**What #11 did not bring is the fourth recency rung**, which this file and `SPEC.md` §5.1 both promised it would. The rung count belongs to `Recency`, which has three variants because the store can answer three questions about a path, and whose `cold` means *untracked* rather than *old*. A wider palette draws the same three rungs in better colours. That correction landed in its own commit ahead of the implementation, and §11.1 now carries the general shape: **a limit blamed on the rendering layer was a limit of the data behind it.** §5.2 caught the mirror image of that a phase ago.

**And the phase closed on a defect found by using the tool, for the third time.** [#59](https://github.com/breferrari/vigia/issues/59) was reported from a screen recording: scroll into the tail of a diff and the pane goes half empty. [#57](https://github.com/breferrari/vigia/issues/57) fixed one route to that screen and reads as though it covers both. Its restart fires on `overshot`, which needs the position past the end of the **last** file, and scrolling never does that: `skip` stays inside whichever file the top is in, the walk simply runs out of files, and the rest of the pane stays blank. One row short at first, another with every keystroke.

The fix must not apply to a jump, and two `follow.rs` tests are what said so. Following a file puts it on the top row and so does `G`; backing up to fill a short tail moves it off and makes a reader hunt for what the jump was for. A `Position` cannot tell a scroll from a jump, so `App` carries it.

> [!warning] The issue was filed on a diagnosis that was wrong
> #59 claimed the rendered area exceeded the window in both dimensions, on four
> symptoms read off video. A probe disproved the mechanism outright, and **two of
> the four were mis-cropped frames**: the pane runs to y≈1265 and the crop stopped
> at y=912, so the middle of the pane was being read as its bottom. A live
> screenshot then showed the header readout and the whole footer present.
>
> What the probe did find was worth the trip. The terminal size **changes** when
> the alternate screen is entered under Warp, 195x77 before and 199x75 after, and
> `Shell::area` was reading it with its own syscall while `Terminal::draw` read it
> again. A resize between the two sized the collect for a screen the paint no
> longer had, on the first frame, which a monitor may sit on for minutes.
>
> The rule this leaves: **measure the artifact, not a picture of it.** Four
> symptoms, one probe, and only one of them survived contact with a number.

> [!warning] Phase 3 is not closed, and [#65](https://github.com/breferrari/vigia/issues/65) is why
> It re-opened the day theming landed. `vigia` drew a file as **+905 -885**, the
> whole thing deleted and re-added, while `git diff` reported that same file as
> **unchanged**. Default Windows configuration, `core.autocrlf=true`, in any
> repository whose `.gitattributes` normalises line endings.
>
> The frame path compares raw worktree bytes against the index blob. Git runs the
> worktree side through the clean filter first, so CRLF becomes LF before anything
> is compared. Without that step every line of a CRLF file differs from its blob.
> `gix-filter` is already in the graph; this is a call not being made.
>
> **It is first in the queue because of what it costs, not what it costs to fix.**
> Every budget in this file is a claim about a tool that shows what changed, and on
> one of three tier-1 targets it currently shows a thousand lines of noise over a
> file nobody touched. The real edit is in there somewhere.
>
> Invisible to the whole suite: every fixture is built by `Scratch`, which shells
> out to `git init` with no `.gitattributes` and no `autocrlf`, so both sides are
> always LF and the filter is never exercised. That is §7's rule one axis over, a
> fixture that cannot tell two things apart because in that fixture they are the
> same thing. Fourth time.
>
> The half of that which is not about this repo is in the vault, since it would
> bite any project whose fixtures are built by a tool that has configuration.

## Phase 4 — distribution

Milestone: [Phase 4](https://github.com/breferrari/vigia/milestone/4)

| | Task | Issue |
|---|---|---|
| ⬜ | The mockup pins a file list above the diff, and the shell draws one stream | [#66](https://github.com/breferrari/vigia/issues/66) |
| ⬜ | The header's two facts compose into a claim the tool does not make | [#67](https://github.com/breferrari/vigia/issues/67) |
| ⬜ | `cargo-dist`, crates.io, Homebrew tap | [#12](https://github.com/breferrari/vigia/issues/12) |
| ⬜ | I3's window has never run, so the day-long claim is a gate | [#47](https://github.com/breferrari/vigia/issues/47) |
| ⬜ | Default view: unstaged only, or working-tree-vs-HEAD | [#50](https://github.com/breferrari/vigia/issues/50) |

**The first two rows come before the other three, and the order is not preference.** This phase is what gives `assets/preview.svg` an audience: a registry page and a README that people arrive at cold, having seen the picture before they have seen the tool. Both issues are places where the picture and the binary currently disagree, and a disagreement that ships is a support burden rather than a stale artifact.

Both were found by a reader looking at the picture and asking what it promised — [#67](https://github.com/breferrari/vigia/issues/67) from *"what is `watching`, and how do I toggle it?"*, [#66](https://github.com/breferrari/vigia/issues/66) from noticing that `src/engine/watch.rs` is drawn **twice** in the mockup, once in the summary block and once as the diff heading, which a single scrolling stream never does. Neither is visible from inside the code, and neither was reachable by `take-next`'s pre-flight, which compares invariant tokens rather than pictures. That is the same blind spot [#40](https://github.com/breferrari/vigia/issues/40) hit when it found B5 shipped while still marked `(proposed)`.

[#66](https://github.com/breferrari/vigia/issues/66) is **B4**, and taking it means re-reading the question. §11.2 asks whether the file list is *navigable* and proposes "one continuous scroll, list as map"; the load-bearing half is whether the list is a **region of the screen** at all, which that proposal settles silently. Its two options have different costs and only one of them is free: a pinned list ordered from `History` costs nothing to rank, and one ordered by diff size needs every changed file's diff, which is [#49](https://github.com/breferrari/vigia/issues/49)'s argument against a repository-wide total arriving in a second place.

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
| ✅ | `take-next`: the pre-flight cannot see an untracked spec prerequisite | [#34](https://github.com/breferrari/vigia/issues/34) |
| ⬜ | The bulk-rewrite I9 gate is flaky on macOS hosted runners | [#36](https://github.com/breferrari/vigia/issues/36) |
| ⬜ | Rename tracking and the non-streaming walk, at ten thousand files | [#48](https://github.com/breferrari/vigia/issues/48) |
| ⬜ | I7 is measured without the highlighter, and the first parse costs 98ms | [#51](https://github.com/breferrari/vigia/issues/51) |
| ⬜ | The heat projection's cost follows the file rather than the window | [#55](https://github.com/breferrari/vigia/issues/55) |
| ⬜ | The chrome may be too dim to read on a real terminal | [#60](https://github.com/breferrari/vigia/issues/60) |
| ⬜ | `G` leaves the pane short, and the first scroll yanks it back a screenful | [#62](https://github.com/breferrari/vigia/issues/62) |
| ⬜ | The row wash drops a column under every wide glyph | [#63](https://github.com/breferrari/vigia/issues/63) |
| ✅ | `take-next`: pre-flight the spec against the tracker | [#20](https://github.com/breferrari/vigia/issues/20) |

---

## Deferral shelf

Items that surfaced mid-phase and would have derailed the block they surfaced in. Deferral is a first-class outcome recorded here, not a dropped ball and not scope creep absorbed silently. Each one carries the phase it moved to.

| Item | Surfaced | Moved to | Why |
|---|---|---|---|
| Multi-worktree view: several agent sessions at once | Market pass, 2026-07-30 | Phase 5 | The strongest differentiator after glanceability, and the most monitor-shaped. Needs the single-worktree frame path to be cheap first, or it multiplies a cost we have not paid down |
| Jujutsu and Sapling support | Market pass, 2026-07-30 | Phase 5 | Git is the thesis. A second VCS before the first one is beautiful is scope, not reach |
| A truncated `.git/index` aborts instead of reporting ([#13](https://github.com/breferrari/vigia/issues/13)) | I2a, 2026-07-30 | Phase 2, with I8 | A `gix` defect, not a frame-path one: an index shorter than the object hash underflows a slice and panics, and `panic = "abort"` makes it uncatchable. The local defences are worse than the problem, and terminal restoration on panic is settled by I8 anyway. What #2 gates is the part vigia owns: given an error, the previous frame survives it |
| A symlink diffs as its target's contents ([#15](https://github.com/breferrari/vigia/issues/15)) | I2a, 2026-07-30 | Phase 5 | Pre-existing in `Worktree::diff`, which reads through the link where git stores the target *path*. Demonstrated against git as the oracle. Out of scope for I2a, which only caches whatever the primitive returns, but coupled to it: the fix has to move the fingerprint to `symlink_metadata` in the same change or a repoint between equal-sized targets reads as unchanged. Lands with the fidelity work I2b needs anyway |
| The fingerprint cannot see a timestamp-preserving write ([#16](https://github.com/breferrari/vigia/issues/16)) | I2a, 2026-07-30 | Phase 5 | `cp -p`, `rsync -t` and `touch -r` keep the length and put the modification time back, and no margin can catch that. Git carries the inode change time for it; `std` exposes no equivalent on Windows on stable, so closing it means depending on `windows-sys` directly, which is a spec decision rather than an implementation detail. Shipping the Unix half alone was rejected: a guarantee that differs by tier-1 platform is worse than one stated uniformly |
| Two paths differing outside UTF-8 collapse onto one cache key ([#17](https://github.com/breferrari/vigia/issues/17)) | I2a, 2026-07-30 | Phase 5 | `to_str_lossy` makes `FileChange::path` both the filesystem identity and the display string, and those are different jobs. The read half predates the frame path. Fixing it changes a published type, so it wants deciding rather than patching |
| A frame reads a whole file to discover it is binary ([#18](https://github.com/breferrari/vigia/issues/18)) | I2a, 2026-07-30 | Phase 5 | 64 MiB read and 16.24ms for a file the first 8000 bytes already condemn, with no size cap on either side. Pre-existing in `Worktree::diff`. Belongs with I3, which is where a memory ceiling gets decided |
| An idle frame is one `stat` per changed file ([#19](https://github.com/breferrari/vigia/issues/19)) | I2a, 2026-07-30 | Phase 5 — **reason expired 2026-07-31, see below** | 36.71ms at 2000 changed files against a 16ms budget, almost all of it syscalls. The fix is to revalidate what is drawn rather than everything, which I4 already licenses and which needs a UI that knows what is visible. Not a defect in the rule, a consequence of the test having to materialise every file to avoid passing vacuously |
| An external kill leaves the terminal in raw mode ([#24](https://github.com/breferrari/vigia/issues/24)) | I8, 2026-07-30 | Phase 5 | I8 promised "including `SIGINT`" and the shell falsified the premise: raw mode clears `ISIG` and `ENABLE_PROCESSED_INPUT`, so the interrupt key is a key event and never a signal. What is left is a signal nobody at this keyboard sent, and `std` has no way to catch one, so closing it is a dependency decision rather than an implementation detail. The single-task version is Unix-only (`signal-hook`, with `SetConsoleCtrlHandler` needed separately on Windows), which is the same asymmetric guarantee #16 already rejected as worse than one stated uniformly. `SPEC.md` I8 was narrowed to say so out loud instead of overselling |
| The bulk-rewrite I9 gate is flaky on macOS hosted runners ([#36](https://github.com/breferrari/vigia/issues/36)) | I3, 2026-07-31 | Phase 5 | Failed once at 79.22ms p99 against a 48ms budget and passed on a re-run of the same commit, in a PR that changes no file under `crates/*/src`. The p50 was 2.94ms, better than the reference machine's own 2.36ms, so a p99 27x it is the contention signature `exclusively_timed` already documents rather than a slower machine. It belongs to I9 and [#32](https://github.com/breferrari/vigia/issues/32)'s gate, not to I3, and the first thing it needs is a failure *rate* rather than a fix: one failure and two greens is not a number |
| `take-next`'s pre-flight cannot see an untracked spec prerequisite ([#34](https://github.com/breferrari/vigia/issues/34)) | #32, 2026-07-31 | Phase 5 | The pre-flight's four comparisons are all keyed on `I<n>` tokens and issue metadata, so a prerequisite stated in `SPEC.md` prose with no issue behind it is invisible to every one of them. `SPEC.md` §10 blocked [#5](https://github.com/breferrari/vigia/issues/5) by name ("do this before the soak test") and nothing tracked the blocker, which is how it survived two phases. Tooling rather than an invariant, so it is out of scope for the issue that found it, and it is the same shape as [#20](https://github.com/breferrari/vigia/issues/20) |
| The heat projection's cost follows the file ([#55](https://github.com/breferrari/vigia/issues/55)) | #41's pre-flight, 2026-07-31 | Phase 5 | Not deferred by a session that wanted to avoid it: **nothing had ever taken it**, because it is an open `SPEC.md` §10 bullet that no issue named, which is the exact hole [#34](https://github.com/breferrari/vigia/issues/34) added the fifth comparison for. `heat_of` walks every line of a drawn file's hunks to place a change, so it is the one drawn thing whose cost follows the file rather than the window. Left rather than taken because the bullet carries its own measurement and it is comfortable: the shell frame moved **7.66ms to 8.14ms p99** against 16ms, while the core path, which runs none of this code, moved 6.96ms to 7.29ms on the same runs, so most of the gap is run-to-run variation. It is filed anyway because the *shape* is the one I4 forbids, and [#45](https://github.com/breferrari/vigia/issues/45) is the precedent for what a known instance of a forbidden shape costs when it sits both unfixed and ungated |
| I7 is measured without the highlighter ([#51](https://github.com/breferrari/vigia/issues/51)) | #45, 2026-07-31 | Phase 5 | The same blind spot as #45's, one invariant over: I7's 20.37ms comes from `crates/vigia-core/examples/timings.rs`, which is core-only and builds no `Highlighter`, while the shipped first paint parses whatever the first screenful shows. Measured at **97.85ms** on Rust and **373.94ms** on wide Markdown against a 50ms budget, once per process rather than per file. Out of scope for #45 because it is a different invariant and because it cannot be the reported symptom: a once-per-process cost is not a repeated stutter, and #45's gates warm past it exactly as §7 says a steady-state gate should. The fix is a design question rather than an implementation detail, since the honest options include drawing the first frame unhighlighted, which needs a wake I1 forbids inventing a timer for |

### A shelf entry whose reason has expired: [#19](https://github.com/breferrari/vigia/issues/19)

Deferral is a first-class outcome, but a deferral **reason** is a dated claim like any other, and this one is no longer true. #19 went to Phase 5 because the fix "needs a UI that knows what is visible." Phase 2 shipped that UI. I4 makes the shell diff only what it draws, and [#32](https://github.com/breferrari/vigia/issues/32) measured the consequence directly: over the same fixture and the same event, the core ran **98 of 182 frames over the 16ms budget** and the shell ran **0 of 1060**. The precondition the deferral named has been met for a phase.

What that does *not* mean is that #19 is now urgent. Nothing ships the core alone, so the breach is not on any path a user is on, and pulling it forward would interrupt a phase mid-flight to fix a number no reader can observe. Both of those are real, and they argue opposite ways:

- **For taking it:** principle 2 says the budgets are the product, and this is a *measured* breach of I9 sitting open while Phase 3 keeps building on the same frame path. Every element added before it is fixed is another caller of the walk it is about.
- **For leaving it:** it is invisible in the shipped binary, Phase 3 is half done, and the shelf exists precisely so mid-phase discoveries do not derail the block they surfaced in.

**Recorded as an open decision rather than settled by default**, which is the thing this section is for. What is not acceptable is the state it was in until 2026-07-31: filed under a blocker that had already dissolved, so nobody would revisit it, and no one had chosen either branch. Decide it when Phase 3 closes at the latest.

### What the fifth pre-flight comparison found on its first run

[#34](https://github.com/breferrari/vigia/issues/34) added a fifth direction to `take-next`'s pre-flight: an open `SPEC.md` §10 bullet that no issue names. It ran once and returned **five of eight bullets with nothing behind them**, which is the same failure that let the settle margin block [#5](https://github.com/breferrari/vigia/issues/5) by name for two phases. Four are now filed; the fifth is deliberately not, and that is recorded here rather than left as a silent drop.

| §10 bullet | Filed as | Why there |
|---|---|---|
| The header counts changed files and not changed lines | [#49](https://github.com/breferrari/vigia/issues/49) | Phase 3. The bullet assigns itself to this phase in as many words, and this phase shipped three issues without it being taken or refused, because no query returns a sentence. It is the closest thing in the tracker to the shape #34 was filed about |
| I3's window has never run | [#47](https://github.com/breferrari/vigia/issues/47) | Phase 4. A soak gate is an internal instrument; "flat resources over days" printed beside an installable binary is a public claim, and the longest run behind it is one hour. `491c9a0` corrected the tick to say so |
| Default view: unstaged only, or vs HEAD | [#50](https://github.com/breferrari/vigia/issues/50) | Phase 4. The answer needs a week of real use, and real use needs a release, so the dependency is real rather than a deferral. It is also the most load-bearing open question in the spec: B3's `no unstaged changes` is worded the way it is *because* the comparison is index-relative |
| Rename tracking, and the walk that runs to completion | [#48](https://github.com/breferrari/vigia/issues/48) | Phase 5. One issue rather than two, because §10 couples them itself: *"they stand or fall together."* A scale question, unmeasured at the ten thousand files where it would bite |
| The chrome may be too dim to read | [#60](https://github.com/breferrari/vigia/issues/60) | Phase 5. Split out of [#59](https://github.com/breferrari/vigia/issues/59) after three of its four symptoms were disproved. Acted on twice already, on the strength of a screen rather than a measurement: `DarkGray` was invisible, `DIM` was invisible, and `ansi` now takes colour 7 and gives up being dim at all. What is still open is whether that was necessary, which needs a contrast ratio or a three-terminal reading rather than another opinion |
| `G` leaves the pane short | [#62](https://github.com/breferrari/vigia/issues/62) | Phase 5. **The deferral reason is that it is a ruling, not a bug.** It was implemented in [#61](https://github.com/breferrari/vigia/pull/61) and reverted: it reverses §11.1's `G` ruling and turns two correct gates red, so it wants the spec amended first in its own commit. The cost argument that justified §11.1's version has dissolved, the same way [#19](https://github.com/breferrari/vigia/issues/19)'s did, and that is the thing to re-examine |
| The row wash drops a column under every wide glyph | [#63](https://github.com/breferrari/vigia/issues/63) | Phase 5. Cosmetic, and **not established at any width a reader uses**: the gate aborted on the first width it tried and two live readings show a solid band. The first step is re-running the sweep without the early abort, because this may be a gate that is wrong about its situation rather than a defect |
| Windows: supported target or best-effort | not filed | The one bullet that is a **posture question with no action in it**. It carries no ordering language and names no work, so an issue would be a place to have an argument rather than a thing to do. It belongs in Phase 4's release notes, and if that turns out to be wrong the cost is one `gh issue create` |

### And what it found on its second run: [#55](https://github.com/breferrari/vigia/issues/55)

The check ran again before [#41](https://github.com/breferrari/vigia/issues/41) was taken and returned **one bullet that was not in the five above**: `heat_of` walks a whole file to place a change, recorded by [#39](https://github.com/breferrari/vigia/issues/39) and tracked by nothing. It was not missed on the first run. It **did not exist** on the first run, because #39 had not landed.

That is the part worth recording, and it is not the same claim as "the check works". A drift check that only ever pays on the backlog it was written against is a one-off audit wearing a check's clothes. This one caught a bullet that a *later* commit created, which is the only evidence that it is a check at all, and it says the fifth comparison earns its place in the pre-flight rather than having earned it once.

The rest of the pre-flight came back clean in all four other directions, which is the other half of the same point: [#20](https://github.com/breferrari/vigia/issues/20)'s own warning is that **a drift check that cannot report "no drift" has not been tested**, and four silent comparisons beside one that fired is what a tested check looks like.

## Pull-forward log

Items that moved into an *earlier* phase than planned. Recorded for the same reason as deferrals: movement should be visible. Over time the balance of this list against the shelf says whether the plan was too ambitious or too cautious.

| Item | Moved | Why |
|---|---|---|
| `notify` named as a dependency in `SPEC.md` §6 | Into Phase 1 | I1 requires filesystem events rather than a timer, so the choice could not wait for the shell. Cross-platform C-toolchain-free status verified on all three tier-1 targets at the same time |
| Terminal restoration implemented with the shell, ahead of I8 | Into [#9](https://github.com/breferrari/vigia/issues/9) | Not scope creep and not I8 done early. A shell that takes the alternate screen without giving it back is not shippable at any stage, and `panic = "abort"` means a `Drop` alone cannot, so the panic hook had to land with the code that takes the screen. What [#8](https://github.com/breferrari/vigia/issues/8) still owns is the whole of its proof, which is also the whole of the invariant |
| I2 split into I2a and I2b | During Phase 1 | The original I2 conflated incremental re-diffing with incremental re-highlighting. Different dependencies, different phases, and Phase 1 could not close while one number meant two things |
| The memory readout's Windows path ([#56](https://github.com/breferrari/vigia/issues/56)) | Into [#41](https://github.com/breferrari/vigia/issues/41), from Phase 5, twenty minutes after being shelved | Filed and closed inside one pass, which is why it is here rather than on the shelf: **the deferral reason was wrong, not merely outgrown.** #56 claimed `GetProcessMemoryInfo` needs `windows-sys` and that this would be *a new crate in the graph*, unlike `libc` on macOS. `cargo tree -i windows-sys@0.61.2 --target x86_64-pc-windows-msvc` returns a non-dev path through `gix-sec`, so the two are the same case and `SPEC.md` §6 had said so since Phase 1: the `-sys` crates `notify` pulls "are FFI declarations against facilities the OS already ships". The reusable half is not the fact but **how it was got wrong**: the macOS route was checked with `cargo tree` and the Windows route was assessed from memory of what a dependency costs, in the same hour, by the same reasoning that should have applied to both. The 42.8ms `tasklist` measurement was never the error and is still in the spec, one job over: it is why `soak.rs` samples RSS a fixed 288 times across its window instead of per frame |
| Fast scrolling drops frames ([#45](https://github.com/breferrari/vigia/issues/45)) | Into Phase 3, not Phase 5 | Every sibling finding ([#15](https://github.com/breferrari/vigia/issues/15) to [#19](https://github.com/breferrari/vigia/issues/19)) went to the shelf, so this one going the other way is the movement worth recording. Those were found by auditing and this was found by **using it**: scrolling a large diff stutters, consistently on a file mixing Japanese, emoji and Latin. I9 measured over a synthetic Rust fixture is not the claim; the claim is that this is glanceable beside an agent, and a pane that stutters under the reader's own thumb has stopped being monitor-class. One mechanism is confirmed by reading, that a drawn row costs its whole line rather than the pane's width, and it is deliberately **not** the reason it is here: the first commit on it is a measurement, because [#32](https://github.com/breferrari/vigia/issues/32) already closed with "measure before narrowing it" and both halves of that intuition were wrong |

---

## How work is taken

One task, taken to done, before the next. See `.claude/skills/take-next/`.
