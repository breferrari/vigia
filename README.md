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
| `f` | follow the newest change, or stop following |
| wheel | scroll |

It shows the working tree against the index, untracked files included, and it follows whatever changed last until you scroll away. With nothing to show it says so, and names the branch it found nothing on.

**What is early about it.** There are no flags and no config file: one optional path and nothing else. Colour is the sixteen ANSI names, so the recency and heat ramps have fewer steps than the mockup below until theming lands.

## 🖼️ Where it is going

Target layout. **This is a mockup, not a screenshot.** Most of it draws today: the header's `watching · 3 files`, the file rows, the counters, the change sparklines, the heat bars, the pulse on what just changed, and the highlighted diff under them. The status bar draws too, and the two departures from the picture are deliberate: the left of the header reads the worktree's name rather than `vigia`, on the argument that a title bar spends six of forty columns telling you which program you started, and memory is quoted in `MiB` because that is the unit the soak that budgets it uses.

<img src="assets/preview.svg" alt="Mockup of the vigia interface: a file list with change sparklines above a syntax highlighted diff, and a status bar showing frame time and memory." width="900">

The sparklines are change density over time. The bars locate change within each file. The frame time sits in the status bar because it is a promise, not a diagnostic: it is the p99 of the last 128 frames, against the 16ms this is gated at. The memory beside it is read once a frame and costs 193ns to read.

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
| 🔨 | **3. Glanceability** | Sparklines, heat bars, live counters, then theming |
| ⬜ | **4. Distribution** | crates.io, Homebrew tap, prebuilt binaries |

Being built in the open, spec first. [`SPEC.md`](SPEC.md) is the source of truth and it is written before the code, so it is the honest place to see where this is going and to argue with it. [`ROADMAP.md`](ROADMAP.md) is the live state, issue linked: the table above is the shape, that file is what is actually done.

## 🏷️ The name

*Vigia* is Portuguese. It means a watchman, a lookout, the one who keeps watch. At sea it also means a porthole: the small round window you look through.

Both readings are the tool. It watches, and it is the window you watch through.

It is also the verb, third person. So `vigia .` reads as a sentence.

## 📄 License

MIT
