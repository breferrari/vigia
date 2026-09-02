# Theming vigia

Every colour the pane draws is a theme key. This file is the reference: how a palette is chosen, what a theme file may say, what every key colours, and what each rung of the colour ladder does to it. It is held against the code by `crates/vigia/tests/theme_docs.rs`, which fails when a key exists that this file does not document, or the reverse.

## Choosing a palette

`VIGIA_THEME` names a built-in (`ansi`, `dark`, `light`) or points at a theme file by path. A built-in name wins over a file of the same name in the working directory. When the variable is unset, `~/.config/vigia/theme` is read if it exists (resolved from `HOME`, then `USERPROFILE`); otherwise the default palette is `ansi`.

`ansi` is the default because it is the only palette that is correct on a terminal whose background nothing has detected: its sixteen names resolve to the reader's own scheme, so the pane matches the terminal beside it instead of fighting it. The cost is that `ansi` never draws row washes, at any depth, because a wash has to assume a background and that palette's contract is that it assumes none. A reader who knows their background gets the full picture by naming `dark` or `light`. ([#325](https://github.com/breferrari/vigia/issues/325) is the build that will detect the background and pick for you; until it lands, the showcase is one variable away.)

`VIGIA_COLOR` overrides the detected colour depth (`truecolor`, `256`, `ansi`, `none`), and `NO_COLOR` turns colour off entirely. Depth and palette are separate axes: the palette decides what may be drawn, the depth decides how finely it can be expressed.

## The theme file

One `key = value` per line. `#` starts a comment, either alone on a line or after a value. A `base = <built-in>` line, which must come before any key, starts the palette from that built-in instead of from the default, so a file can say only what it changes.

A value is, in order: an optional foreground colour, an optional `on <colour>` background, and any number of modifiers. Each part is optional but the value must say something.

```
added      = #3fb950
added_row  = on #0f2c1c
path_hover = underline
alert      = #f85149 on #2d1418 bold
```

A colour is `#rrggbb`, a 256-palette index `0` to `255`, one of the sixteen ANSI names (`black`, `red`, ..., `bright-white`), or `default` for the terminal's own colour. The modifiers are `bold`, `dim`, `italic`, `underline`, `reverse`.

**A value edits the key's current style rather than replacing it.** `added = bold` makes additions bold and keeps their colour; only the parts the value names are overwritten. Errors are loud by design: an unknown key, colour or modifier stops the shell with the line number, because a reader who wrote a theme and silently got the default would have no way to find out why.

## The keys

What follows groups the keys by surface. The docblocks in `crates/vigia/src/theme.rs` carry the full reasoning per key; this table carries the role.

### Chrome

| key | colours |
|---|---|
| `chrome` | the header and footer lines |
| `chrome_dim` | secondary chrome text: key hints, the follow marker, readouts, and the chrome rows' background |
| `note` | a stand-in for content there is no diff for: binary, conflict |
| `alert` | something went wrong and the reader should know |

### The file list

| key | colours |
|---|---|
| `path` | a changed file's path, at the freshest recency |
| `path_live` | a path that changed inside the glance window but not in the last tick |
| `path_cold` | a path nothing has written since watching began |
| `path_hover` | a listed path the pointer rests on; the pointer's own colour, underlined |
| `pulse` | the `●` marking a file that moved in the last tick |
| `kind` | the letter naming what happened to a file |
| `staged` | the kind letter and run label of a staged change; git's own green |

### The sparkline

| key | colours |
|---|---|
| `spark` | a sparkline bucket at the quietest of the three stops |
| `spark_warm` | a bucket at a third or more of the screen's busiest |
| `spark_hot` | a bucket at two thirds or more of it |
| `spark_track` | a bucket nothing was written in |

### The heat strip

| key | colours |
|---|---|
| `heat_track` | a slice nothing changed in |
| `heat_added` | a slice holding additions |
| `heat_added_warm` | the same, busier |
| `heat_added_hot` | the same, in the file's busiest band |
| `heat_removed` | a slice holding removals |
| `heat_removed_warm` | the same, busier |
| `heat_removed_hot` | the same, in the file's busiest band |
| `heat_mixed` | a slice holding both |
| `heat_mixed_warm` | the same, busier |
| `heat_mixed_hot` | the same, in the file's busiest band |

### The scrollbar

| key | colours |
|---|---|
| `bar` | the thumb: where in the whole a region is looking |
| `bar_active` | the same mark while the reader is holding or keying it |
| `bar_hover` | the same mark while the pointer merely rests on it |
| `bar_track` | the unfilled part, drawn rather than left blank |

### The diff

| key | colours |
|---|---|
| `hunk` | a hunk's `@@` header |
| `gutter` | line numbers |
| `added` | an added line's sigil |
| `removed` | a removed line's sigil |
| `context` | an unchanged line shown for orientation |
| `added_row` | the wash behind an added line; drawn only at truecolour |
| `removed_row` | the same, behind a removed line |
| `added_word` | the hotter wash on the bytes of an added line that actually changed, when it pairs with a removal |
| `removed_word` | the same, inside a removed line |
| `added_gutter` | the line-number cells of an added line, one tone off the wash (the two-tone gutter) |
| `removed_gutter` | the same, on a removed line |
| `added_bar` | the sigil column's cell on an added line, the wash's stand-in below truecolour |
| `removed_bar` | the same, on a removed line |
| `selection` | the wash over rows a drag has selected; it stands in for the diff wash while it is up, and on `ansi` it reverses the row rather than colouring it |

### Syntax

| key | colours |
|---|---|
| `keyword` | `fn`, `if`, `pub`, `mut` |
| `type_name` | a type's name |
| `function` | a function's name |
| `variable` | a binding, a parameter, a field |
| `constant` | a named constant, and a language literal |
| `string` | a string literal |
| `number` | a numeric literal |
| `comment` | a comment |

## How colour degrades

The depth ladder resolves every key on the way out of theme loading, so a palette never has to know the terminal:

- **truecolor**: everything as written.
- **256**: each RGB value quantised to the 6x6x6 cube or the grey ramp.
- **16 colours** (the default when nothing advertises more): backgrounds are dropped, so the row washes disappear and `added_bar` / `removed_bar` carry the row signal; foregrounds map to the sixteen names.
- **`NO_COLOR` / `none`**: no colour at all; glyphs and modifiers carry everything.

## What lands next

`SPEC.md` §11.2 B18 rules the 2026 vocabulary in, and each build adds its keys to this file as it lands: the diff washes and word emphasis with their gutter tones ([#321](https://github.com/breferrari/vigia/issues/321)), gradient stops for the glance ramps ([#322](https://github.com/breferrari/vigia/issues/322)), the segmented chrome and icon accents ([#323](https://github.com/breferrari/vigia/issues/323)), and the terminal-derived system palette ([#325](https://github.com/breferrari/vigia/issues/325)). A build that introduces a colour this file cannot name fails its own gate.
