<div align="center">

<img src="assets/banner.jpg" alt="vigia: a watchtower sweeping a beam of light across changed lines of code" width="100%">

*Portuguese: a watchman, the one who keeps watch.*

`🚧 Early. v0.1.0 is the first release, and until its tag lands the install lines below are what it will ship, not what is live yet.`

</div>

---

## 🔭 What it is

You run a coding agent in one pane. You run `vigia` in the pane beside it.

It shows your working tree changing, continuously, without being touched. No branches, no commits, no stash list, no staging. The diff, and nothing else.

It is closer to `btop` than to a git client: something you glance at, read from shape and colour, then glance away from.

**It takes the mouse.** The wheel scrolls whichever half of the pane you are pointing at, both scrollbars can be grabbed and dragged, and clicking a file in the pinned list jumps the diff to it. None of that costs you a mode: nothing is ever selected, so the keys mean the same thing on every frame.

**A monitor, not a reviewer.** A reviewer is something you launch per changeset to step through, annotate, and decide on. `vigia` is already open. It should be correct when nobody has touched it for an hour, and still cheap when nobody has closed it for a week.

## 📦 Try it

```sh
cargo install vigia                        # any platform with a Rust toolchain
brew install breferrari/tap/vigia          # macOS and Linuxbrew
```

Or take a prebuilt binary, which needs no toolchain at all:

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.ps1 | iex"
```

Every [release](https://github.com/breferrari/vigia/releases) also carries the archives directly, for x86-64 Linux, Intel and Apple-silicon macOS, and x86-64 Windows. The Linux build is statically linked against musl, so it runs on any distribution without matching a system libc.

Building from source works too, and needs Rust 1.85 or newer:

```sh
cargo install --git https://github.com/breferrari/vigia vigia
```

**No C toolchain, on any of those paths.** Every dependency is pure Rust, which is a property CI asserts on each shipped target rather than a claim: a `cc`, `cmake` or `bindgen` entering the dependency graph fails the build.

Then, from the worktree you want to watch:

```sh
vigia                  # the current directory
vigia ~/code/some-repo
vigia --version        # the only option there is
```

| Keys | |
|---|---|
| `q` `Esc` `Ctrl+C` `Ctrl+D` | quit |
| `j` `k` `↑` `↓` | scroll a row |
| `Space` `PgDn` `PgUp` | page |
| `d` `u` | half a page, the `less` bindings |
| `g` `Home` / `G` `End` | first / last changed file |
| `n` / `p` | next / previous changed file |
| `1` to `6` | jump to that row of the pinned list |
| `J` `K` `Shift+↑` `Shift+↓` | scroll the pinned file list |
| `f` | follow the newest change, or stop following |
| `m` | show the worktree churn band above the list, or hide it |
| `?` | all of this, as a sheet over the pane |

| Mouse | |
|---|---|
| wheel | scroll whichever region the pointer is over |
| drag a scrollbar | move that region, and both bars are exact |
| click a scrollbar track | send that region to where you clicked |
| click `▲` or `▼` | one row, and it repeats while you hold it |
| click a listed file | jump the diff to it |
| click `✕` on the sheet | close it |

**You do not have to keep any of that.** `?` draws it as a table in the middle of the pane, `?` again takes it away, and the `✕` in its corner does the same. It costs the diff nothing: the sheet is drawn over rows that are already there, so nothing moves when it opens and nothing moves when it goes. No key changes meaning while it is up, either, so `q` still quits and `j` still scrolls the diff behind it.

The band `m` draws is the whole tree rather than one file: the last two minutes of churn projected across the pane, oldest on the left. It starts hidden, because it costs four rows of the diff and is not wanted on every pane.

The digits count the rows on screen, not the files in the repository: `3` is the third row the list is drawing, so it means a different file once you have scrolled the list with `J`. A digit naming a row that is not drawn does nothing at all, and so does `n` at the last changed file or `p` at the first.

It shows the working tree against the index, untracked files included, and it follows whatever changed last until you scroll away. With nothing to show it says so, and names the branch it found nothing on.

## ⚙️ Configure it

There are **two independent settings**, and most confusion here is the two being read as one. A **palette** is which colours `vigia` means. A **depth** is how many of them your terminal can actually show. Both have to allow a thing before it appears on screen, so a palette that asks for a row wash still draws none on a terminal detected as 16-colour.

Each has one place it belongs, and which is which follows from what the setting is *about*. A palette is a preference about you, so it lives in a file and follows you into every shell. A depth is a fact about the terminal in front of you, and one machine can have a truecolour pane, an `ssh` into something ancient and a CI job at the same time, so it stays a variable.

| | What decides it, first answer wins |
|---|---|
| **Palette** | `VIGIA_THEME` (a built-in name, or a path) → `~/.config/vigia/theme` → `ansi` |
| **Depth** | `VIGIA_COLOR` → `NO_COLOR` → `TERM=dumb` → `COLORTERM` → `TERM_PROGRAM` → `TERM` → 16 |

Nothing else is read. There is no flag for either, and no setting in one can change the other.

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
| File rows | `path` `path_live` `path_cold` `pulse` `spark` `spark_track` `kind` |
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

**`ansi` is the default and draws no row wash at any depth**, deliberately. A wash has to assume a background and that palette assumes none: every colour in it is a *name*, so it resolves to whatever your terminal scheme says and `vigia` matches the pane beside it instead of arguing with it. It is the only palette that is right on a background nothing has detected. The cost is the wash.

### Keeping your terminal's own colours and getting the wash too

You do not have to choose. `base` starts from a palette and every line after it overrides one thing, so keep `ansi` for the sixteen names your scheme already defines and add the two backgrounds it declines to guess:

```
base        = ansi
added_row   = on #1b3d29
removed_row = on #45222a
```

Those two values are what `dark` uses. Pick your own if your pane is lighter or darker: the only rule is that they stay far enough from your background to read as bands, and far enough from each other that an addition never looks like a removal.

### Depth

How many colours your terminal has is detected, and the chain is in the table above. `VIGIA_COLOR` overrides it with `never`, `16`, `256`, `truecolor` or `auto`, and `NO_COLOR` is honoured.

**The row wash needs 24-bit colour.** It is dropped at every rung below rather than approximated, because a quantised background is a solid block, and a block behind highlighted code destroys the colours on it. The 256-colour cube is the case worth naming: its two darkest levels per channel are 0 and 95, so `#1b3d29` lands on `#005f00`, and a newly added file draws as a screen of flat green rather than as a tint. Below 24-bit the diff signal is the `+` and `−` column, which is where it was before themes existed.

