# Built with

| | |
|---|---|
| [ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm) | The TUI, and Windows plus cross-platform mouse |
| [gix](https://github.com/GitoxideLabs/gitoxide) | Pure Rust git. Diffs in process, no subprocess per change |
| [notify](https://github.com/notify-rs/notify) | Native filesystem events, which is what *no polling timer* requires |
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting, pure Rust, so no C toolchain in CI |
| [two-face](https://codeberg.org/CosmicHarper/two-face) | The 217 grammars, [bat](https://github.com/sharkdp/bat)'s curated set |
| [tachyonfx](https://github.com/ratatui/tachyonfx) | Effects over the drawn buffer, so a note or a message can be seen arriving |
| [ratatui-textarea](https://github.com/ratatui/ratatui-textarea) | The note box: its text, its caret, its undo |
| [fancy-regex](https://github.com/fancy-regex/fancy-regex) | The `hide` pattern |
| [image](https://github.com/image-rs/image) | Reads a changed image's format and size: PNG, JPEG, GIF, WebP, BMP, ICO, TIFF, QOI and TGA |
| [ratatui-image](https://github.com/ratatui/ratatui-image) | Draws a changed image's pictures, as half-blocks or a terminal image protocol |
| [resvg](https://github.com/linebender/resvg) | Draws a changed SVG's pictures |
| [svgtypes](https://github.com/linebender/svgtypes) | Reads an SVG's size for its size line |
| [hayro](https://github.com/LaurenzV/hayro) | Draws a changed PDF's pictures |
| [hayro-syntax](https://github.com/LaurenzV/hayro) | Reads a PDF's page size for its size line |
| [skrifa](https://github.com/googlefonts/fontations) | Finds an installed font that can draw an SVG's text |

The last rows draw changed images.

Three of those earn their place by what they *do not* add. `fancy-regex` is already in the graph under `syntect`, so the `hide` pattern costs the binary nothing. `tachyonfx` schedules nothing, which is what keeps *no polling timer* this program's own rule to keep rather than a dependency's. `two-face` builds the grammar dump at compile time and is absent from every shipped graph.

Everything is pure Rust on purpose: a genuinely static Linux binary needs no cross toolchain, and macOS and Windows are plain tier-1 targets.

## The numbers behind the promises

The scrollbar beside the diff is **row-exact**: it spans the screen's rows over the diff's total rows and sits at the rows above it. Counting every changed file's height turned out to cost **8.76ms** where materialising the same diffs costs **442.71ms**, so the bar says where the end is rather than approximating it. A file the agent is still writing keeps the height it had until the write settles, two seconds after the last one, and is counted once then.

That count is incremental too: a file that has not changed since the last tick is proved unchanged by a `stat` rather than read again, which is **1.29ms against 12.90ms** over a hundred files.

The frame time in the status bar is a promise rather than a diagnostic: it is the p99 of the last 128 frames, against the 16ms this is gated at. The memory beside it is read once a frame and costs about **240ns**, a syscall against a 16ms budget.
