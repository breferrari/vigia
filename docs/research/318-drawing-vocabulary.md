# What a monitor-class TUI can draw that vigia is not drawing

Research dossier for [#318](https://github.com/breferrari/vigia/issues/318). This file is the "note down everything" artifact: every technique surveyed, every measurement taken, every source read, and the pricing that turns them into rulings. Claims carry their source (a URL, a file path, or the command that measured them). Screenshots live in `assets/`.

The mandate, from the reader: lightweight and fast stays, but the ambition ceiling comes off. It is 2026 and this is a CLI; cutting-edge and cool is possible. Spec rulings are dated evidence with checkable reasons, not walls. Survey broadly and be thorough.

Status: **in progress**. Sections fill as the research runs.

## 1. Survey of the world

What the best-looking terminal tools actually do, read from their sources and screenshots.

### 1.1 OpenCode

(to fill: stack, gradients, theming, borders, spacing)

### 1.2 btop

(to fill: gradient technique, multi-row curves, theme system; installed locally, run side by side)

### 1.3 The wider field

(to fill: Charm ecosystem, lazygit, gitui, delta, yazi, eza, superfile, helix, zellij, starship, television, posting, and whatever the search surfaces as 2025-2026 state of the art)

## 2. Technique inventory

Each technique: what it is, who uses it, terminal support, what it would buy vigia, what it costs, degradation story.

(to fill: per-cell truecolour gradients, background washes, box drawing weight and rounding, half and quarter blocks, braille plotting, sextants and octants, Nerd Font icons, underline styles and colour, OSC 8 hyperlinks, synchronized output, motion while active, pixel graphics protocols)

## 3. Local lab

Measurements from this machine: foot and ghostty, JetBrainsMono Nerd Font, Wayland.

### 3.1 Font coverage

(to fill: fc-list sweeps of the candidate ranges)

### 3.2 Fallback rendering probes

(to fill: what a terminal actually draws for glyphs the configured font lacks; the question #316's ruling never asked)

### 3.3 Side-by-sides

(to fill: vigia beside btop and the best examples found)

## 4. Opportunity map

Ranked candidates: visual payoff, cost, degradation, spec rulings touched with reasons re-checked.

(to fill after sections 1-3)

## 5. Rulings

What moved into SPEC.md, what was declined and why, what was filed as build issues.

(to fill last)