If your rows are unwashed and you know your terminal draws 24-bit, it is nearly always detection: `COLORTERM` is the only convention for claiming it and **nothing propagates it**. `ssh` forwards `TERM` and not `COLORTERM`, and a multiplexer replaces `TERM` with an entry of its own.

```sh
VIGIA_COLOR=truecolor        # settles it, in the pane or in your rc
```

Inside `tmux`, that is only half of it: `tmux` has to pass 24-bit through rather than round it to its own palette.

```sh
# ~/.tmux.conf
set -g  default-terminal "tmux-256color"
set -ga terminal-overrides ",*:Tc"
```

**What is early about it.** The surface is one optional path and `--version`, plus the configuration above. Nothing else is a flag, on purpose.

## 🖼️ Where it is going

Target layout. **This is a mockup, not a screenshot**, and `VIGIA_THEME=dark` is what draws it. All of it draws today: the header's `· 3 changed` beside the worktree's name, with `watching` alone at the right, the pinned list of changed files above the rule, the counters, the change sparklines, the heat bars and their three-step ramp, the caret on the file the diff is inside, the dot that pulses on what just changed, the scrollbar down the diff, the tinted rows and their left bars, and the highlighted diff under them. The status bar draws too, and the departures from the picture are deliberate and written down in `SPEC.md`: the left of the header reads the worktree's name rather than `vigia`, on the argument that a title bar spends six of forty columns telling you which program you started, and both regions draw a file the same way where the mockup splits the elements between them, because a file scrolled out of a capped list would otherwise take its counters with it.

<img src="assets/preview.svg" alt="Mockup of the vigia interface: a pinned list of changed files with sparklines and heat bars above a syntax highlighted diff, and a status bar showing frame time and memory." width="900">

The sparklines are change density over time. The bars locate change within each file. The list is pinned, so those signals stay on screen while you read the diff under them; it grows to the number of changed files, caps, and scrolls with `J` and `K` when there are more than fit. The scrollbar beside the diff is row-exact: it spans the screen's rows over the diff's total rows and sits at the rows above it. Counting every changed file's height turned out to cost 8.76ms where materialising the same diffs costs 442.71ms, so the bar says where the end is rather than approximating it. That count is incremental too: a file that has not changed since the last tick is proved unchanged by a `stat` rather than read again, which is 1.29ms against 12.90ms over a hundred files. The frame time sits in the status bar because it is a promise, not a diagnostic: it is the p99 of the last 128 frames, against the 16ms this is gated at. The memory beside it is read once a frame and costs 193ns to read.

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
| 🚪 **Clean exit** | Terminal restored on every exit it can observe: the quit key, Ctrl+C, an error, a panic, or the first kill from outside the program |

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
| ✅ | **4. The artifacts tell the truth** | README, mockup, spec and tracker agree with the code and each other |
| ✅ | **6. Measured, not assumed** | Claims that outran their evidence get the measurement that settles them |
| 🔨 | **7. Distribution** | crates.io, Homebrew tap, prebuilt binaries |
| ⬜ | **8. Look and feel** | Layout, colour, keys, chrome: the polish a first user actually sees |

Being built in the open, spec first. [`SPEC.md`](SPEC.md) is the source of truth and it is written before the code, so it is the honest place to see where this is going and to argue with it. [`ROADMAP.md`](ROADMAP.md) is the live state, issue linked: the table above is the shape, that file is what is actually done.

## 🏷️ The name

*Vigia* is Portuguese. It means a watchman, a lookout, the one who keeps watch. At sea it also means a porthole: the small round window you look through.

Both readings are the tool. It watches, and it is the window you watch through.

It is also the verb, third person. So `vigia .` reads as a sentence.

## 📄 License

MIT
