<div align="center">

```
   ██╗   ██╗██╗ ██████╗ ██╗ █████╗
   ██║   ██║██║██╔════╝ ██║██╔══██╗
   ██║   ██║██║██║  ███╗██║███████║
   ╚██╗ ██╔╝██║██║   ██║██║██╔══██║
    ╚████╔╝ ██║╚██████╔╝██║██║  ██║
     ╚═══╝  ╚═╝ ╚═════╝ ╚═╝╚═╝  ╚═╝
```

**A live diff monitor for your terminal.** 🪟

*Portuguese: a watchman. Also, at sea, a porthole.*

`🚧 Early development. No release yet, nothing to install.`

</div>

---

## 🪟 What it is

You run a coding agent in one pane. You run `vigia` in the pane beside it.

It shows your working tree changing, continuously, without being touched. No
branches, no commits, no stash list, no staging. The diff, and nothing else.

It is closer to `btop` than to a git client: something you glance at, read from
shape and colour, then glance away from.

**A monitor, not a reviewer.** A reviewer is something you launch per changeset
to step through, annotate, and decide on. `vigia` is already open. It should be
correct when nobody has touched it for an hour, and still cheap when nobody has
closed it for a week.

## 🖼️ Where it is going

Target layout. **This is a sketch, not a screenshot.** Nothing renders yet.

```
┌─ vigia ──────────────────────────────────── watching · 3 files ─┐
│                                                                 │
│   src/engine/watch.rs     ▁▂▅▇█▅▂▁     +42 −7    ████████░░░░   │
│   src/render/frame.rs     ▁▁▂▃▂▁▁▁     +11 −3    ███░░░░░░░░░   │
│   Cargo.toml              ▁▁▁▁▁▂▁▁      +2 −0    █░░░░░░░░░░░   │
│                                                                 │
├─ src/engine/watch.rs ─────────────────────────── ● just changed ┤
│                                                                 │
│    38    fn coalesce(&mut self, ev: Event) -> Option<Frame> {   │
│    39  +     if self.pending.is_empty() {                       │
│    40  +         self.deadline = Instant::now() + DEBOUNCE;     │
│    41  +     }                                                  │
│    42        self.pending.insert(ev.path);                      │
│    43  −     self.flush()                                       │
│    44  +     None                                               │
│    45    }                                                      │
│                                                                 │
└─ 0.8ms frame · 11MB · follow ⏵ ─────────────────────────────────┘
```

The sparklines are change density over time. The bars locate change within each
file. The frame time sits in the status bar because it is a promise, not a
diagnostic.

## ⚡ Design commitments

These are budgets, not hopes. Each one gets a test that fails when it is missed,
and a regression past any of them fails the build.

| | |
|---|---|
| 🔔 **Event driven** | Zero wakeups while idle. No filesystem event, no work. Never a polling timer |
| ♻️ **Incremental** | Re-parse cost scales with the size of your edit, not the size of the file |
| 🪶 **Flat over days** | Under 5% memory drift across 24 hours. No retained temp files |
| 🌊 **Streaming** | First paint under 100ms, even on a 100,000 line diff |
| 🚀 **Instant start** | Under 50ms to first paint |
| 🎞️ **60fps** | Frame time under 16ms at p99, while files are being written |
| 🧘 **Correct untouched** | Follows the newest change and scrolls to it with no input |
| 📐 **Narrow panes** | Legible at 40 columns, because half a laptop screen is the point |
| 🚪 **Clean exit** | Terminal restored exactly, including on Ctrl+C and on panic |

## 🧱 Built with

| | |
|---|---|
| [ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm) | The TUI. `crossterm` for cross platform mouse and Windows support |
| [gix](https://github.com/GitoxideLabs/gitoxide) | Pure Rust git. Diffs computed in process, no subprocess per change |
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting, pure Rust, so no C toolchain in CI |

Everything is pure Rust on purpose: a genuinely static Linux binary needs no
cross toolchain, and macOS and Windows are plain tier 1 targets.

## 🗺️ Status

| | Phase | |
|---|---|---|
| 🚧 | **1. Core engine** | Watch, coalesce, diff, incremental highlight. No UI |
| ⬜ | **2. Minimum monitor** | The TUI: follow mode, scroll, mouse, layout, clean exit |
| ⬜ | **3. Glanceability** | Sparklines, heat bars, live counters, theming |
| ⬜ | **4. Distribution** | crates.io, Homebrew tap, prebuilt binaries |

Being built in the open, spec first. [`SPEC.md`](SPEC.md) is the source of truth
and it is written before the code, so it is the honest place to see where this is
going and to argue with it.

## 🏷️ The name

*Vigia* is Portuguese. It means a watchman, a lookout, the one who keeps watch.
At sea it also means a porthole: the small round window you look through.

Both readings are the tool. It watches, and it is the window you watch through.

It is also the verb, third person. So `vigia .` reads as a sentence.

## 📄 License

MIT
