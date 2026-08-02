<div align="center">

<img src="assets/banner.jpg" alt="vigia: a watchtower sweeping a beam of light across changed lines of code" width="100%">

*Portuguese: a watchman, the one who keeps watch.*

`🚧 Early development. It runs, and it installs from source. No packaged release yet.`

</div>

---

## 🔭 What it is

You run a coding agent in one pane. You run `vigia` in the pane beside it.

It shows your working tree changing, continuously, without being touched. No branches, no commits, no stash list, no staging. The diff, and nothing else.

It is closer to `btop` than to a git client: something you glance at, read from shape and colour, then glance away from.

**A monitor, not a reviewer.** A reviewer is something you launch per changeset to step through, annotate, and decide on. `vigia` is already open. It should be correct when nobody has touched it for an hour, and still cheap when nobody has closed it for a week.

## 📦 Try it

There is **no crates.io release and no prebuilt binary yet**. Both are Phase 4, and publishing a name to crates.io is permanent, so it happens when there is a version worth keeping forever. What exists today is a binary that builds from source in one command.

```sh
cargo install --git https://github.com/breferrari/vigia vigia
```

Rust 1.85 or newer is the only requirement. Every dependency is pure Rust, so there is no C toolchain and no system library to install first.

Then, from the worktree you want to watch:

```sh
vigia                  # the current directory
vigia ~/code/some-repo
```

| Keys | |
|---|---|
| `q` `Esc` `Ctrl+C` `Ctrl+D` | quit |
| `j` `k` `↑` `↓` | scroll a row |
| `Space` `PgDn` `PgUp` | page |
| `g` `Home` / `G` `End` | first / last changed file |
| `J` `K` `Shift+↑` `Shift+↓` | scroll the pinned file list |
| `f` | follow the newest change, or stop following |
| wheel | scroll |

It shows the working tree against the index, untracked files included, and it follows whatever changed last until you scroll away. With nothing to show it says so, and names the branch it found nothing on.

## ⚙️ Configure it

There is one file and one variable, and which is which follows from what the setting is *about*. A palette is a preference about you, so it lives in a file and follows you into every shell. A colour depth is a fact about the terminal in front of you, and one machine can have a truecolour pane, an `ssh` into something ancient and a CI job at the same time, so it stays a variable.

```
~/.config/vigia/theme
```

Read when it exists, on every platform, resolved from `HOME` or `USERPROFILE`. No file is the ordinary case and is not an error. A file that exists and does not parse **is**: `vigia` says which line and exits before it takes the screen, because an error painted inside a full-screen program that then hands the terminal back is an error nobody reads.

```
base       = dark
added_row  = on #0f2c1c
path       = #e6edf3 bold
comment    = 244
```

`base` names the palette to start from and everything else overrides one thing, so a three-line file is a normal size for one. A key it does not recognise is an error naming the line, never a line quietly ignored.

A value is `[colour] [on colour] [modifiers]`:

| Part | Written as |
|---|---|
| Colour | `#rrggbb`, a palette index `0` to `255`, one of the sixteen names, or `default` |
| Names | `black` `red` `green` `yellow` `blue` `magenta` `cyan` `grey` `white`, each with a `bright-` twin |
| Background | `on` followed by a colour |
| Modifiers | `bold` `dim` `italic` `underline` `reverse`, any number of them |

The keys, which are every colour the shell draws with:

| Group | Keys |
|---|---|
| Chrome | `chrome` `chrome_dim` |
| Scrollbars | `bar` `bar_track` |
| File rows | `path` `path_live` `path_cold` `pulse` `spark` `kind` |
| Heat strip | `heat_track`, and `heat_added` `heat_removed` `heat_mixed` each with a `_warm` and `_hot` twin |
| Diff | `hunk` `gutter` `added` `removed` `context` `note` `alert` |
| Row wash | `added_row` `removed_row` `added_bar` `removed_bar` |
| Syntax | `keyword` `type_name` `function` `variable` `constant` `string` `number` `comment` |

Three palettes ship, and `VIGIA_THEME` overrides the file with one of them or with a path, which is how you say "not this time" without editing anything:

```sh
VIGIA_THEME=ansi    # the default: the sixteen names, inherited from your scheme
VIGIA_THEME=dark    # the mockup below, in 24-bit colour
VIGIA_THEME=light   # the same design for a light terminal
VIGIA_THEME=~/themes/mine
```

**`ansi` is the default and draws no row wash at any depth**, deliberately. A wash has to assume a background and that palette assumes none: every colour in it is a *name*, so it resolves to whatever your terminal scheme says and `vigia` matches the pane beside it instead of arguing with it. It is the only palette that is right on a background nothing has detected. The cost is the wash, and naming a theme buys it back.

