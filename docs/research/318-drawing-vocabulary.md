# What a monitor-class TUI can draw that vigia is not drawing

Research dossier for [#318](https://github.com/breferrari/vigia/issues/318). This file is the "note down everything" artifact: every technique surveyed, every measurement taken, every source read, and the pricing that turns them into rulings. Claims carry their source (a URL, a file path, or the command that measured them). Screenshots live in `assets/`.

The mandate, from the reader: lightweight and fast stays, but the ambition ceiling comes off. It is 2026 and this is a CLI; cutting-edge and cool is possible. Spec rulings are dated evidence with checkable reasons, not walls. Survey broadly and be thorough.

Status: **in progress**. Sections fill as the research runs.

## 1. Survey of the world

What the best-looking terminal tools actually do, read from their sources and screenshots.

### 1.1 OpenCode

Surveyed 2026-08-25 from shallow clones of `anomalyco/opencode` (formerly `sst/opencode`, GitHub 301s to the new org) at `ac1c048e` and `sst/opentui`.

**Stack.** OpenTUI + SolidJS on Bun with a native Zig rendering core (their in-house framework; Bubble Tea was abandoned in the 1.0 rewrite for performance). Flexbox layout, high-level renderables including a `Diff` widget, `Markdown`, `Code`, `ScrollBox`.

**Gradients.** No declarative gradient prop; everything is per-cell RGB/alpha interpolation in app code:

- The prompt's "Knight Rider" scanner: per-character colour per frame, gradient derived from **one** accent colour (head alpha 1.0, a 1.15x bloom step, then exponential alpha decay `0.65^i`); inactive cells same hue at alpha 0.2, so it is background-independent.
- A pulsing radial gradient behind the logo: three expanding cosine rings lerping panel colour toward primary per cell, **pre-rendered into ~138 cached frames** blitted with `.set()`, clamped to 30fps while animating. Checks `capabilities.rgb` and swaps full-block/half-block rendering.
- `tint(base, overlay, alpha)`: one linear per-channel lerp used for every soft colour in the UI, including diff backgrounds generated as `tint(bg, green, 0.22)` dark / `0.14` light.
- Real per-cell alpha compositing in the Zig buffer: dialogs float on a translucent black scrim (alpha 150).

**Theme system.** 52 tokens including **4 background layers** and **12 diff tokens** (per-side line bg, word-highlight, sign, line-number bg); 33 JSON themes; tokens can reference other tokens, take raw ANSI indices, or `{dark, light}` pairs. Mode comes from the terminal: OSC 11 luminance classification, DECRQM mode 2031 for live colour-scheme-change notifications, and a synthetic **"system" theme built from the terminal's own 16-colour palette + fg/bg** with a transparent true background. Everything RGBA end to end; capabilities probed (`rgb`, `sync`, `hyperlinks`, kitty keyboard, sixel...), not assumed.

**Visual signature.** Almost borderless: depth from the four stacked background layers; the accent is a single heavy left rule `┃` (an `EmptyBorder` with only the left edge), rounded borders only on errors; dialogs are borderless panels on the alpha scrim; toasts pin top-right with heavy rules in the variant colour. Spacing rhythm `padding 2/1`, `gap 1-2`. **Zero Nerd Font glyphs** (grep over the TUI source finds no PUA codepoints): iconography is plain Unicode (`✓ • ⋯`, braille spinner, half/full blocks). The logo is a half-block pixel font with a baked drop-shadow (`tint(background, fg, 0.25)` behind `▀`). Animations are gated by one toggle, fade-ins are 160ms smoothstep, and the flashy gradient runs inside one dialog, not ambiently.

**Most vigia-relevant ideas.** (1) The terminal-derived system theme (palette + OSC 10/11, diff washes tinted from the terminal's own green/red, transparent background) makes a tool look native everywhere without shipping 33 themes. (2) Live dark/light re-resolution as a runtime event, which matters for a monitor that outlives a daytime theme flip. (3) Alpha/lerp-first colour math instead of hardcoded dims. (4) The 12-token diff vocabulary. (5) Animation with a budget conscience: frame caches, fps clamp, one global toggle, mode 2026 sync detection.

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

Surveyed 2026-08-25 against shallow clones (delta, crush, lazygit, gitui, helix, yazi, eza, superfile, television, zellij, starship, lipgloss, glamour, bubbletea, posting, ratatui) plus cited PRs and docs. Organised by technique.

**Diff presentation, the section that matters most here.**

- **delta** is the canonical beautiful diff, and its formula is exact: syntax-highlighted foreground over a **low-chroma full-width background wash**, with a hotter same-hue wash for word-level emphasis. The constants (`src/color.rs:156-186`): dark minus `#3f0001`, minus-emph `#901011`, plus `#002800`, plus-emph `#006000`; light minus `#ffe0e0`/`#ffc0c0`, plus `#d0ffd0`/`#a0efa0`; each with 256-colour fallbacks (52/124/22/28). Word-level emphasis tokenises with `\w+` and only pairs lines whose edit distance is under `--max-line-distance 0.6`, which is why highlights never smear across unrelated lines. File headers underlined, hunk headers boxed, `⋮` as the line-number gutter separator.
- **crush** (Charm's AI TUI) modernises the same formula with a **two-tone gutter**: insert gutter bg `#293229`, code bg `#303a30` (dark), so the line-number column is a slightly darker shade of the same wash, structuring two columns with no border. Chroma syntax re-rendered over the wash, cached per (content + bg) hash.
- **lazygit** outsources diff beauty to delta via the pager config; **gitui** is the plain-foreground baseline that looks dated beside them.
- **helix** marks VCS changes with a one-column sub-cell ribbon: `▍` (U+258D) for add/modify, `▔` (U+2594) for deletions.

**Gradients and colour.** lipgloss v2 has first-class `Blend1D/Blend2D` interpolating in **CIELAB** (via go-colorful), which is why Charm gradients do not go muddy in the middle; crush applies them per grapheme cluster (logo, `▶▶▶▶` queue pills, a `╱`-textured field behind the wordmark). starship fakes gradients by stepping Powerline segment backgrounds. yazi ships per-icon hex foregrounds and `reversed = true` for the hovered row.

**Adaptive theming is the defining 2025-2026 trend.** delta auto-detects light/dark via OSC 10/11 (`terminal-colorsaurus`). yazi queries OSC 11 **and** `CSI ? 996 n`, subscribes to change notifications with mode 2031, falls back to Rec.709 luma > 0.6. lipgloss v2 made background query explicit (`BackgroundColor(in,out)` + `LightDark`); bubbletea v2 delivers it as a message. Everyone ships paired dark/light styles.

**Nerd Font icons.** lazygit: opt-in (`nerdFontsVersion`), ~790 lines of per-extension icons with hex colours. yazi: on by default, theme-driven, vendored from nvim-web-devicons, with conditional rules. eza: `--icons=auto` only when stdout is a tty. superfile: one `nerdfont` bool swapping the whole table for ASCII. Field consensus: **icons are opt-in or theme-driven, never required, and every serious tool has a clean glyphless mode.**

**Chrome fashion.** Rounded corners are the 2025 default (lazygit default `rounded` with focus as border *colour*; television `Rounded` with `None` per panel; superfile `╭╮╰╯`; zellij themes carry `rounded_corners`). lipgloss adds half-block borders that read as a solid slab, plus a compositing Canvas/Layer system. The counter-current is borderless: yazi's single `│` separators, crush's padding-and-wash structure, posting's translucent Textual layers (`background: $surface 50%`). Powerline `` U+E0B0 is the status-bar separator everywhere.

**Sub-cell graphics and new Unicode.** **ratatui now ships `Marker::Sextant` (2x3) and `Marker::Octant` (2x4, U+1CD00) in the canvas module** (PR ratatui#2235), joining braille and half blocks: octants are braille-resolution but densely packed, real area fills with no dot gaps. helix's eighth-block gutter above. chafa has drawn with wedges and sextants since 1.8.0 (what yazi uses for no-graphics preview). Real pixel graphics: yazi has seven adapter drivers (kitty old/new, iTerm2, sixel, ueberzug++, chafa) probed by behaviour (DA1 attr 4, kitty query id 31, XTVERSION); zellij re-renders sixel and kitty graphics inside panes.

**OSC 8 hyperlinks.** delta `--hyperlinks` wraps commits, files and line numbers (templatable to `vscode://file/{path}:{line}`); eza `--hyperlink`. In ratatui this was long impossible because the buffer diff splits per cell; **ratatui 0.30.1 added `CellDiffOption::ForcedWidth`**, the hook `tui-link` and OpenAI's Codex CLI (itself ratatui+crossterm) use to emit OSC 8 with honest diff widths.

**Motion.** crush runs a 20fps spinner with staggered character birth and prerendered static frames; Charm's `harmonica` is a spring-physics easing library; Textual animates via its CSS. Everyone gates animation behind a toggle.

**The one-paragraph synthesis.** The field's diff state of the art is delta's formula (syntax fg + low-chroma wash + hotter emph wash, light/dark by OSC query, not flag), modernised by crush's two-tone gutter. Chrome is rounded-or-borderless with focus as border colour; icons are theme-driven with a mandatory clean fallback; density comes from eighth-blocks and now octants, which ratatui already ships; OSC 8 on file names is the newest table-stakes nicety.

## 1.9 Premises settled against the code

- **Truecolour already reaches every terminal that advertises it.** `Depth::from_env` (`crates/vigia/src/colour.rs:137`) treats `COLORTERM=truecolor|24bit` as "the strongest positive signal", on top of a terminal table. The `Ansi16` default bites only where nothing says anything: ssh (forwards `TERM`, not `COLORTERM`) and multiplexers. So a truecolour-first look is not gated on a policy change; it is gated on nothing for most local terminals, and the ssh/tmux story is the part that needs a ladder, which exists.
- **The glyph ladder's "detection never returns octants" is a statement about fonts, not about terminals.** foot and ghostty rasterise octants and sextants themselves (section 3.2), so a `Glyphs` detection table keyed on terminal, exactly like the one `Depth` already uses, can return them where the terminal self-renders. The mechanism the spec said was impossible for glyphs ("no terminal reports which glyphs its font carries") was never needed for these ranges, because the font is not consulted.

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

### 3.3 The tool as it draws today, across its modes

Captured 2026-08-25 in foot on the chaotic synthetic repo (`assets/probe.py`'s sibling scripts; ~19 changed files: multi-hunk edits, block deletions, whole-file delete, renames with and without edits on top, staged+unstaged mixes, a binary, a 600-column line, untracked files, plus a burst driver writing at uneven cadence). Screenshots in `assets/`: `mode-default`, `mode-masthead`, `mode-rail`, `mode-staged`, `mode-sheet`, `mode-narrow60`, `mode-light`, `mode-ansi16`, `mode-nocolor`, `mode-braille`, and `btop-reference` from the same machine.

What the captures say about the current look, held beside the survey:

- **The diff wash is the dated surface.** An added or removed block paints one flat tinted slab across the pane, at one intensity, with no word-level emphasis and no gutter separation. Beside delta's low-chroma wash + hotter same-hue word emphasis, or crush's two-tone gutter, it reads 2019. It is also the *dominant* surface: in a real burst the pane is mostly wash.
- **The file list is already strong.** Heat strips with mixed bands, sparklines, right-anchored counters, kind sigils, the staged run with `new ← old` renames: the glance row is ahead of most of the field. What it lacks is entirely decorative: no gradient in the ramps (three flat stops), no icons, no hover affordances beyond the underline.
- **Chrome is nearly absent by design** and mostly reads well: one rule under the header, a bottom status line. The gestures sheet is the one bordered element (plain single-line box, square corners). Nothing is rounded anywhere; the field's default is rounded or deliberately borderless.
- **The ladders exist and work**: light, 256, 16-colour, `NO_COLOR`, and the glyph rungs all drew correctly in captures, which is a real asset most of the surveyed tools do not have in this form; degradation here is a mechanism, not a hope.
- **Observation, filed not concluded**: under the chaotic driver the status bar's frame cell read `60-69ms` where the quiet lab read `12-17ms` and the spec's budget gate reads 2.4ms p50 at 80x24. Different pane (220x55), different workload, and the cell may measure the whole wake rather than the paint; noted for [#72](https://github.com/breferrari/vigia/issues/72)'s workload evidence rather than treated as a regression claim here.

## 4. Opportunity map

Ranked draft, pending the terminal-support matrix (agent still out) and the checkpoint. Payoff is visual, judged against the captures; cost is engineering; every row keeps the ladder (what it becomes at 256, 16, `NO_COLOR`, ASCII is part of the row).

| # | candidate | payoff | cost | spec rulings touched |
|---|---|---|---|---|
| 1 | **Delta-formula diff washes**: low-chroma line wash, hotter same-hue word-level emphasis, two-tone gutter (crush) | highest: it is the dominant surface | medium: word-pairing needs an edit-distance bound; theme keys exist | green/red roles kept (hue unchanged); picture redrawn |
| 2 | **Terminal-adaptive theme**: OSC 10/11 query, system palette theme, live dark/light (mode 2031) | high: native look everywhere, no flag | medium: query plumbing | `VIGIA_THEME` stays as override; default changes |
| 3 | **Truecolour gradients on glance elements** (btop mechanism: precomputed ramps; per-row for multi-row, per-value for one-row) | med-high | low: theme + paint only | 3-stop rulings dated by the 256 cube; top rung gains stops, lower rungs keep today's |
| 4 | **Octant/sextant sparkline rungs by terminal table** (foot/ghostty/kitty self-render; ratatui `Marker::Octant` shipped) | medium | low: extend `Glyphs::detect`'s existing table | "detection never returns octants" corrected: it was a font fact, not a terminal fact |
| 5 | **Chrome polish**: rounded corners + border-spliced title on the sheet; Powerline caps in header/status segments (font-guaranteed 16/16) | medium | low | none |
| 6 | **OSC 8 hyperlinks on file paths** (ratatui 0.30.1 `CellDiffOption::ForcedWidth`) | medium: click a path, editor opens | medium | new capability; no ruling contradicts |
| 7 | **Nerd Font file icons in the list**, opt-in, theme-driven, ASCII fallback (lazygit/yazi shape) | medium | medium: icon table | I5 floor respected by opt-in + fallback |
| 8 | **#316 premise correction**: combining overlays render via font fallback on both terminals here; the refusal's "vanishes with nothing to fall back to" was `fc-list` quoted as rendering truth | low direct (mark is solved with ink) | ruling-only | §11.2 correction; reopens the vocabulary, not the mark |
| 9 | **Styled/coloured underlines** | unknown: no fact currently wants the channel | low | §5.3 reservation was written when plain was the only underline; a ruling should say whether the *style* axis is also reserved |
| 10 | **Motion vocabulary** (OpenCode restraint: fps clamp, one toggle, active-only) | low-med | varies | I1 untouched while animation is change-driven |

(to refine at the checkpoint)

## 4.1 Checkpoint

(to fill: what was decided with the reader)

## 4. Opportunity map

Ranked candidates: visual payoff, cost, degradation, spec rulings touched with reasons re-checked.

(to fill after sections 1-3)

## 5. Rulings

What moved into SPEC.md, what was declined and why, what was filed as build issues.

(to fill last)
