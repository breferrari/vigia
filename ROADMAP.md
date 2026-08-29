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

**What has not run is the window in the budget.** An hour is what a session can measure; four hours is what CI will, nightly on Linux and weekly on all three targets; 24 hours needs a runner without the cap. The gate exists and fires, and the number it is standing on is an hour, which `SPEC.md` §10 records as open rather than closed.

**Phase 2 is closed.** The shell draws the working-tree diff, follows what changed with nothing pressed, scrolls by key and wheel, degrades to forty columns without cutting a hint in half, restores the terminal on every exit the process controls, re-highlights only the hunks that changed, and holds its resources flat over a run measured in minutes locally and hours nightly. Every invariant in `SPEC.md` §3 now has a test that fails when it is violated. What the phase turned up and did not fix is on the shelf below, with an issue and a milestone each.

## Phase 3 — glanceability

Milestone: [Phase 3](https://github.com/breferrari/vigia/milestone/3)

**Phase 3 re-opened for [#65](https://github.com/breferrari/vigia/issues/65) and closed again.** `vigia` drew a file as **+905 -885** while `git diff` reported it unchanged, on default Windows configuration: the frame path compared raw worktree bytes against the index blob, where git runs the worktree side through the clean filter first. It went first because of what it cost rather than what it cost to fix — every budget in this file is a claim about a tool that shows what changed, and on one of three tier-1 targets it was showing a thousand lines of noise. Invisible to the whole suite, for the reason `SPEC.md` §7 now carries as a rule.

**Two lessons this phase produced that are not about any of its rows.** A gate can be wrong about its *situation* as well as about its assertion, and reading cannot tell the two apart — the replacement for #57's gate asserted the right outcome about a case the fixture could never reach, and only mutation found it. And **measure the artifact, not a picture of it**: #59 was filed on four symptoms read off video, a probe disproved the mechanism outright, and two of the four turned out to be mis-cropped frames. What the probe did find was worth the trip — the terminal size *changes* when the alternate screen is entered under Warp, and `Shell::area` was reading it with its own syscall while `Terminal::draw` read it again.

| | Task | Issue |
|---|---|---|
| ✅ | I10 bounded history, and the sparkline, gradient and pulse drawn from it | [#38](https://github.com/breferrari/vigia/issues/38) |
| ✅ | The heat strip, and the whole-file line count it needs | [#39](https://github.com/breferrari/vigia/issues/39) |
| ✅ | The header mode word, the mode set, and the empty state (B3) | [#40](https://github.com/breferrari/vigia/issues/40) |
| ✅ | Fast scrolling drops frames, and a drawn row costs its whole line | [#45](https://github.com/breferrari/vigia/issues/45) |
| ✅ | Fast trackpad scrolling over a large diff falls behind the hand | [#54](https://github.com/breferrari/vigia/issues/54) |
| ✅ | The header carries no changed-line total, and §10 closed with the reason | [#49](https://github.com/breferrari/vigia/issues/49) |
| ✅ | The status bar: frame time and RSS, on all three tier-1 targets | [#41](https://github.com/breferrari/vigia/issues/41) |
| ✅ | A viewport past the end of the diff drew one row and blanked the screen | [#57](https://github.com/breferrari/vigia/issues/57) |
| ✅ | On Windows every CRLF file read as a full rewrite | [#65](https://github.com/breferrari/vigia/issues/65) |
| ✅ | I3: the restart left a second screenful of hunk parses in the pass | [#64](https://github.com/breferrari/vigia/issues/64) |
| ✅ | Theming, with a 256-colour degradation path | [#11](https://github.com/breferrari/vigia/issues/11) |
| ✅ | A macOS reader gets no row wash, because detection under-claimed the depth and 256 cannot hold one | [#103](https://github.com/breferrari/vigia/issues/103) |
| ✅ | Scrolling into the tail of a diff leaves the pane half empty | [#59](https://github.com/breferrari/vigia/issues/59) |

**[#10](https://github.com/breferrari/vigia/issues/10) was split before anything here was taken**, which this file had blocked the phase on. Reading `assets/preview.svg` as the specification it already is (`SPEC.md` §5.1) turned up eight distinct pieces of work behind two rows, and #10 alone carried four features that share no implementation.

The correction that mattered was not the count. **It is that this is not a rendering phase** (`SPEC.md` §5.2), and building the first child confirmed it: the diff was mostly `vigia-core`. The split follows what each element needs rather than what it looks like: history-backed (#38), whole-file-backed (#39), chrome (#40), self-measuring (#41).

**The heat strip is in, and the expensive part of it did not exist.** [#39](https://github.com/breferrari/vigia/issues/39) was the whole-file-backed child, and `SPEC.md` §5.2 had it as the element that pulls hardest against I2a: locating change *within* a file needs the file's length, and measuring that per frame puts back the read I2a removed. §5.2 predicted a cache keyed on `(path, blob id)`.

That is the third time the expensive-looking property turned out to be a by-product of work already being done: I5's follow target and I10's burst paths were the other two, both already resolved by the gitignore filter. §5.2 now says to look for the by-product before designing the cache.

## Phase 4 — the artifacts tell the truth

Milestone: [Phase 4](https://github.com/breferrari/vigia/milestone/4)

**Filter: does this close a gap between what a published artifact claims and what the binary does?**

> [!NOTE]
> **This phase was called "distribution" until 2026-08-01, and had stopped being a filter**
>
> It held eight issues of which exactly **one** was distribution. Each of the other seven was filed here on the argument that it must land *before* shipping — which is a statement about **ordering**, not about membership, and the two were quietly treated as the same thing. Principle 1 says a phase name is something you can quote back at a proposal to kill or delay it, and "distribution" cannot reject a sparkline bug. A milestone that accepts anything is one `take-next` cannot take from: step 1 asks for the earliest phase with open work and would have answered "distribution" for a month of work that distributes nothing. **Each phase now carries its filter in bold under its heading, so the next mis-file has something to fail against.**

| | Task | Issue |
|---|---|---|
| ✅ | B4 ruled as layout: the file list is a pinned, scrollable region | [#66](https://github.com/breferrari/vigia/issues/66) |
| ✅ | The header's two facts compose into a claim the tool does not make | [#67](https://github.com/breferrari/vigia/issues/67) |
| ✅ | The mockup lays the glance row in columns; the shell right-packs it | [#77](https://github.com/breferrari/vigia/issues/77) |
| ✅ | Every file has an empty sparkline at launch, which is the common case | [#78](https://github.com/breferrari/vigia/issues/78) |
| ✅ | The pulse says one thing twice, so the label goes and the dot stays | [#102](https://github.com/breferrari/vigia/issues/102) |
| ✅ | The mockup gives the footer's readouts three colours, and the shell draws one grey | [#92](https://github.com/breferrari/vigia/issues/92) |
| ✅ | The pane-not-contents ruling, in the two elements #77 does not touch | [#93](https://github.com/breferrari/vigia/issues/93) |

**The first two rows came before the other two, and the order was not preference.** Both are done. Both were places where the picture and the binary disagreed, and a disagreement that ships is a support burden rather than a stale artifact.

**And one panic was a simplification's bill.** The heat strip used to be drawn through `set_stringn`, which clips; it was traded for a direct cell write to stop an allocation per cell, and the clipping went with it unremarked, so an area wider than its buffer aborted the whole paint on the one row carrying a strip. The rule and the scrollbar had the same trade made earlier and the same hole. All three take the clipping form now, which makes `render`'s own documented contract true on the width axis; the height axis is [#91](https://github.com/breferrari/vigia/issues/91).

None of them was reachable by `take-next`'s pre-flight, which compares invariant tokens and issue metadata. **A picture is a specification that carries neither**, which is the same blind spot [#40](https://github.com/breferrari/vigia/issues/40) hit when it found B5 shipped while still marked `(proposed)`, and the reason this phase exists as its own thing rather than as polish inside another.

The ordering question this row worried about resolved the other way from both options it names. `History` is free to rank by and was still rejected, because ranking the list by anything the stream is not ordered by decouples the caret from the scroll position and the region stops being a *map*. Status order is free too, and it is the only order under which the two regions describe the same place.

## Phase 6 — measured, not assumed

Milestone: [Phase 6](https://github.com/breferrari/vigia/milestone/6)

**Filter: can this be decided without a number?** If yes, it belongs somewhere else.

| | Task | Issue |
|---|---|---|
| ✅ | The soak could not report the slope §10 needs, and the workflow killed the window it offers | [#112](https://github.com/breferrari/vigia/issues/112) |
| ✅ | I3's day-long window has run, and §10 carries its reading: 1.85% against 5% | [#47](https://github.com/breferrari/vigia/issues/47) |
| ✅ | The drift gate's warmup is workload-dependent, and the report says so beside the verdict | [#126](https://github.com/breferrari/vigia/issues/126) |
| ✅ | The height walk re-read every undrawn file on every tick, and every wall-clock gate materialised past it | [#101](https://github.com/breferrari/vigia/issues/101) |
| ✅ | An opt-in observation log is declined, and the monitor writing nothing stops being an accident | [#117](https://github.com/breferrari/vigia/issues/117) |

**Closed 2026-08-07, with its last two questions shelved rather than answered, and the reason is the lesson.** [#72](https://github.com/breferrari/vigia/issues/72) and [#50](https://github.com/breferrari/vigia/issues/50) close from **use** — a week of the pane open beside real sessions — and a pass cannot produce a week. Leaving them here kept this phase perpetually "next", so the queue kept serving measurement passes to questions only living could answer: a session took #72 the same day the queue was supposed to serve the release, built three observers, and the one thing of product value it produced ([#129](https://github.com/breferrari/vigia/issues/129), a real-use feel defect) had nothing to do with any instrument being built. The order is the strategy; a phase that cannot close by work does not get to sit at the front of it.

**Separated from Phase 4 because these are blocked on calendar time rather than on engineering time.** [#72](https://github.com/breferrari/vigia/issues/72) needs a real working session beside a real agent, and [#47](https://github.com/breferrari/vigia/issues/47) needs a runner without a six-hour cap. [#112](https://github.com/breferrari/vigia/issues/112) is the exception and sits above #47 because it was split out of it: reading #47's acceptance list against the code found that two of its four criteria had engineering prerequisites rather than calendar ones, and that both prerequisites blocked the rest. The soak reported no gradient, so §10's +0.92 against −0.70 could only ever have been re-derived by hand off a printed series; and the workflow pinned a 5.5-hour timeout under an input offering 86400, so the day-long dispatch died of this repository rather than of the platform. Neither is visible from #47's title, which is why it went two phases unnoticed. Bundled with the artifact fixes, a phase of one-afternoon changes would sit open behind a day of waiting, and a phase that cannot close stops being a unit of work.

[#50](https://github.com/breferrari/vigia/issues/50) is here rather than anywhere else because it **cannot** be answered from a fixture: §10 says *"confirm against a week of real use"*, and until [#72](https://github.com/breferrari/vigia/issues/72) that week had no issue behind it, which is how #50 sat in a phase for a fortnight being permanently unresolvable.

[#72](https://github.com/breferrari/vigia/issues/72) also pays for itself outside this phase. Four shelf entries — [#19](https://github.com/breferrari/vigia/issues/19), [#16](https://github.com/breferrari/vigia/issues/16), [#48](https://github.com/breferrari/vigia/issues/48), [#73](https://github.com/breferrari/vigia/issues/73) — are deferred on judgements about how many files are really dirty at once and how often. One session converts all four from judgement into evidence, and [#76](https://github.com/breferrari/vigia/issues/76) is the check that would notice when it does.

## Phase 7 — distribution

Milestone: [Phase 7](https://github.com/breferrari/vigia/milestone/7)

**Filter: does this put the binary in someone else's hands?**

| | Task | Issue |
|---|---|---|
| ✅ | An external kill leaves the terminal in raw mode | [#24](https://github.com/breferrari/vigia/issues/24) |
| ✅ | `cargo-dist`, crates.io, Homebrew tap | [#12](https://github.com/breferrari/vigia/issues/12) |

**[#12](https://github.com/breferrari/vigia/issues/12) is done: v0.1.0 shipped 2026-08-09.** [`vigia 0.1.0`](https://crates.io/crates/vigia) and `vigia-core 0.1.0` are on crates.io, neither yanked, so both names are claimed permanently, which is what this row was always about. The [GitHub release](https://github.com/breferrari/vigia/releases/tag/v0.1.0) carries four target archives, both install scripts and their checksums. `cargo install vigia` was verified cold on a machine with no path override: it resolved `vigia-core` from the registry and the installed binary reports `vigia 0.1.0`. **One channel is short.** The Homebrew push was denied 403 because the tap token can read and not write, so `brew install breferrari/tap/vigia` does not work yet; [#137](https://github.com/breferrari/vigia/issues/137) carries the diagnosis and the one-job re-run that fixes it. That failure landed on the reversible side of an ordering chosen for it: crates.io runs last, after everything that can be deleted and redone. **What the row used to say, and why it is worth keeping:** The pipeline landed 2026-08-08: `cargo-dist` 0.32 with four targets and three installers, a generated `release.yml`, a custom publish job that sends both crates to crates.io, the tap created, the version at 0.1.0, and `crates/vigia/tests/package.rs` giving `SPEC.md` §9 the gates it had never had. What has not happened is the **tag**, and it cannot happen from a session: it needs two secrets that only a token holder can set, and a `RELEASE-SMOKE.md` run on macOS and Linux hardware. The row flips when `v0.1.0` is pushed and §5 is ticked. **The name is still unclaimed until then**, because crates.io has no reservation mechanism and publishing is the only claim, which is the fact §9 has recorded since 2026-07-30 and which was re-verified against `index.crates.io` on 2026-08-08.

**[#24](https://github.com/breferrari/vigia/issues/24) was here because the filter includes what the binary does in those hands.** A pane closing sends the signal nobody at the keyboard typed, a tool that lives in panes will be killed that way routinely, and a first-run user whose terminal comes back wrecked does not file an issue, they uninstall. **Landed 2026-08-08, and taken symmetrically**: the single-task version was Unix-only, [#16](https://github.com/breferrari/vigia/issues/16) had already rejected a guarantee that means different things on different tier-1 targets, and what made the uniform answer affordable was a measurement rather than a principle — neither half adds a crate to any graph, because `signal-hook` is already in both Unix graphs through crossterm and the Windows half is a feature of a crate `vigia` already declares. The other two shelf entries that read like release blockers by title were re-examined 2026-08-05 and stay shelved on their own recorded triage: [#91](https://github.com/breferrari/vigia/issues/91)'s panic is unreachable from the binary (an API-contract gap), and [#63](https://github.com/breferrari/vigia/issues/63)'s hole is real in the buffer and unreachable on the screen.

**The release itself has a smoke checklist, [`RELEASE-SMOKE.md`](RELEASE-SMOKE.md)**, run against the built artifact before the tag that triggers the publish — because a sibling project shipped two consecutive CI-green patches that broke the flagship install on day one, and a crates.io publish is permanent in a way theirs was not.

## Phase 8 — look and feel

Milestone: [Phase 8](https://github.com/breferrari/vigia/milestone/8)

**Filter: does it change what the pane looks or feels like in the first minute of use?** Shape, colour, and input.

| | Task | Issue |
|---|---|---|
| ✅ | Nothing shows staged files, so an agent that stages its own work empties the pane | [#313](https://github.com/breferrari/vigia/issues/313) |
| ✅ | The list becomes a left rail on its own, and it should be a toggle the reader asks for | [#295](https://github.com/breferrari/vigia/issues/295) |
| ✅ | The selector has no arrow key, and the arrows the reader tried scroll the list instead | [#296](https://github.com/breferrari/vigia/issues/296) |
| ✅ | Nothing pins the pane to one file, so reading a file means scrolling past its end into the next | [#297](https://github.com/breferrari/vigia/issues/297) |
| ✅ | decision: nothing carries a view default. **Ruled: a second file**, `~/.config/vigia/config` | [#306](https://github.com/breferrari/vigia/issues/306) |
| ✅ | Build: the ruling says a reader can set view defaults and nothing reads the file | [#309](https://github.com/breferrari/vigia/issues/309) |
| ✅ | Braille returns to the band, and the rung stops being a constant | [#244](https://github.com/breferrari/vigia/issues/244) |
| ✅ | The mockup insets its text and the shell draws full-bleed — a fourth departure, unrecorded | [#119](https://github.com/breferrari/vigia/issues/119) |
| ✅ | Half-page scroll: `d` and `u`, the `less` bindings | [#121](https://github.com/breferrari/vigia/issues/121) |
| ✅ | File-granular navigation: `n`/`p`, digits jump to a listed file | [#122](https://github.com/breferrari/vigia/issues/122) |
| ✅ | decision: should the pinned list draw the rank the digits address? | [#149](https://github.com/breferrari/vigia/issues/149) |
| ✅ | decision: OSC 8 links and a yank key — outbound affordances that write nothing | [#120](https://github.com/breferrari/vigia/issues/120) |
| ✅ | decision: hover highlight, reopened because both reasons it was declined on were false | [#123](https://github.com/breferrari/vigia/issues/123) |
| ✅ | A hover mark on the scrollbars' step buttons, and the mechanism the rest will reuse | [#186](https://github.com/breferrari/vigia/issues/186) |
| ✅ | The hover mark where no rung existed, so one was built: a list row, the thumb, the track | [#189](https://github.com/breferrari/vigia/issues/189) |
| ✅ | decision: the rule learns to speak — headings in the border line | [#124](https://github.com/breferrari/vigia/issues/124) |
| ✅ | The counters are one dim grey where the picture draws them green and red | [#157](https://github.com/breferrari/vigia/issues/157) |
| ✅ | The heat strip draws one bar where the picture draws twelve slices, and the sparkline is flat where it ramps | [#196](https://github.com/breferrari/vigia/issues/196) |
| ✅ | The store samples at the sparkline's width, so a wide graph has eight points to draw | [#198](https://github.com/breferrari/vigia/issues/198) |
| ✅ | The worktree churn graph: the hero element nothing draws | [#158](https://github.com/breferrari/vigia/issues/158) |
| ✅ | The masthead starts drawn, and the reader who asked for the toggle wants it hidden | [#204](https://github.com/breferrari/vigia/issues/204) |
| ✅ | Braille resolution as a glyph rung above the block ramp | [#159](https://github.com/breferrari/vigia/issues/159) |
| ✅ | Support every modern language: the bundled grammar set is a Sublime Text 3 snapshot | [#235](https://github.com/breferrari/vigia/issues/235) |
| ✅ | The list keeps six rows on a fifty-row pane, so the map is void where it could be | [#160](https://github.com/breferrari/vigia/issues/160) |
| ✅ | The glance elements are postage stamps at width: bucket counts as a width rung | [#161](https://github.com/breferrari/vigia/issues/161) |
| ✅ | The sparkline's bucket ceiling was the band's period, and that period is gone | [#234](https://github.com/breferrari/vigia/issues/234) |
| ✅ | The region model assumes a vertical stack, so a rail cannot be expressed in it | [#251](https://github.com/breferrari/vigia/issues/251) |
| ✅ | An agent's write is how the grammar compile arrives, and the warmer only ever ran at launch | [#129](https://github.com/breferrari/vigia/issues/129) |
| ✅ | Follow jumps to the changed file's heading, so a change low in a long file lands off screen | [#257](https://github.com/breferrari/vigia/issues/257) |
| ✅ | decision: may the graph age. **Ruled: it ages** | [#243](https://github.com/breferrari/vigia/issues/243) |
| ✅ | Build: the window ages on a clock that stops when it empties | [#277](https://github.com/breferrari/vigia/issues/277) |
| ✅ | One loud burst sets the band's yardstick, so a sparse window draws as spikes on a floor | [#256](https://github.com/breferrari/vigia/issues/256) |
| ✅ | Three paint marks name a region by its first row, which a rail makes ambiguous | [#254](https://github.com/breferrari/vigia/issues/254) |
| ✅ | Side-by-side regions at real width: the list becomes a left rail | [#252](https://github.com/breferrari/vigia/issues/252) |
| ✅ | The sigil sits flush against the line, where the picture puts a column between them | [#164](https://github.com/breferrari/vigia/issues/164) |
| ✅ | Nothing separates a file's last line from the next file's heading | [#165](https://github.com/breferrari/vigia/issues/165) |
| ✅ | The scrollbar has no step buttons, and a held button cannot repeat | [#166](https://github.com/breferrari/vigia/issues/166) |
| ✅ | The hint bar names four gestures of roughly thirteen | [#80](https://github.com/breferrari/vigia/issues/80) |
| ✅ | decision: the keymap outgrew the hint bar, so does it get an overlay | [#167](https://github.com/breferrari/vigia/issues/167) |
| ✅ | Build B12: `?` toggles a centred gestures sheet over the pane | [#206](https://github.com/breferrari/vigia/issues/206) |
| ✅ | The sheet keeps the diff colours it covers, and its close control never brightens | [#211](https://github.com/breferrari/vigia/issues/211) |
| ✅ | The sheet is 56 columns whatever the pane is, so a short one drops the mouse group | [#220](https://github.com/breferrari/vigia/issues/220) |
| ✅ | The sheet has no roomy rung: no air, no sections, and a reorder that would invert the keep-set | [#285](https://github.com/breferrari/vigia/issues/285) |
| ✅ | At the residual floor the sheet still drops gestures in silence, and reaching them is an input-model ruling | [#286](https://github.com/breferrari/vigia/issues/286) |
| ✅ | The sheet omits a gesture the README teaches, and the gate for exactly that is a hand-written list | [#288](https://github.com/breferrari/vigia/issues/288) |
| ✅ | The close control's pressed weight is unreachable, and its docblock claims three | [#298](https://github.com/breferrari/vigia/issues/298) |
| ✅ | The churn band samples once a second, so a bursty worktree draws scatter | [#223](https://github.com/breferrari/vigia/issues/223) |
| ✅ | Attribute a wall-clock overshoot with thread CPU time | [#212](https://github.com/breferrari/vigia/issues/212) |
| ✅ | The status sigil sits two columns apart between the list and the diff's headings | [#173](https://github.com/breferrari/vigia/issues/173) |
| ✅ | The header stands against the first list row, the one boundary drawn with nothing | [#174](https://github.com/breferrari/vigia/issues/174) |
| ✅ | The scrollbar's track is flush right, where the glyph that centres it is also inside CP437 | [#175](https://github.com/breferrari/vigia/issues/175) |
| ✅ | The columns left of the diff's scrollbar read as neither wash nor track | [#214](https://github.com/breferrari/vigia/issues/214) |
| ✅ | The row wash has no left bar, and the reason it was refused expired with #119 | [#218](https://github.com/breferrari/vigia/issues/218) |
| ✅ | decision: text cannot be selected or copied. **Ruled: the terminal selects (B20), `y` copies the path (B9)** | [#177](https://github.com/breferrari/vigia/issues/177) |
| ✅ | Build B9: `y` copies the caret file's path over OSC 52 | [#372](https://github.com/breferrari/vigia/issues/372) |
| ✅ | Bulk-rewrite settle guard fails on loaded musl runners | [#352](https://github.com/breferrari/vigia/issues/352) |
| ⬜ | The masthead graph draws a flat track and one spike, and the spike does not sit in the band | [#348](https://github.com/breferrari/vigia/issues/348) |
| ⬜ | decision: a bulk write marks every row with the pulse, so the mark says nothing on the shape an agent produces | [#362](https://github.com/breferrari/vigia/issues/362) |
| ⬜ | Turning wrap off leaves the scroll range at the wrapped row count, so scrolling goes erratic. **Reported from use** | [#364](https://github.com/breferrari/vigia/issues/364) |
| ⬜ | research: price animation on an arriving change | [#365](https://github.com/breferrari/vigia/issues/365) |
| ⬜ | watch.rs evicts an arbitrary path from a HashSet | [#368](https://github.com/breferrari/vigia/issues/368) |
| ✅ | `cargo install vigia` fails on a yanked `bisync` pinned through gix 0.86 | [#349](https://github.com/breferrari/vigia/issues/349) |
| ✅ | A long line cannot be read to its end, and the ruling against wrapping was made without a toggle in it | [#272](https://github.com/breferrari/vigia/issues/272) |
| ✅ | The pulse leaves the last edited file after a second, and it used to stay | [#345](https://github.com/breferrari/vigia/issues/345) |
| ✅ | The pointer's mark is the loudest weight in the list, and the file the diff is inside has none | [#193](https://github.com/breferrari/vigia/issues/193) |
| ✅ | The scrollbar cuts a one-column hole through every washed row | [#239](https://github.com/breferrari/vigia/issues/239) |
| ⬜ | A wide pane repeats the band's samples rather than resolving them | [#241](https://github.com/breferrari/vigia/issues/241) |
| ✅ | The glance elements draw spikes where the picture draws a wave | [#242](https://github.com/breferrari/vigia/issues/242) |
| ✅ | The pane draws with a fraction of the vocabulary its own font guarantees | [#318](https://github.com/breferrari/vigia/issues/318) |
| ✅ | Every hue is a theme key, and the default theme is the showcase | [#320](https://github.com/breferrari/vigia/issues/320) |
| ✅ | The diff learns the delta formula: calm washes, hot words, a two-tone gutter | [#321](https://github.com/breferrari/vigia/issues/321) |
| ✅ | The glance ramps become gradients and the band goes with them | [#322](https://github.com/breferrari/vigia/issues/322) |
| ✅ | The chrome earns 2026: segmented bars, a spliced sheet title, opt-in icons | [#323](https://github.com/breferrari/vigia/issues/323) |
| ✅ | The glyph ladder learns which terminals draw octants natively | [#324](https://github.com/breferrari/vigia/issues/324) |
| ✅ | The pane takes its colours from the terminal, and follows a theme flip live | [#325](https://github.com/breferrari/vigia/issues/325) |
| ⬜ | A theme flip mid-session cannot reach the shell, and the blocker is crossterm's parser | [#332](https://github.com/breferrari/vigia/issues/332) |
| ⬜ | A system palette built from the terminal's own colours | [#333](https://github.com/breferrari/vigia/issues/333) |
| ✅ | A path is a link: OSC 8 on the list and the headings | [#326](https://github.com/breferrari/vigia/issues/326) |
| ✅ | The pane shows what is no longer there, and `Esc` quits from the help sheet | [#340](https://github.com/breferrari/vigia/issues/340) |
| ✅ | The staged mark spends a gutter column on every row of both runs to mark the rows of one | [#316](https://github.com/breferrari/vigia/issues/316) |
| ✅ | Side-by-side regions at real width: the list becomes a left rail | [#162](https://github.com/breferrari/vigia/issues/162) |

**The phase's filter is its own instrument.** Rows arrive here from a reader watching the pane, not from a derivation, and the pattern held: five rows moved to the front on 2026-08-21 and three more on 2026-08-24, every one of them a gesture somebody reached for and did not find, or a hitch somebody felt. That is the only instrument that produces this filter, and [#72](https://github.com/breferrari/vigia/issues/72) is what widens it.


**Ordering.** [#119](https://github.com/breferrari/vigia/issues/119) sat first because it moves layout boundaries the other rows would otherwise re-derive twice. **After distribution on purpose**: the minimal real thing ships first — a crates.io name is claimed by publishing — and polish lands as visible post-release momentum, guided by [#72](https://github.com/breferrari/vigia/issues/72)'s real-use data instead of ahead of it.


**A `decision` row is one whose feel improvement costs an invariant a sentence; one that costs nothing is an ordinary row.** The ruling lands in `SPEC.md` §11.2 and the build follows in the same pass unless it is genuinely too large for one — [#167](https://github.com/breferrari/vigia/issues/167) shipped a ruling with nothing on screen for four hours, and a reader pressed `?` and got nothing.


**Four rows of this phase turned on a premise that did not survive being checked, and B10 did it twice.** Worth stating once as a pattern rather than five times as a coincidence: **a `decision` issue's own framing is a dated claim like any other**, and in B10's case the false premise was the spec's own sentence as well as the issue's. The count is rows and not failures, because the row is what gets taken. The lesson B10 was first read for was too small — *the sentence has to be true when it is written* — and the sentence that replaced it was written one paragraph later and checked no harder. **The larger rule is that a conclusion which outlives the reason it was given is the thing to be suspicious of.** §5.3 carries it.


**What is left open and is nobody's bug yet.** The rail is 70 columns until a 213-column pane, so a long path elides beside its cluster. That is the same elision every label on this pane already makes and it keeps the tail, which is what identifies the file; whether the rail should take a larger share so common paths draw whole is a look-and-feel question the pane will answer faster than an argument here will. The lever is one constant.


**Every PR in this phase carries a screenshot, and the bar is sized to the surface.** Feel is vetoed by eye or it is not vetoed at all.

## Shelf

Milestone: [Shelf](https://github.com/breferrari/vigia/milestone/5)

Everything on the deferral shelf below has a milestone here, so shelved work is still reachable by a milestone-filtered query rather than only readable in prose. The shelf carries the *reason*; this table carries the *state*.

**It carried the name "Phase 5 — deferred findings" until 2026-08-06, and the number is retired because it was a lie with a good excuse.** A phase number claims a place in a sequence; this is a shelf, permanently open, never "next", and the file spent a paragraph fighting its own name. The sections above run 4 → 6 → 7 → 8 in the order they are meant to be taken, and the shelf sits after all of them because it holds no place among them at all. What survives the rename: the milestone URL keeps `/5`, and older issues, notes and the dated cells below cite "Phase 5" — every such citation means this shelf. The exclusion mechanism never rested on the name: `take-next` step 1 skips it by the `Shelf:` description prefix, which is unchanged, and a title with no phase number now also sorts last by that query's own fallback, so the two guards finally agree instead of one covering for the other.

**A shelf item comes off it when daily use asks for it, and the asking is the whole test.** That sentence used to live here, was deleted at some point, and survives only as a quotation in two pull-forward rows below, which is a rule that exists solely as its own citation. Restated because a filter nobody can read is not one, and because the sixty-odd rows under it are what happens when the entry trigger fires on every audit and the exit trigger fires on a judgement nobody is scheduled to make.

**The occasion is the pull-forward, and the question is one line: would this be built if it were not already written down?** Ask it of an item when something reaches for it, not on a schedule nobody keeps. An item that cannot answer yes is not waiting for a phase, it is declined, and closing it as such is a result rather than a loss. Five declines in a hundred and twenty-seven closed issues is not a shelf being filtered; it is a shelf being filled.

**If a second shelf is ever created, its milestone description must begin `Shelf:`.** Until [#83](https://github.com/breferrari/vigia/issues/83) the never-next rule lived only in this paragraph, which is prose, and `take-next` step 1 is a query: it read the milestone list, saw three peers, and offered the shelf as the next phase. The marker is what a query can read, and this paragraph is where whoever creates the next one is standing, so it is stated here rather than only in the skill. Comparison 6 of that skill's pre-flight is the check that fires when the two disagree.

| | Task | Issue |
|---|---|---|
| ✅ | A symlink diffs as its target's contents, and on Windows was never reusable | [#15](https://github.com/breferrari/vigia/issues/15) |
| ✅ | The macOS watch suite fails three different ways under CI load, and it is blocking merges | [#337](https://github.com/breferrari/vigia/issues/337) |
| ⬜ | The caret row's weight is the one modifier a theme file cannot reach | [#195](https://github.com/breferrari/vigia/issues/195) |
| ⬜ | The sheet's tables are audited, not derived, so the keymap can still drift into them | [#312](https://github.com/breferrari/vigia/issues/312) |
| ⬜ | The fingerprint cannot see a timestamp-preserving write | [#16](https://github.com/breferrari/vigia/issues/16) |
| ⬜ | Two paths differing outside UTF-8 collapse onto one cache key | [#17](https://github.com/breferrari/vigia/issues/17) |
| ⬜ | A frame reads a whole file to discover it is binary | [#18](https://github.com/breferrari/vigia/issues/18) |
| ⬜ | An idle frame is one `stat` per changed file | [#19](https://github.com/breferrari/vigia/issues/19) |
| ✅ | `take-next`: the pre-flight cannot see an untracked spec prerequisite | [#34](https://github.com/breferrari/vigia/issues/34) |
| ⬜ | The bulk-rewrite I9 gate is flaky on macOS hosted runners | [#36](https://github.com/breferrari/vigia/issues/36) |
| ⬜ | Rename tracking and the non-streaming walk, at ten thousand files | [#48](https://github.com/breferrari/vigia/issues/48) |
| ⬜ | The thesis workload, measured from real use rather than a fixture | [#72](https://github.com/breferrari/vigia/issues/72) |
| ⬜ | Default view: unstaged only, or working-tree-vs-HEAD | [#50](https://github.com/breferrari/vigia/issues/50) |
| ✅ | I7 is measured without the highlighter, and the first parse costs 98ms | [#51](https://github.com/breferrari/vigia/issues/51) |
| ⬜ | The heat projection's cost follows the file rather than the window | [#55](https://github.com/breferrari/vigia/issues/55) |
| ⬜ | The chrome may be too dim to read on a real terminal | [#60](https://github.com/breferrari/vigia/issues/60) |
| ⬜ | `G` leaves the pane short, and the first scroll yanks it back a screenful | [#62](https://github.com/breferrari/vigia/issues/62) |
| ⬜ | The row wash drops a column under every wide glyph | [#63](https://github.com/breferrari/vigia/issues/63) |
| ⬜ | A file the attributes declare binary is diffed as text anyway | [#68](https://github.com/breferrari/vigia/issues/68) |
| ⬜ | An LFS-tracked text file diffs its pointer against its content | [#69](https://github.com/breferrari/vigia/issues/69) |
| ⬜ | The settle margin's cost is bounded structurally and unbounded temporally | [#73](https://github.com/breferrari/vigia/issues/73) |
| ⬜ | The `gix` status surface is load-bearing on every budget with no seam | [#74](https://github.com/breferrari/vigia/issues/74) |
| ⬜ | A push to `main` was cancelled where the concurrency guard says it cannot be | [#75](https://github.com/breferrari/vigia/issues/75) |
| ⬜ | `take-next`: a deferral reason is a dated claim and nothing re-reads one | [#76](https://github.com/breferrari/vigia/issues/76) |
| ✅ | The wheel ignores the pointer, and the thumb it draws cannot be grabbed | [#79](https://github.com/breferrari/vigia/issues/79) |
| ✅ | A washed row may be reaching the scrollbar column, or the terminal is | [#81](https://github.com/breferrari/vigia/issues/81) |
| ⬜ | The row's two fixed runs allocate a byte each, per content row per frame | [#171](https://github.com/breferrari/vigia/issues/171) |
| ⬜ | A steady worktree saturates half the band, because the factor above the mean was never measured on this signal | [#281](https://github.com/breferrari/vigia/issues/281) |
| ⬜ | A wider pane can take a row off the body, and the diff pays it | [#283](https://github.com/breferrari/vigia/issues/283) |
| ⬜ | The rail's arrival width is derived at the block rung, and a dense rung climbs earlier | [#284](https://github.com/breferrari/vigia/issues/284) |
| ✅ | `take-next` sorts milestones by a field that is null on every one of them | [#83](https://github.com/breferrari/vigia/issues/83) |
| ⬜ | A repeated `base` reports itself with eighteen spaces mid-sentence | [#88](https://github.com/breferrari/vigia/issues/88) |
| ⬜ | The worktree name skips the control-character transformation content rows get | [#89](https://github.com/breferrari/vigia/issues/89) |
| ⬜ | `render` promises any area is legal, and an area taller than its buffer panics | [#91](https://github.com/breferrari/vigia/issues/91) |
| ⬜ | The diff's total height is taken from the cache by presence, not by validity | [#84](https://github.com/breferrari/vigia/issues/84) |
| ⬜ | `FrameStats::bytes` conflates bytes counted with bytes diffed | [#85](https://github.com/breferrari/vigia/issues/85) |
| ⬜ | `Worktree::measure` has no test over a real repository | [#86](https://github.com/breferrari/vigia/issues/86) |
| ✅ | `take-next`: pre-flight the spec against the tracker | [#20](https://github.com/breferrari/vigia/issues/20) |
| ⬜ | The heat strip and scrollbar tracks resolve to the colour of the pane behind them | [#98](https://github.com/breferrari/vigia/issues/98) |
| ⬜ | The character walk is bounded per span rather than per row | [#106](https://github.com/breferrari/vigia/issues/106) |
| ⬜ | The take-order is derived from milestone titles, when the roadmap already holds it | [#108](https://github.com/breferrari/vigia/issues/108) |
| ⬜ | A `core.autocrlf` or `.git/info/attributes` change is invisible to the cache guard | [#111](https://github.com/breferrari/vigia/issues/111) |
| ✅ | A denied rustdoc lint that no job runs | [#131](https://github.com/breferrari/vigia/issues/131) |
| ⬜ | `take-next` reads Copilot's line comments with the wrong login | [#132](https://github.com/breferrari/vigia/issues/132) |
| ⬜ | `MIN_TICKS` restates `MIN_FRAMES`, and the queue it looks like it guards is unbounded | [#114](https://github.com/breferrari/vigia/issues/114) |
| ⬜ | 0.1.1: the crate carries no LICENSE, and Windows posture is still unstated | [#135](https://github.com/breferrari/vigia/issues/135) |
| ⬜ | 0.1.1: trusted publishing, so the crates.io token stops existing | [#141](https://github.com/breferrari/vigia/issues/141) |
| ✅ | The bump cannot move a protected main, because the checks it needs can never arrive | [#143](https://github.com/breferrari/vigia/issues/143) |
| ⬜ | The workflow gates read text, and text has more spellings than the mechanism | [#145](https://github.com/breferrari/vigia/issues/145) |
| ✅ | decision: a bonus hint rung is never worth a row, and nobody ruled whether it is worth a readout | [#147](https://github.com/breferrari/vigia/issues/147) |
| ⬜ | Pointer motion draws a full frame, and I1's letter says it should not | [#154](https://github.com/breferrari/vigia/issues/154) |
| ⬜ | §5.1 says the deliberate departures are two, and enumerates four | [#156](https://github.com/breferrari/vigia/issues/156) |
| ✅ | The absolute frame budgets flake on shared runners, and the failure reads as a regression | [#178](https://github.com/breferrari/vigia/issues/178) |
| ⬜ | Under a default tmux, a non-active pane may get no mouse events at all | [#188](https://github.com/breferrari/vigia/issues/188) |
| ✅ | `App::chrome` takes four pointer marks positionally, and every new mark churns thirty call sites | [#191](https://github.com/breferrari/vigia/issues/191) |
| ✅ | The churn band's heights are ungated, so three mutations to its stacking survive | [#225](https://github.com/breferrari/vigia/issues/225) |
| ⬜ | WSL inside Windows Terminal gets no row wash, because `WT_SESSION` is read only on Windows | [#226](https://github.com/breferrari/vigia/issues/226) |
| ✅ | `gh pr checks --watch` reports green while the matrix has not started | [#236](https://github.com/breferrari/vigia/issues/236) |
| ⬜ | The caret, the pulse and the elision markers are drawn outside CP437 with no rung | [#237](https://github.com/breferrari/vigia/issues/237) |
| ⬜ | The CPU attribution clock under-reports on a loaded Windows runner | [#246](https://github.com/breferrari/vigia/issues/246) |
| ⬜ | A heat strip finer than its file draws a solid change as dashes | [#230](https://github.com/breferrari/vigia/issues/230) |
| ⬜ | A churn sample buys the file size the status walk already paid for | [#233](https://github.com/breferrari/vigia/issues/233) |
| ⬜ | The churn band measures how many files were written, not how much changed | [#232](https://github.com/breferrari/vigia/issues/232) |
| ✅ | A screenful of one-line-paragraph prose costs 117ms with every pattern already compiled | [#261](https://github.com/breferrari/vigia/issues/261) |
| ⬜ | `watch.rs` takes budget slack and CI never gives it any | [#263](https://github.com/breferrari/vigia/issues/263) |
| ⬜ | A fenced code block costs 32 to 60ms of parse, and #261's guard does not reach it | [#264](https://github.com/breferrari/vigia/issues/264) |
| ⬜ | The residual after #261 is a diffuse Markdown parse cost, largest single term an email auto-link at 94us a call | [#265](https://github.com/breferrari/vigia/issues/265) |
| ✅ | ci complete fails on every draft PR, because its legs skip and it treats skipped as failure | [#267](https://github.com/breferrari/vigia/issues/267) |
| ⬜ | A two-face bump with no xtask rerun leaves the committed dump stale and every gate green | [#268](https://github.com/breferrari/vigia/issues/268) |
| ✅ | holds_p99_rounds excused a uniform breach as a host stall | [#269](https://github.com/breferrari/vigia/issues/269) |
| ⬜ | The host-versus-work attribution needs a resolution floor, because the CPU clock is coarser than the budget it defends | [#270](https://github.com/breferrari/vigia/issues/270) |
| ⬜ | A commit can describe a gate it deleted, and the suite stays green because a missing gate is what no gate can see | [#289](https://github.com/breferrari/vigia/issues/289) |
| ✅ | `take-next` step 8 names a recurrence and prescribes a workaround, with no point at which the recurrence becomes a bug | [#290](https://github.com/breferrari/vigia/issues/290) |
| ✅ | Nothing shows staged files, so an agent that stages its own work empties the pane | [#313](https://github.com/breferrari/vigia/issues/313) |
| ⬜ | The mutation harness is re-improvised every pass, and the same footgun has fired in four of them | [#299](https://github.com/breferrari/vigia/issues/299) |
| ⬜ | A PR reached ready, mergeable and never checked, because the push and the ready raced | [#301](https://github.com/breferrari/vigia/issues/301) |
| A draft PR shows a red `ci complete`, and a draft-era run can cancel the real one | #267, 2026-08-23 | Shelf, taken | Instrument work, and the same `cancel-in-progress` grouping #301 records from the other direction. Closed by [#274](https://github.com/breferrari/vigia/issues/274). |
| ⬜ | take-next says a draft shows no checks, and this repo's draft shows a red one | [#293](https://github.com/breferrari/vigia/issues/293) |
| ✅ | The pre-flight reads a truncated board and calls it drift | [#369](https://github.com/breferrari/vigia/issues/369) |
| ⬜ | The pre-flight's cheapest-looking loop is not its slow one | [#371](https://github.com/breferrari/vigia/issues/371) |

**[#178](https://github.com/breferrari/vigia/issues/178) is an instrument finding and goes here rather than into a phase**, which is this file's own rule about a queue that serves the product and the mirror equally. Found while merging [#166](https://github.com/breferrari/vigia/issues/166): the absolute frame budgets fail on shared CI runners often enough to be a pattern, and each failure reads as a regression. `main` at `34f74ec` reported p99 98.96ms against the 48ms budget with **p50 9.25ms and max 255.40ms**; [#176](https://github.com/breferrari/vigia/pull/176) reported p99 73.14ms with **p50 3.67ms and max 179.78ms**, on a different test and a different platform, and passed on a re-run of the identical commit. The shape is the finding: a regression moves the median, and a runner losing the CPU for a quantum moves two samples of 250. Deferred rather than fixed because the product pass it interrupted was not blocked by it (one re-run cleared it) and because the fix is a ruling about where budgets are measured rather than a patch. What makes it worth filing at all: [#142](https://github.com/breferrari/vigia/pull/142) already recorded one of these and reported it honestly, which is the right handling and also the warning, because the third time nobody reads the numbers. **Closed 2026-08-17 by [#212](https://github.com/breferrari/vigia/issues/212)**, which is the entry above rather than a repeat of it: a breach is attributed with thread CPU time instead of being re-measured and believed, so the gate can say whether the time went into work or into waiting for a CPU. The row sat ⬜ for the day between the fix landing and this being noticed, and what found it was `preflight.sh` comparison 3 rather than anybody reading the file, which is the whole reason that check is mechanical.

**[#188](https://github.com/breferrari/vigia/issues/188) is here because it needs a person at a terminal, and it is the largest unverified thing this repository currently believes about its own mouse.** Found while reversing [#123](https://github.com/breferrari/vigia/issues/123), by reading tmux's source to check a claim that ruling was about to make about `focus-events`. The reading is that `server_client_reset_state` takes its mode from the **active** pane and unions `MODE_MOUSE_ALL` across panes only inside a guard on the `mouse` option, which itself defaults to off. If that is right, then under a default tmux a non-active pane never has `?1003h` requested for it, and `vigia` beside the agent the reader is typing in receives no wheel, no click and no drag at all, where §11.1 describes all three unqualified. **Deferred rather than folded into #123 for the reason that ruling exists**: asserting an unverified mechanism about shipped behaviour inside a ruling written to correct exactly that error would have been the error again, one level down. It is a Shelf entry and not a Phase 8 row because it is not a look-and-feel question and because no product pass is blocked by it: hover degrades to its residual rung either way. What it costs to settle is one `tmux new-session` at the default and once with `set -g mouse on`, which is a minute for somebody with a terminal and unreachable from here.

**[#191](https://github.com/breferrari/vigia/issues/191) is here because two review agents reached the same verdict independently: real churn, not a real hazard.** `App::chrome` takes four pointer marks positionally, all of which belong to `Shell` and none of which `App` reads, and [#186](https://github.com/breferrari/vigia/issues/186) threaded a fourth through about thirty call sites in thirteen files. What keeps it off a phase is that transposition is **compiler-caught**, since the four are distinct types, so the cost is legibility and churn rather than a defect waiting to happen. What keeps it out of #186 is that folding it in would re-churn those same thirty sites during an audit, which is where a pass manufactures the findings it then discovers. It is an instrument finding by this file's own rule, so it waits for a product pass to be blocked by it.

**Two of the five findings closed on [#66](https://github.com/breferrari/vigia/issues/66)'s own branch rather than waiting for a phase**, which is the rule about fixing what the work surfaces rather than filing it away. [#79](https://github.com/breferrari/vigia/issues/79) is the pointer and the thumb, both of which are affordances that branch published; leaving a drawn thumb inert for a phase would have shipped the same aspirational-UX defect [#66](https://github.com/breferrari/vigia/issues/66) exists to remove. [#81](https://github.com/breferrari/vigia/issues/81) was filed undiagnosed and asked for a gate rather than a fix, so it cost one test: it is **green**, the wash stops where `with_bar` narrows the rect, and the mark in the report was the host terminal's own scrollbar.

The other three stay. [#77](https://github.com/breferrari/vigia/issues/77) and [#78](https://github.com/breferrari/vigia/issues/78) are claims about the picture and belong with their siblings in Phase 4, and [#80](https://github.com/breferrari/vigia/issues/80) is a ruling nobody has made rather than a defect anybody can fix. **That last clause expired on 2026-08-15 and #80 is in Phase 8 now**, which is [#76](https://github.com/breferrari/vigia/issues/76)'s rule arriving in the only way it can until something automates it: a reader looked at the footer and asked why the new keys were not on it. #80 was written about the **arrows** and about `jk`/`JK` spending eleven columns on a case distinction, and both of those really are rulings, because an arrow is the key everybody guesses. Phase 8 then added five gestures and not one of them is an arrow: `d` and `u` ([#121](https://github.com/breferrari/vigia/issues/121)), `n`, `p` and the digits ([#122](https://github.com/breferrari/vigia/issues/122)). Those are the opposite of an arrow, they are exactly the class §11.1's own argument says earns a slot, and #122's whole case for the digits was **reach**. A reach key nobody can discover has not shipped the reach it was built for, so the defect half exists now and the shelf entry's own sentence is what says it should leave.

---

## Deferral shelf

Items that surfaced mid-phase and would have derailed the block they surfaced in. Deferral is a first-class outcome recorded here, not a dropped ball and not scope creep absorbed silently. Each one carries the phase it moved to.

| Item | Surfaced | Moved to | Why |
|---|---|---|---|
| The sheet has no roomy rung: no air, no sections, and a reorder that would invert the keep-set ([#285](https://github.com/breferrari/vigia/issues/285)) | #220, 2026-08-24 | Phase 8 | #220 carried three rungs under one title, which is #125's shape. Split before a plan was written. This rung needs a display reorder the height ladder's keep-set is load-bearing on, so it cannot ride along with the width rung. |
| At the residual floor the sheet still drops gestures in silence, and reaching them is an input-model ruling ([#286](https://github.com/breferrari/vigia/issues/286)) | #220, 2026-08-24 | Phase 8 | The half no column trading can reach, on a pane short and narrow at once. Reaching it is an input-model ruling against B12's *it is not a mode*, and it is blocked by #220 and #285 because both move the heights at which dropping starts. |
| The sheet omits a gesture the README teaches, and the gate for exactly that is a hand-written list ([#288](https://github.com/breferrari/vigia/issues/288)) | #220, 2026-08-24 | Phase 8 | Pre-existing, found by #220's docs audit. Adding a row to `MOUSE` re-measures the two-column rung that #220 just pinned in `SPEC.md` §11.1, so it is a re-measurement rather than a line. |
| A commit can describe a gate it deleted, and the suite stays green because a missing gate is what no gate can see ([#289](https://github.com/breferrari/vigia/issues/289)) | #220, 2026-08-24 | Shelf | Instrument work, so the Shelf rather than a phase: #220 was not blocked by it. Two probe removals two days apart each took real gates with them and both commits described the gates they had deleted. |
| A PR reached ready, mergeable and never checked, because the push and the ready raced ([#301](https://github.com/breferrari/vigia/issues/301)) | #295, 2026-08-24 | Shelf | Instrument work, so the Shelf: #295 was delayed rather than blocked. `ci.yml` lists `ready_for_review` precisely so a PR cannot reach mergeable unchecked, and its own comment says so. |
| The mutation harness is re-improvised every pass, and the same footgun has fired in four of them ([#299](https://github.com/breferrari/vigia/issues/299)) | #286, 2026-08-24 | Shelf | Instrument work, so the Shelf rather than a phase: #286 was not blocked by it, only slowed. `git checkout -- <file>` reverting a mutation on a dirty tree destroyed uncommitted work three times in #286's pass, and the vault records the same trap on 2026-07-30 (which already called it a repeat) and twice more on 2026-08-22. |
| `take-next` step 8 names a recurrence and prescribes a workaround, with no point at which the recurrence becomes a bug ([#290](https://github.com/breferrari/vigia/issues/290)) | #220, 2026-08-24 | Shelf, fixed same day | Instrument work, and #220 was not blocked by it. Surfaced by the vault session auditing #220's own work record: step 8 states the write guard has refused nine calls running and offers hand-filing, and names no point at which that becomes a defect. |
| `watch.rs` takes budget slack and CI never gives it any ([#263](https://github.com/breferrari/vigia/issues/263)) | #129, 2026-08-21 | Shelf | Surfaced by #262's first CI run, which failed on macOS at 516.53ms against 500ms and passed on a re-run. The message is the finding rather than the timing: it printed `MAX_DELAY_BOUND` untouched, so the slack in force was 1.0. |
| A screenful of one-line-paragraph prose costs 117ms with every pattern already compiled ([#261](https://github.com/breferrari/vigia/issues/261)) | #129, 2026-08-21 | Shelf | Turned up while measuring #129 and is the opposite shape to it: #129's cliff is flat in content size, this one tracks what is on screen and survives every warm. **Taken and closed 2026-08-22, and the cause recorded here when it was shelved was wrong in every part** — the real cause was Markdown's block-start lookahead exploring the inline-content alternation exponentially. Kept rather than deleted because a shelf entry recording a falsified cause is what a later reader plans against. |
| A fenced code block costs 32 to 60ms of parse, and #261's guard does not reach it ([#264](https://github.com/breferrari/vigia/issues/264)) | #261, 2026-08-22 | Shelf | Split out of #261, which closed the prose and table case entirely in this repository and could not close this one. |
| The residual after #261 is a diffuse Markdown parse cost, largest single term an email auto-link at 94us a call ([#265](https://github.com/breferrari/vigia/issues/265)) | #261, 2026-08-22 | Shelf | **Not a breach on any platform measured, and shelved for that reason rather than for cost.** After #261's guard the worst 24-line screenful of **prose** in this repository is `ROADMAP.md` at 10.03ms and `SPEC.md` at 8.81ms, both inside I9's 16ms with room, so nothing here is something a reader can feel. |
| ci complete fails on every draft PR, because its legs skip and it treats skipped as failure ([#267](https://github.com/breferrari/vigia/issues/267)) | #266, 2026-08-22 | Shelf | **An instrument finding, so it is shelved rather than filed into a phase**, which is this file's own rule about a queue that serves the product and the mirror equally. |
| A steady worktree saturates half the band, because the factor above the mean was never measured on this signal ([#281](https://github.com/breferrari/vigia/issues/281)) | #256, 2026-08-22 | Shelf | Measured while sweeping #256: on a flat fixture the band puts **40 of 76 columns at full height** at eighty columns, and a saturated column carries one bit. |
| A wider pane can take a row off the body, and the diff pays it ([#283](https://github.com/breferrari/vigia/issues/283)) | #252, 2026-08-23 | Shelf | Found by a width sweep written for the rail and **not caused by it**: it reproduces at eight columns, in `Footer::plan`, on `main`, where the rail is gated on 134. |
| The rail's arrival width is derived at the block rung, and a dense rung climbs earlier ([#284](https://github.com/breferrari/vigia/issues/284)) | #252, 2026-08-23 | Shelf | Found in #252's fourth audit round. The rail arrives at 134 because that is the one width below 328 where splitting the pane costs neither region a glance rung, and `Columns::plan` takes the glyph rung: a braille or octant cell draws two buckets per column, so the stacked ladder climbs at a 119-column pane and is already past the settled rung by 133. |
| A two-face bump with no xtask rerun leaves the committed dump stale and every gate green ([#268](https://github.com/breferrari/vigia/issues/268)) | #266, 2026-08-22 | Shelf | **An instrument finding that predates the PR that found it**, so it is shelved rather than filed into a phase. |
| holds_p99_rounds excused a uniform breach as a host stall ([#269](https://github.com/breferrari/vigia/issues/269)) | #266, 2026-08-22 | **fixed in #266** | An instrument finding that would normally shelve, **taken inside the product pass because that pass was blocked by it**: #261's new prose gate could not fail on the defect it exists to catch. |
| The host-versus-work attribution needs a resolution floor ([#270](https://github.com/breferrari/vigia/issues/270)) | #266, 2026-08-22 | Shelf | The residual of #269, split out **after the obvious fix was implemented, found broken on a shipped tier, and reverted**, which is why it is worth a row rather than a note: the next reader will reach for the same fix. |
| `take-next` step 1 cannot see a session already inside the row it hands you ([#303](https://github.com/breferrari/vigia/issues/303)) | #298, 2026-08-25 | Shelf | Instrument work, so the Shelf rather than a phase: this pass was not blocked by it, it took the next unstarted row and continued. |
| Every sheet guard reads a `Regions` snapshot that is one batch stale ([#307](https://github.com/breferrari/vigia/issues/307)) | #298, 2026-08-25 | Shelf | Found by #298's adversarial round and **older and wider than that row**, so it is shelved rather than folded in: it defeats `action_for`'s sheet guard exactly as much as the two producers #298 added, and it predates both. |
| The pre-flight's cheapest-looking loop is not its slow one ([#371](https://github.com/breferrari/vigia/issues/371)) | #369, 2026-08-29 | Shelf | Instrument work, nothing blocked. #369's round predicted the wrong loop; the attribution is in the issue. The loop whose ceiling #369 raised is 2.57s of 19.4s; the one no fetch bounds is 13.34s. |
| The sheet's tables are audited, not derived, so the keymap can still drift into them ([#312](https://github.com/breferrari/vigia/issues/312)) | #288, 2026-08-25 | Shelf | Found by #288's `/simplify` round and deeper than that row's own acceptance, which is gate-side by its own words. |
| WSL inside Windows Terminal gets no row wash ([#226](https://github.com/breferrari/vigia/issues/226)) | #159, 2026-08-18 | Shelf | Found by mutation testing during #159's audit, on the colour ladder rather than the glyph one: `Depth::from_env` reads `WT_SESSION` only when the `windows` flag is set, and Windows Terminal exports that variable into WSL where the binary is a Linux one. |
| The churn band's heights are ungated ([#225](https://github.com/breferrari/vigia/issues/225)) | #159, 2026-08-18 | Shelf | Found by mutation testing during #159's audit, on code that predates it: three mutations to `band_cell` and the arithmetic feeding it survive the whole suite, because every band gate checks presence, row count and yielding, and none reads a drawn column's glyph. |
| A heat strip finer than its file draws a solid change as dashes ([#230](https://github.com/breferrari/vigia/issues/230)) | #161, 2026-08-18 | Shelf | Found while raising the strip's source resolution, on code that predates it: `bucket_of` places a line at `(line - 1) * HEAT_BUCKETS / lines`, so when a file has fewer lines than the strip has slices, consecutive lines land on non-adjacent slices and a change that is solid in the file draws as dashes. |
| Support every modern language ([#235](https://github.com/breferrari/vigia/issues/235)) | #161, 2026-08-18 | Shelf | Reported from a live pane while this pass was in flight, and confirmed by probing `syntect` directly rather than by reading the code: `load_defaults_newlines` carries 75 syntaxes and none of them is Swift, so `syntax_for` returns `None` and the file draws as plain text, which is its documented ordinary case. |
| The caret row's weight is the one modifier a theme file cannot reach ([#195](https://github.com/breferrari/vigia/issues/195)) | #193, 2026-08-16 | Shelf | Raised by the altitude review on [#193](https://github.com/breferrari/vigia/issues/193), which found the stated reason for the constant false: it argued that the theme owns colour and the shell owns structure, citing `CARET` and `RULE`, and those are **glyphs**, a vocabulary the theme grammar does not have at all. |
| decision: a bonus hint rung is never worth a row, and nobody ruled whether it is worth a readout ([#147](https://github.com/breferrari/vigia/issues/147)) | #121, 2026-08-14 | Shelf | §11.1's drop order puts advice above instrumentation at every width, and `HINT_BASELINE` later carved out a category that ruling predates: a rung nobody is *owed*. |
| The workflow gates read text, and text has more spellings than the mechanism ([#145](https://github.com/breferrari/vigia/issues/145)) | #143, 2026-08-11 | Shelf | Five evasions of `package.rs`'s workflow gates, each demonstrated by a mutation that was YAML-parsed first to confirm it landed: a reindented job defeats the fixed-column key scan, an `echo` carrying a step's name moves `step_block`'s anchor, an `env:` value satisfies `assert_precedes`, and the version arithmetic and `concurrency` block have no gate at all. |
| `take-next` reads Copilot's line comments with the wrong login ([#132](https://github.com/breferrari/vigia/issues/132)) | #24, 2026-08-08 | Shelf | Step 7's `/comments` query filters on `copilot-pull-request-reviewer[bot]`, which is the login `/reviews` returns; `/comments` returns `Copilot`, so the documented command prints nothing however many comments exist. |
| A denied rustdoc lint that no job runs ([#131](https://github.com/breferrari/vigia/issues/131)) | #24, 2026-08-08 | Shelf | `lib.rs` denies `rustdoc::broken_intra_doc_links` and nothing runs `cargo doc`, so the lint has never been evaluated: two links are broken on `main` today and `--document-private-items` exits 101 rather than documenting. |
| `MIN_TICKS` restates `MIN_FRAMES`, and the queue it looks like it guards is unbounded ([#114](https://github.com/breferrari/vigia/issues/114)) | #112, 2026-08-04 | Phase 5 | Found by the adversarial agent auditing [#113](https://github.com/breferrari/vigia/pull/113) and confirmed at two scales: `frames` and `ticks` are the same number by construction, since `frames += 1` is only reachable through the arm that already incremented `ticks`, so `gate` states one floor twice. |
| A `core.autocrlf` or `.git/info/attributes` change is invisible to the cache guard ([#111](https://github.com/breferrari/vigia/issues/111)) | #101, 2026-08-04 | Phase 5 | Found by the adversarial agent auditing [#110](https://github.com/breferrari/vigia/pull/110), which demonstrated the file-shaped case: a carried span reported **80 rows where a cold frame computes 8** and never recovered. |
| The take-order rests on milestone titles rather than this file ([#108](https://github.com/breferrari/vigia/issues/108)) | #83, 2026-08-04 | Phase 5 | Raised by the altitude review on [#107](https://github.com/breferrari/vigia/pull/107) and demonstrated: this file's `## Phase <n>` section order is `1 2 3 4 6 7 5`, which already carries the take-order **and** shelf-last, in a version-controlled artifact a reviewer can diff. |
| The character walk is bounded per span rather than per row ([#106](https://github.com/breferrari/vigia/issues/106)) | #51, 2026-08-03 | Phase 5 | Found by the adversarial agent auditing [#105](https://github.com/breferrari/vigia/pull/105) and demonstrated rather than argued: `CHARS_PER_COLUMN` is spent per *run* rather than per row, so a row carrying six spans walks six times the budget its own doc claims. |
| The thesis workload, measured from real use ([#72](https://github.com/breferrari/vigia/issues/72)) | Phase 6, 2026-08-07 | Shelf | Not deferred for lacking value — deferred because **a pass cannot produce a week**. The evidence it needs accrues from the pane being open beside real sessions, which is happening daily, and each real-use finding files as its own issue the way [#129](https://github.com/breferrari/vigia/issues/129) already did. |
| Default view: unstaged only, or working-tree-vs-HEAD ([#50](https://github.com/breferrari/vigia/issues/50)) | Phase 6, 2026-08-07 | Shelf | §10's own words: "confirm against a week of real use" — the same calendar dependency as #72, shelved for the same reason, and the first outside-reader datum (committed files vanishing read as *missing*) is already on #72's thread. |
| Multi-worktree view: several agent sessions at once | Market pass, 2026-07-30 | Phase 5 | The strongest differentiator after glanceability, and the most monitor-shaped. Needs the single-worktree frame path to be cheap first, or it multiplies a cost we have not paid down |
| Jujutsu and Sapling support | Market pass, 2026-07-30 | Phase 5 | Git is the thesis. A second VCS before the first one is beautiful is scope, not reach |
| A truncated `.git/index` aborts instead of reporting ([#13](https://github.com/breferrari/vigia/issues/13)) | I2a, 2026-07-30 | Phase 2, with I8 | A `gix` defect, not a frame-path one: an index shorter than the object hash underflows a slice and panics, and `panic = "abort"` makes it uncatchable. |
| A symlink diffs as its target's contents ([#15](https://github.com/breferrari/vigia/issues/15)) | I2a, 2026-07-30 | Phase 5 — **reason expired, taken 2026-08-05** | Pre-existing in `Worktree::diff`, which reads through the link where git stores the target *path*. Demonstrated against git as the oracle. |
| The fingerprint cannot see a timestamp-preserving write ([#16](https://github.com/breferrari/vigia/issues/16)) | I2a, 2026-07-30 | Phase 5 | `cp -p`, `rsync -t` and `touch -r` keep the length and put the modification time back, and no margin can catch that. |
| Two paths differing outside UTF-8 collapse onto one cache key ([#17](https://github.com/breferrari/vigia/issues/17)) | I2a, 2026-07-30 | Phase 5 | `to_str_lossy` makes `FileChange::path` both the filesystem identity and the display string, and those are different jobs. |
| A frame reads a whole file to discover it is binary ([#18](https://github.com/breferrari/vigia/issues/18)) | I2a, 2026-07-30 | Phase 5 | 64 MiB read and 16.24ms for a file the first 8000 bytes already condemn, with no size cap on either side. Pre-existing in `Worktree::diff`. |
| An idle frame is one `stat` per changed file ([#19](https://github.com/breferrari/vigia/issues/19)) | I2a, 2026-07-30 | Phase 5 — **reason expired 2026-07-31, see below** | 36.71ms at 2000 changed files against a 16ms budget, almost all of it syscalls. The fix is to revalidate what is drawn rather than everything, which I4 already licenses and which needs a UI that knows what is visible. |
| An external kill leaves the terminal in raw mode ([#24](https://github.com/breferrari/vigia/issues/24)) | I8, 2026-07-30 | Phase 5 → Phase 7, 2026-08-05 (see the pull-forward log) — **taken 2026-08-08** | I8 promised "including `SIGINT`" and the shell falsified the premise: raw mode clears `ISIG` and `ENABLE_PROCESSED_INPUT`, so the interrupt key is a key event and never a signal. |
| The bulk-rewrite I9 gate is flaky on macOS hosted runners ([#36](https://github.com/breferrari/vigia/issues/36)) | I3, 2026-07-31 | Phase 5 | Failed once at 79.22ms p99 against a 48ms budget and passed on a re-run of the same commit, in a PR that changes no file under `crates/*/src`. |
| `take-next`'s pre-flight cannot see an untracked spec prerequisite ([#34](https://github.com/breferrari/vigia/issues/34)) | #32, 2026-07-31 | Phase 5 | The pre-flight's four comparisons are all keyed on `I<n>` tokens and issue metadata, so a prerequisite stated in `SPEC.md` prose with no issue behind it is invisible to every one of them. |
| The heat projection's cost follows the file ([#55](https://github.com/breferrari/vigia/issues/55)) | #41's pre-flight, 2026-07-31 | Phase 5 | Not deferred by a session that wanted to avoid it: **nothing had ever taken it**, because it is an open `SPEC.md` §10 bullet that no issue named, which is the exact hole [#34](https://github.com/breferrari/vigia/issues/34) added the fifth comparison for. |
| A file the attributes declare binary is diffed as text ([#68](https://github.com/breferrari/vigia/issues/68)) | #65, 2026-08-01 | Phase 5 | Out of scope for #65 because it is a **different attribute doing a different thing**: that issue normalises bytes, this one suppresses a diff. |
| An LFS-tracked text file diffs its pointer against its content ([#69](https://github.com/breferrari/vigia/issues/69)) | #65, 2026-08-01 | Phase 5 | **Not a defect discovered but a consequence recorded**, and not a regression: it is what happened before any filter ran. |
| I7 is measured without the highlighter ([#51](https://github.com/breferrari/vigia/issues/51)) | #45, 2026-07-31 | Phase 5 | The same blind spot as #45's, one invariant over: I7's 20.37ms comes from `crates/vigia-core/examples/timings.rs`, which is core-only and builds no `Highlighter`, while the shipped first paint parses whatever the first screenful shows. |
| The settle margin's cost is unbudgeted ([#73](https://github.com/breferrari/vigia/issues/73)) | Craft review, 2026-07-31 | Phase 5 | Not a rediscovery of [#32](https://github.com/breferrari/vigia/issues/32), which settled the margin's **soundness** and added the gate that drives frames without ever letting the fixture settle. |
| The worktree name skips the control-character transformation ([#89](https://github.com/breferrari/vigia/issues/89)) | Audit of [#67](https://github.com/breferrari/vigia/issues/67), 2026-08-02 | Phase 5 | Found by mutation sweep rather than by use, and the framing took a round to get right: it reads as `put_marked` measuring with `width_of` where `ratatui` will drop control graphemes, so a name is marked as cut when only invisible characters did not fit. |
| `render` promises any area is legal and panics on a tall area ([#91](https://github.com/breferrari/vigia/issues/91)) | Audit of [#77](https://github.com/breferrari/vigia/issues/77), 2026-08-03 | Phase 5 | Found by an adversarial pass on [#77](https://github.com/breferrari/vigia/issues/77), which introduced an **x-axis** instance of the same defect and fixed that one: the footer's recolouring pass walks columns by index where every other writer clips. |
| A repeated `base` reports itself with eighteen spaces ([#88](https://github.com/breferrari/vigia/issues/88)) | Audit of [#67](https://github.com/breferrari/vigia/issues/67), 2026-08-02 | Phase 5 | Found by an audit agent reading `theme.rs` for a doc-comment correction rather than by using the tool. A two-line string literal lost its continuation backslash, so eighteen columns of source indentation reach a reader's terminal inside an error message, and it has shipped since [#61](https://github.com/breferrari/vigia/pull/61). |
| The row's two fixed runs allocate a byte each ([#171](https://github.com/breferrari/vigia/issues/171)) | `/simplify` of [#164](https://github.com/breferrari/vigia/issues/164), 2026-08-15 | Shelf | An **optimisation rather than a defect**, which is why it is here and not in the pass that found it. `Painter::line_row` turns two `&'static str`s into `String`s per content row per frame, the sigil and its gap, and neither can be elided because both escape into the run vector. |
| Five findings from using the pinned list ([#77](https://github.com/breferrari/vigia/issues/77), [#78](https://github.com/breferrari/vigia/issues/78), [#79](https://github.com/breferrari/vigia/issues/79), [#80](https://github.com/breferrari/vigia/issues/80), [#81](https://github.com/breferrari/vigia/issues/81)) | #66, 2026-08-02 | Phase 3 and Phase 8 | Reported by running the branch rather than by reading it, which is the fourth time that has been the finding method here after [#30](https://github.com/breferrari/vigia/issues/30), [#45](https://github.com/breferrari/vigia/issues/45) and [#57](https://github.com/breferrari/vigia/issues/57). |
| `take-next` picks a phase by an undefined sort ([#83](https://github.com/breferrari/vigia/issues/83)) | The Phase 4 re-housing, 2026-08-01 | Phase 5 | Step 1 sorts milestones by `due_on` and **every milestone here has none**, so the order is whatever the API returns. |
| The diff's total height can be one edit stale ([#84](https://github.com/breferrari/vigia/issues/84)) | [#66](https://github.com/breferrari/vigia/issues/66)'s branch, 2026-08-02 | Phase 5 | Found while building the row-exact bar and **attempted on the branch that found it**, which is where the deferral is owed an argument rather than a note. |
| Two findings from hardening the counting path ([#85](https://github.com/breferrari/vigia/issues/85), [#86](https://github.com/breferrari/vigia/issues/86)) | #71's core audit, 2026-08-02 | Phase 5 | Both are about the *instrument* rather than the code, which is why they are here and not fixed in place. #85's split is right and the three gates it breaks each need their claim restated rather than their number adjusted: it was attempted on the branch and reverted rather than half-landed. |
| A `main` run was cancelled at queue time ([#75](https://github.com/breferrari/vigia/issues/75)) | #65's merge, 2026-08-01 | Phase 5 | `ci.yml` sets `cancel-in-progress` false on `main` and says why in a comment — *"a commit nobody verified lands looking like it was"* — and the run for `5c8af44` was cancelled before a single job started. |
| A deferral reason is a dated claim ([#76](https://github.com/breferrari/vigia/issues/76)) | The shelf itself, 2026-08-01 | Phase 5 | Second instance, so it is a pattern rather than an incident. [#19](https://github.com/breferrari/vigia/issues/19)'s reason named a precondition Phase 2 then met, and it sat expired for a whole phase until a session happened to read the shelf — the section above this table exists because of it. |
| An agent's write is how the grammar compile arrives ([#129](https://github.com/breferrari/vigia/issues/129)) | #72, 2026-08-07 | Phase 8 | The first finding this repository has produced from a *worktree* rather than from a fixture, and it is a shape `SPEC.md` §10 already knew about arriving by a route nobody wrote down. §10 lists the ways a reader meets a cold grammar parse and every one is a key they pressed: `G`, a follow jump, scrolling up. |
| No seam between this crate and `gix` ([#74](https://github.com/breferrari/vigia/issues/74)) | Craft review, 2026-07-31 | Phase 5 | The status walk, rename tracking, the attributes stack and the filter machinery are consumed directly and widely, so §3's budgets are a **transitive** property of a pre-1.0 dependency's implementation. |
| Pointer motion draws a full frame ([#154](https://github.com/breferrari/vigia/issues/154)) | I1, 2026-08-15 | Phase 8 | Found while ruling [#123](https://github.com/breferrari/vigia/issues/123), which changes no source: this has been true since the mouse landed in Phase 2. |
| §5.1's departure count contradicts itself ([#156](https://github.com/breferrari/vigia/issues/156)) | §5.1, 2026-08-15 | Phase 8 | Found while ruling [#124](https://github.com/breferrari/vigia/issues/124), which had to say whether a speaking rule would be a *fifth* departure and could not. §5.1 numbers a first, a second, a third and a fourth, and two other sentences still say the total is **two**: the header row's "first of two deliberate departures" and §11.1's "the deliberate departures stay at two". |

### A deferral reason is a dated claim like any other

**Worked instance, and the one that made this a heading: [#19](https://github.com/breferrari/vigia/issues/19).** It went to Phase 5 because the fix *"needs a UI that knows what is visible"*, and Phase 2 shipped that UI: I4 makes the shell diff only what it draws, and [#32](https://github.com/breferrari/vigia/issues/32) measured the consequence — over the same fixture and event, the core ran **98 of 182 frames over the 16ms budget** and the shell ran **0 of 1060**. The precondition the deferral named had been met for a phase.

That does not make it urgent. Nothing ships the core alone, so the breach is on no path a user is on. Both branches are real and argue opposite ways: **for taking it**, principle 2 says the budgets are the product and this is a measured I9 breach with every new element another caller of the walk it is about; **for leaving it**, it is invisible in the shipped binary and the shelf exists precisely so mid-phase discoveries do not derail the block they surfaced in. **Recorded as an open decision rather than settled by default.** What is not acceptable is the state it was in until 2026-07-31: filed under a blocker that had already dissolved, so nobody would revisit it. Decide it when Phase 3 closes at the latest.

**So a shelf entry is re-read against its own reason, not only against its priority**, and that is the exit trigger this table needs to have one at all. It fires on the same occasion the entry trigger does: every pass that files a row here asks of one existing row whether the reason it carries is still true.

**A drift check that only ever pays on the backlog it was written against has not been shown to work.** Preflight's fifth comparison found five §10 bullets no issue named on its first run, which is what it was written for, and then found a sixth ([#55](https://github.com/breferrari/vigia/issues/55)) on its second — a bullet that had been added since. The second run is the one that established the check catches drift rather than a known gap.

## Pull-forward log

Items that moved into an *earlier* phase than planned. Recorded for the same reason as deferrals: movement should be visible. Over time the balance of this list against the shelf says whether the plan was too ambitious or too cautious.

| Item | Moved | Why |
|---|---|---|
| `notify` named as a dependency in `SPEC.md` §6 | Into Phase 1 | I1 requires filesystem events rather than a timer, so the choice could not wait for the shell. Cross-platform C-toolchain-free status verified on all three tier-1 targets at the same time |
| Terminal restoration implemented with the shell, ahead of I8 | Into [#9](https://github.com/breferrari/vigia/issues/9) | Not scope creep and not I8 done early. A shell that takes the alternate screen without giving it back is not shippable at any stage, and `panic = "abort"` means a `Drop` alone cannot, so the panic hook had to land with the code that takes the screen. What [#8](https://github.com/breferrari/vigia/issues/8) still owns is the whole of its proof, which is also the whole of the invariant |
| I2 split into I2a and I2b | During Phase 1 | The original I2 conflated incremental re-diffing with incremental re-highlighting. Different dependencies, different phases, and Phase 1 could not close while one number meant two things |
| The memory readout's Windows path ([#56](https://github.com/breferrari/vigia/issues/56)) | Into [#41](https://github.com/breferrari/vigia/issues/41), from Phase 5, twenty minutes after being shelved | Filed and closed inside one pass, which is why it is here rather than on the shelf: **the deferral reason was wrong, not merely outgrown.** #56 claimed `GetProcessMemoryInfo` needs `windows-sys` and that this would be *a new crate in the graph*, unlike `libc` on macOS. `cargo tree -i windows-sys@0.61.2 --target x86_64-pc-windows-msvc` returns a non-dev path through `gix-sec`, so the two are the same case and `SPEC.md` §6 had said so since Phase 1: the `-sys` crates `notify` pulls "are FFI declarations against facilities the OS already ships". The reusable half is not the fact but **how it was got wrong**: the macOS route was checked with `cargo tree` and the Windows route was assessed from memory of what a dependency costs, in the same hour, by the same reasoning that should have applied to both. The 42.8ms `tasklist` measurement was never the error and is still in the spec, one job over: it is why `soak.rs` samples RSS a fixed 288 times across its window instead of per frame |
| I7 measured over the shell's first paint ([#51](https://github.com/breferrari/vigia/issues/51)) | Out of the Phase 5 shelf, 2026-08-03 | Taken ahead of its shelf because it **blocked the top of Phase 6**. [#101](https://github.com/breferrari/vigia/issues/101) asks for a first frame inside budget and its own diagnosis said the cost was the diff's height walk and explicitly *not* #51. Measured before planning: a first `App::view` over two files of two hundred lines costs 92.62ms, and a grammarless extension shows no step at all, so ~92ms of #101's 93ms was #51's. The half worth keeping is not which issue was right but **how the wrong one was reached**: #101 ruled #51 out on lines highlighted, which is the correct test for a per-line cost and no test at all for a compile paid once per grammar |
| Fast scrolling drops frames ([#45](https://github.com/breferrari/vigia/issues/45)) | Into Phase 3, not Phase 5 | Every sibling finding ([#15](https://github.com/breferrari/vigia/issues/15) to [#19](https://github.com/breferrari/vigia/issues/19)) went to the shelf, so this one going the other way is the movement worth recording. Those were found by auditing and this was found by **using it**: scrolling a large diff stutters, consistently on a file mixing Japanese, emoji and Latin. I9 measured over a synthetic Rust fixture is not the claim; the claim is that this is glanceable beside an agent, and a pane that stutters under the reader's own thumb has stopped being monitor-class. One mechanism is confirmed by reading, that a drawn row costs its whole line rather than the pane's width, and it is deliberately **not** the reason it is here: the first commit on it is a measurement, because [#32](https://github.com/breferrari/vigia/issues/32) already closed with "measure before narrowing it" and both halves of that intuition were wrong |
| An external kill leaves the terminal in raw mode ([#24](https://github.com/breferrari/vigia/issues/24)) | Out of the Phase 5 shelf into Phase 7, 2026-08-05 | The shelf's own filter is "only if daily use asks for them", and distribution changes who the daily user is: a pane closing sends exactly the signal I8 cannot catch, a tool that lives in panes is killed that way routinely, and a first-run user whose terminal comes back wrecked uninstalls rather than filing. The half worth keeping is the **re-examination that moved one and left two**: [#91](https://github.com/breferrari/vigia/issues/91) and [#63](https://github.com/breferrari/vigia/issues/63) read like release blockers by title, and their own shelf entries had already established otherwise — the panic unreachable from the binary, the hole unreachable on the screen. A triage by title would have pulled three issues forward and spent a phase on two defects no user can reach; the shelf reasons are load-bearing exactly because they are re-read at moments like this, which is what [#76](https://github.com/breferrari/vigia/issues/76) keeps saying |
| `take-next`'s undefined milestone sort ([#83](https://github.com/breferrari/vigia/issues/83)) | Out of the Phase 5 shelf, 2026-08-04 | Taken ahead of its shelf on the same grounds as [#51](https://github.com/breferrari/vigia/issues/51) above, one step earlier in the pass: #51 blocked the top of Phase 6, and this blocked **reaching** Phase 6 at all. Phase 6 is next by this file's own ordering, and step 1 returned the shelf instead. The half worth keeping is not that it was wrong but **what catches a silent wrong answer, and what does not**. The [#66](https://github.com/breferrari/vigia/issues/66) session recorded step 1 returning the wrong phase as an open thread and shipped Phase 4 work regardless; the pass that fixed it was handed the shelf outright. Both were caught by a human reading roadmap prose the query cannot see, and neither by anything automatic, which is why the fix is not only a better sort: pre-flight comparison 6 now checks step 1's answer against `ROADMAP.md`'s section order, so the marker being edited off, a phase renumbered, or a milestone renamed off `Phase <n>` each become a finding instead of a silent skip. Correct by construction and correct by evidence are different claims, and §10 already draws that line for I3 |
| Support every modern language ([#235](https://github.com/breferrari/vigia/issues/235)) | Out of the Shelf, 2026-08-19 | Shelved 2026-08-18 with the rest of [#161](https://github.com/breferrari/vigia/issues/161)'s findings and pulled forward the next day, on the evidence this repository ranks above every other kind: **the primary user is daily-driving the tool and this is what he hits.** That is the [#45](https://github.com/breferrari/vigia/issues/45) grade of finding rather than the audit grade, and the shelf's own filter is *only if daily use asks for them*. Daily use asked. **Re-verified live 2026-08-19** rather than taken on the issue's word: `highlight.rs:591` still loads `SyntaxSet::load_defaults_newlines`, `syntect` is still pinned at 5.3.0, and `syntax_for` still resolves by extension and then by whole file name with no override in between. So the 75-syntax Sublime Text 3 snapshot stands: TypeScript, TOML, Kotlin, Swift, Dart and about twenty more draw as plain text, `Cargo.toml` is unhighlighted in a tool written in Rust whose own front-page picture puts `Cargo.toml` in the file list, and two extensions resolve to the **wrong** language rather than to none. **It splits in two and the split is the point.** The mis-mappings (`.h` to Objective-C, `.sass` to Ruby Haml) are confident wrong colour, need no dependency and no spec change, and ship first. The missing set is a dependency (`two-face 0.5.2+bat-0.26.1` bundles bat's grammars as `syntect` dumps) and therefore a `SPEC.md` §6 proposal *before* it is built, per `CLAUDE.md`'s rule, gated on three numbers rather than on taste: no `cc` anywhere in the graph, I7's 50ms first paint against a set load that is 318µs today, and the binary size four release archives have to carry. **Shipped 2026-08-19 as one PR rather than the predicted two**, and the `.h` row went the other way: the ruling keeps Objective-C, with `bat`'s opposite call recorded beside it in `SPEC.md` §6's table |
| The bump cannot move a protected `main` ([#143](https://github.com/breferrari/vigia/issues/143)) | Out of the Shelf, 2026-08-11 | Release infrastructure, so the Shelf is where it belongs by the rule that instrument findings wait for a product pass to be blocked by one. This is that rule firing rather than an exception to it: the release button's first real run could not move `main` at all, so nothing shippable could ship until it was fixed. Taken mid-pass, ahead of Phase 8's [#121](https://github.com/breferrari/vigia/issues/121), which is untouched and still next |
| `take-next` says a draft shows no checks ([#293](https://github.com/breferrari/vigia/issues/293)) | Shelved 2026-08-24 | Found during [#285](https://github.com/breferrari/vigia/issues/285) by the session seeing red on its own draft and stopping to diagnose it. The skill's step 7 warns that *"a draft shows no checks, and no checks is not green"*, describing an **empty** check list; on this repository a draft shows a **failing** one, because `ci.yml`'s `ci complete` job carries `if: always()` and no draft guard on purpose and its `needs` are all skipped. Both halves of the box are wrong here: the list is not empty, and the surface does not look settled, it looks broken. **It is a Shelf row rather than a phase row because no product pass is blocked by it**, which is that rule firing rather than an exception: the check is correct, and what it costs is minutes of diagnosis per session plus a slow training of readers to ignore red on the one surface where red matters. The work is to decide which of the two is wrong and change **that** one, rather than patching the prose to match today's workflow and leaving them free to drift again. The likely answer is the skill, since `ci.yml` already argues its own side in a comment; whichever way it goes they should end up naming each other, because the whole failure is a skill describing a workflow it does not read |

---

## How work is taken

One task, taken to done, before the next. See `.claude/skills/take-next/`.