How many colours your terminal has is detected. `VIGIA_COLOR` overrides that with `never`, `16`, `256`, `truecolor` or `auto`, and `NO_COLOR` is honoured. Below 256 the row wash is dropped rather than approximated, because an ANSI background is a solid block and a block behind highlighted code destroys the colours on it.

**What is early about it.** There are no flags: one optional path, and the configuration above.

## 🖼️ Where it is going

Target layout. **This is a mockup, not a screenshot**, and `VIGIA_THEME=dark` is what draws it. All of it draws today: the header's `watching · 3 files`, the pinned list of changed files above the rule, the counters, the change sparklines, the heat bars and their three-step ramp, the caret on the file the diff is inside, the pulse on what just changed, the scrollbar down the diff, the tinted rows and their left bars, and the highlighted diff under them. The status bar draws too, and the departures from the picture are deliberate and written down in `SPEC.md`: the left of the header reads the worktree's name rather than `vigia`, on the argument that a title bar spends six of forty columns telling you which program you started, and both regions draw a file the same way where the mockup splits the elements between them, because a file scrolled out of a capped list would otherwise take its counters with it.

<img src="assets/preview.svg" alt="Mockup of the vigia interface: a pinned list of changed files with sparklines and heat bars above a syntax highlighted diff, and a status bar showing frame time and memory." width="900">

The sparklines are change density over time. The bars locate change within each file. The list is pinned, so those signals stay on screen while you read the diff under them; it grows to the number of changed files, caps, and scrolls with `J` and `K` when there are more than fit. The scrollbar beside the diff is row-exact: it spans the screen's rows over the diff's total rows and sits at the rows above it. Counting every changed file's height turned out to cost 8.76ms where materialising the same diffs costs 442.71ms, so the bar says where the end is rather than approximating it. The frame time sits in the status bar because it is a promise, not a diagnostic: it is the p99 of the last 128 frames, against the 16ms this is gated at. The memory beside it is read once a frame and costs 193ns to read.

## ⚡ Design commitments

These are budgets, not hopes. Each one gets a test that fails when it is missed, and a regression past any of them fails the build.

| | |
|---|---|
| 🔔 **Event driven** | Zero wakeups while idle. No filesystem event, no work. Never a polling timer |
| ♻️ **Incremental diff** | Re-diff cost scales with what changed, not with the size of your worktree |
| 🖍️ **Incremental highlight** | Re-parse cost scales with the size of your edit, not the size of the file |
| 🪶 **Flat over days** | Under 5% memory drift across 24 hours. No retained temp files |
| 🌊 **Streaming** | First paint under 100ms, even on a 100,000 line diff |
| 🚀 **Instant start** | Under 50ms to first paint |
| 🎞️ **60fps** | Frame time under 16ms at p99, while files are being written |
| 🧘 **Correct untouched** | Follows the newest change and scrolls to it with no input |
| 📐 **Narrow panes** | Legible at 40 columns, because half a laptop screen is the point |
| 🚪 **Clean exit** | Terminal restored on every exit it controls: the quit key, Ctrl+C, an error, or a panic |

## 🧱 Built with

| | |
|---|---|
| [ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm) | The TUI. `crossterm` for cross platform mouse and Windows support |
| [gix](https://github.com/GitoxideLabs/gitoxide) | Pure Rust git. Diffs computed in process, no subprocess per change |
| [notify](https://github.com/notify-rs/notify) | Native filesystem events per platform, which is what "no polling timer" requires |
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting, pure Rust, so no C toolchain in CI |

Everything is pure Rust on purpose: a genuinely static Linux binary needs no cross toolchain, and macOS and Windows are plain tier 1 targets.

## 🗺️ Status

| | Phase | |
|---|---|---|
| ✅ | **1. Core engine** | Watch, coalesce, diff, incremental re-diff. No UI |
| ✅ | **2. Minimum monitor** | The TUI: follow mode, scroll, mouse, layout, clean exit |
| ✅ | **3. Glanceability** | Sparklines, heat bars, live counters, the status bar, theming |
| ⬜ | **4. Distribution** | crates.io, Homebrew tap, prebuilt binaries |

Being built in the open, spec first. [`SPEC.md`](SPEC.md) is the source of truth and it is written before the code, so it is the honest place to see where this is going and to argue with it. [`ROADMAP.md`](ROADMAP.md) is the live state, issue linked: the table above is the shape, that file is what is actually done.

## 🏷️ The name

*Vigia* is Portuguese. It means a watchman, a lookout, the one who keeps watch. At sea it also means a porthole: the small round window you look through.

Both readings are the tool. It watches, and it is the window you watch through.

It is also the verb, third person. So `vigia .` reads as a sentence.

## 📄 License

MIT
