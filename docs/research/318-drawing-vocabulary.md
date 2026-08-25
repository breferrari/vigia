# What a monitor-class TUI can draw that vigia is not drawing

Research dossier for [#318](https://github.com/breferrari/vigia/issues/318). This file is the "note down everything" artifact: every technique surveyed, every measurement taken, every source read, and the pricing that turns them into rulings. Claims carry their source (a URL, a file path, or the command that measured them). Screenshots live in `assets/`.

The mandate, from the reader: lightweight and fast stays, but the ambition ceiling comes off. It is 2026 and this is a CLI; cutting-edge and cool is possible. Spec rulings are dated evidence with checkable reasons, not walls. Survey broadly and be thorough.

Status: **in progress**. Sections fill as the research runs.

## 1. Survey of the world

What the best-looking terminal tools actually do, read from their sources and screenshots.

### 1.1 OpenCode

(to fill: stack, gradients, theming, borders, spacing)

### 1.2 btop

Source basis: shallow clone of `aristocratos/btop` at `76e323d` (2026-08-22, post v1.4.7); local install btop 1.4.7 with 41 stock themes at `/usr/share/btop/themes/`. Paths resolve on GitHub under `aristocratos/btop/blob/main/`.

**Gradients.** `src/btop_theme.cpp`, `generateGradients()` (lines 306-363). 101 precomputed steps per gradient, one per integer percent, each stored as a fully rendered ANSI escape string so draw time is a lookup plus append, zero math. Interpolation is plain linear integer RGB per channel, no HSL, no gamma, no easing: good endpoints do the work. Gradients are 2-stop or 3-stop from theme keys `<name>_start/_mid/_end`; `_mid` splits the 101 steps at 50. Application differs by element:

- Meters: per-cell, each cell coloured by its own position, so a full meter sweeps the gradient.
- Multi-row graphs: **per-row**, one colour per text row mapped to the vertical axis (top = `_end`). One escape per row per frame; reads as "hotter at the top" while costing almost nothing.
- One-row graphs: per-cell by value.
- Process list: per-row by distance from the selection (a fade), plus per-value cells.

At 256 colours the same 101 steps are quantised through the 6x6x6 cube (`round(c/51)`); banded but still ordered.

**Graphs.** Glyph tables in `Symbols::graph_symbols` (`src/btop_draw.cpp:84-134`), three sets: `braille` (a 5x5 lookup indexed `left_level * 5 + right_level`, each level 0-4 dots per braille column, so two samples per cell and 4 x height dots of vertical resolution), `block` (quadrant/half blocks, ~2 levels per half-cell), `tty` (` ░▒█`). Value mapping per row band with a +0.1 bias so tiny values still light a dot; `no_zero` keeps an idle baseline. Scrolling is O(1) per tick: two alternating string buffers, pop one UTF-8 cell at the front, append one at the end. The whole "smoothness" is 2x4 subpixels per cell plus 100-step vertical quantisation; no curve fitting.

**Themes.** Line-oriented `theme[key]="value"`, about 50 keys: chrome, per-box border colours, and the gradient triples. Empty `main_bg` means terminal-default background. Truecolour is a config flag, not detected; `--low-color` quantises to 256, and tty mode (auto-detected via `/dev/tty` on Linux) drops to 16 hardcoded SGR colours, shade-character graphs, square corners.

**Chrome.** Rounded corners `╭╮╰╯` by default; titles spliced into the top border as `┐` + bold title + `┌` in a contrasting colour so the border visually opens around the text; superscript digits as hotkeys; per-box border hues; boxes drawn once and cached, content overprinted with cursor addressing.

**Transferable mechanisms** (all replicable in ratatui): (1) precomputed 101-colour gradients; (2) the braille 5x5 two-samples-per-cell table with anti-vanishing bias; (3) per-row gradient colouring against the vertical axis, which looks expensive and costs one style per row; (4) border-spliced titles over tinted rounded borders; (5) a three-tier ladder truecolor/braille, 256-quantised, 16-colour + shades + square corners.

### 1.3 The wider field

(to fill: Charm ecosystem, lazygit, gitui, delta, yazi, eza, superfile, helix, zellij, starship, television, posting, and whatever the search surfaces as 2025-2026 state of the art)

## 2. Technique inventory

Each technique: what it is, who uses it, terminal support, what it would buy vigia, what it costs, degradation story.

(to fill: per-cell truecolour gradients, background washes, box drawing weight and rounding, half and quarter blocks, braille plotting, sextants and octants, Nerd Font icons, underline styles and colour, OSC 8 hyperlinks, synchronized output, motion while active, pixel graphics protocols)

## 3. Local lab

Measurements from this machine: foot and ghostty, JetBrainsMono Nerd Font, Wayland.

### 3.1 Font coverage

Measured 2026-08-25 with `fc-list ':family=JetBrainsMono Nerd Font:charset=<hex>'` per codepoint (`assets/probe.py` is the companion visual probe; the sweep script iterates the range and counts hits).

| range | what | coverage |
|---|---|---|
| U+2500..257F | box drawing | 128/128 |
| U+2580..259F | block elements | 32/32 |
| U+2800.. | braille (head sampled) | 32/32 |
| U+1FB00..1FB3B | sextants | 0/60 |
| U+1FB3C..1FB6F | smooth mosaics | 0/52 |
| U+1FB70..1FB9F | more legacy blocks | 0/48 |
| U+1CD00.. | octants (head sampled) | 0/9 |
| U+20D0..20FF | combining marks for symbols | 0/48 |
| U+20DD..20E4 | enclosing marks | 0/8 |
| U+0300..036F | Latin combining | 23/112 (U+0334..0338: 3/5) |
| U+25E2..25E5 | corner triangles | 0/4 |
| U+E0B0..E0BF | Powerline separators (incl. rounded) | 16/16 |
| U+E200..E2A9 | Powerline extra + Pomicons | 170/170 |
| U+E5FA.. | Seti file icons (head sampled) | 50/50 |
| U+F0001.. | Material icons (head sampled) | 31/31 |

So the font itself already carries: complete box drawing, blocks, braille, the full Powerline separator vocabulary including rounded caps, and thousands of file-type and badge icons. It carries none of: sextants, octants, combining overlays, enclosing marks.

### 3.2 Fallback rendering probes

`assets/probe.py` rendered in foot 1.x and ghostty (the two terminals installed here), screenshots `assets/probe-foot.png` and `assets/probe-ghostty.png`, taken 2026-08-25 on Hyprland with stock JetBrainsMono Nerd Font.

**Both terminals render every section**, including everything the font has zero coverage of:

1. **Sextants, smooth mosaics, and octants all draw correctly.** These terminals rasterise the Symbols for Legacy Computing and octant ranges themselves, font-independently, the way every terminal already rasterises box drawing. The spec's "no font measured carries U+1CD00, so detection never returns octants" is a fact about fonts being quoted as if it were a fact about rendering; on foot and ghostty the font never gets asked.
2. **The combining overlay renders.** `M` + U+20D2, the exact composition #316 refused: both terminals draw the vertical stroke over the M, via font fallback (foot) or custom handling. `a` + U+20DD draws a clean enclosing circle in ghostty, a smaller but legible one in foot. `=` + U+0338 draws a correct not-equals. #316's premise, "a mark that needs a font we cannot see vanishes", is false on both terminals present; whether it holds anywhere that matters now depends on the wider matrix (survey pending), not on `fc-list`.
3. **All five underline styles are distinct** (plain, double, curly, dotted, dashed) and the separate underline colour works: `red-curly` draws a red undercurl under default-colour text in both.
4. **Truecolour gradients are perfectly smooth** at 60 steps across, foreground on blocks and background wash alike. No banding.
5. **Delta-style diff washes read exactly as intended**: whole-line dark green/red tint, brighter tint on the changed word, text legible throughout.
6. **Nerd Font icons and rounded Powerline caps are crisp** (they are real font glyphs here, per 3.1).

### 3.3 Side-by-sides

(to fill: vigia beside btop and the best examples found)

## 4. Opportunity map

Ranked candidates: visual payoff, cost, degradation, spec rulings touched with reasons re-checked.

(to fill after sections 1-3)

## 5. Rulings

What moved into SPEC.md, what was declined and why, what was filed as build issues.

(to fill last)
