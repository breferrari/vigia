# Make it yours

Everything `vigia` reads to decide how the pane is drawn and what it opens as. Nothing here is required: the defaults are what the pane ships with, and the [README](../README.md) covers the two settings most people ever touch.

Three independent settings decide how the pane is **drawn**, and most confusion here is any two being read as one. A **palette** is which colours `vigia` means. A **depth** is how many your terminal can show. **Glyphs** is which drawing characters its font carries. All three have to allow a thing before it appears. A fourth decides what the pane **starts as**, and a fifth is not about drawing at all.

| | First answer wins |
|---|---|
| 🎨 **Palette** | `VIGIA_THEME` (a name, or a path) → `~/.config/vigia/theme` → **your terminal's own background** → `ansi` |
| 🔦 **Depth** | `VIGIA_COLOR` → `NO_COLOR` → `TERM=dumb` → `COLORTERM` → `TERM_PROGRAM` → `TERM` → 16 |
| ✏️ **Glyphs** | `VIGIA_GLYPHS` → `TERM=dumb`/`linux` → **an engine that draws octants and names its version** → `TERM_PROGRAM` → `WT_SESSION` → `TERM` → braille, or blocks on a bare Windows console |
| 🪟 **View** | `~/.config/vigia/config` → everything off, except `links` and `notes`, and nothing hidden |
| 🔔 **Updates** | one look at crates.io when the pane opens, and a footer line only if there is a newer release. `VIGIA_UPDATE=off` declines it |

```sh
VIGIA_THEME=ansi     # the fallback: the sixteen names, inherited from your scheme
VIGIA_THEME=dark     # the picture above, in 24-bit colour
VIGIA_THEME=light    # the same design for a light terminal
VIGIA_THEME=~/themes/mine
```

Nothing else is read. There is no flag for any of them, and no setting in one can change another.

The update look is one request, once, and it never repeats. It costs the opening frame nothing, because it runs beside the pane rather than in front of it, and every way it can go wrong (no network, a slow answer, a reply that makes no sense) is the same as it having nothing to say: **silence**. If you would rather it did not ask at all, `VIGIA_UPDATE=off`.

A theme file is usually about three lines. `base` picks a palette to start from and every line after it overrides one thing, so this keeps your terminal's own sixteen colours and adds the two backgrounds `ansi` declines to guess:

```ini
base        = ansi
added_row   = on #1b3d29
removed_row = on #45222a
```

<details>
<summary><b>🎨 The full theme format</b></summary>

<br>

`~/.config/vigia/theme` is read when it exists, on every platform, resolved from `HOME` or `USERPROFILE`. No file is the ordinary case and is not an error. A file that exists and does **not parse** is: `vigia` says which line and exits *before* it takes the screen, because an error painted inside a full-screen program that then hands the terminal back is an error nobody reads.

A key it does not recognise is an error naming the line, never a line quietly ignored.

A value is `[colour] [on colour] [modifiers]`:

| Part | Written as |
|---|---|
| Colour | `#rrggbb`, a palette index `0` to `255`, one of the sixteen names, or `default` |
| Names | `black` `red` `green` `yellow` `blue` `magenta` `cyan` `grey` `white`, each with a `bright-` twin |
| Background | `on` followed by a colour |
| Modifiers | `bold` `dim` `italic` `underline` `reverse`, any number |

Every key is documented in **[docs/THEME.md](docs/THEME.md)**, one row per key, grouped by surface. That file is the reference, and `crates/vigia/tests/theme_docs.rs` holds it against the code in both directions, so a key cannot land undocumented and a documented key cannot quietly stop existing. The short shape: `_warm` and `_hot` twins are the intensity rungs (a sparkline column and a heat slice both ramp through three levels, one mechanism), `bar_active` is a bar being dragged, and `bar_hover` and `path_hover` are the marks under the pointer.

**With no theme named, `vigia` asks your terminal its background at startup and picks `dark` or `light` from the answer.** A terminal that stays silent (ssh, some multiplexers) gets `ansi` instead, and **`ansi` draws no row wash at any depth**, deliberately. A wash has to assume a background and that palette assumes none: every colour in it is a *name*, so it resolves to whatever your terminal scheme says and `vigia` matches the pane beside it instead of arguing with it. The cost is the wash, which is why the three-line file above exists: keep `ansi` for the sixteen names your scheme already defines, and add the two backgrounds it declines to guess. Pick your own if your pane is lighter or darker. The only rule is that they stay far enough from your background to read as bands, and far enough from each other that an addition never looks like a removal.

</details>

<details>
<summary><b>✏️ Sparkline glyphs, and what to do if you see boxes</b></summary>

<br>

The per-file sparkline draws from the eighth-blocks `▁▂▃▄▅▆▇█` by default on terminals whose font may not carry anything denser, and from **braille** where it can. Braille packs two buckets into one cell, so the usual twelve-column strip fits six columns instead of twelve, and it survives on a narrower pane instead of halving and then disappearing.

**Nothing can ask a terminal which glyphs its font has.** There is no escape sequence for it, so this is decided the same way the colour depth is: from what the terminal calls itself. If the guess is wrong in either direction, say so:

```sh
VIGIA_GLYPHS=braille         # denser: 8 buckets in 4 columns
VIGIA_GLYPHS=block           # the safe floor, if you see boxes
VIGIA_GLYPHS=octant          # Unicode 16 solid 2x4, chosen for you where the terminal draws them
VIGIA_GLYPHS=auto            # decide for me, which is the default
```

**If the sparkline is a row of boxes, you want `block`.** That is a font without the braille patterns U+2800 to U+28FF, and it is the one direction detection cannot see. Windows is where this is most likely: the old console draws with Consolas, which carries none of them, so a bare `conhost` gets blocks and Windows Terminal gets braille.

`octant` is chosen for you exactly where it is not a bet: ghostty 1.2+, kitty 0.40+ and VTE-based terminals from 0.78 draw the Unicode 16 octants themselves, the way every terminal draws box drawing, and each of those names itself and its version in the environment. Everywhere else the octants would come from your font, most fonts do not have them yet, and the answer stays braille; `VIGIA_GLYPHS=octant` remains your word for a terminal the table does not know. foot 1.20+ draws them too and exports no version, so it is deliberately not promoted: a foot one release older would get tofu, and that trade is recorded in the code rather than taken silently.

</details>

<details>
<summary><b>🔦 Colour depth, and why your rows might be unwashed</b></summary>

<br>

`VIGIA_COLOR` overrides detection with `never`, `16`, `256`, `truecolor` or `auto`, and `NO_COLOR` is honoured.

**The row wash needs 24-bit colour.** It is dropped at every rung below rather than approximated, because a quantised background is a solid block, and a block behind highlighted code destroys the colours on it. The 256-colour cube is the case worth naming: its two darkest levels per channel are 0 and 95, so `#1b3d29` lands on `#005f00`, and a newly added file draws as a screen of flat green rather than a tint. Below 24-bit the diff signal is the `+` and `−` column, which is where it was before themes existed.

If your rows are unwashed and you know your terminal draws 24-bit, it is nearly always detection: `COLORTERM` is the only convention for claiming it and **nothing propagates it**. `ssh` forwards `TERM` and not `COLORTERM`, and a multiplexer replaces `TERM` with an entry of its own.

```sh
VIGIA_COLOR=truecolor        # settles it, in the pane or in your rc
```

Inside `tmux` that is only half of it, because `tmux` has to pass 24-bit through rather than round it to its own palette:

```sh
# ~/.tmux.conf
set -g  default-terminal "tmux-256color"
set -ga terminal-overrides ",*:Tc"
```

</details>

<details>
<summary><b>📋 Where a copied row actually goes</b></summary>

<br>

**Dragging the diff tries three ways to reach a clipboard and stops at the first that works.**

1. **Your machine's own clipboard tool**, which is `pbcopy` on macOS, `wl-copy` or `xclip` or `xsel` on Linux and `clip` on Windows. It needs nothing of your terminal and nothing of tmux, so on the machine you are sitting at it simply works.
2. **`tmux load-buffer -w -`**, when `$TMUX` says you are in a pane. The escape below reaches nothing there: `set-clipboard` has defaulted to `external` since tmux 2.6, and `external` lets tmux set the clipboard while forbidding the applications inside it. Handing the rows to tmux makes tmux the one setting it. `-w` arrived in **tmux 3.2**; an older one refuses and the next route is tried.
3. **OSC 52**, the escape, written straight to the terminal. It is the only one that crosses `ssh`, and it is the one every pane has.

**Over `ssh` the first is skipped**, because it would set a clipboard on the far machine that you cannot see, and succeed at doing it, which would stop the chain before the escape that crosses back to you.

Each tool is fed on standard input, so nothing in a copied row is ever read as an argument, and each gets a second to answer before the next is tried, because the loop carrying your copy is the loop drawing the pane.

**If the clipboard still does not change**, you are on the third route and your terminal is ignoring OSC 52. It has no reply, so nothing here can tell you: the footer says `sent` and means the bytes went. Two settings decide it, and neither is `vigia`'s:

```sh
tmux show -sv set-clipboard        # `off` swallows it whichever way it goes
tmux info | grep -i 'Ms:'          # empty means tmux cannot tell your terminal
```

```sh
# ~/.tmux.conf
set -ga terminal-features ",*:clipboard"
```

`Shift`+drag is the way round all of it: that is your terminal's own selection and its own clipboard.

</details>

### 🪟 The pane you want, every time

`f`, `r`, `s`, `o`, `a`, `w` and `c` change what the body is made of. Press `m` and flip them there, or say so once by hand:

```sh
# ~/.config/vigia/config
follow   = off    # stop the pane moving to the newest change; this is the off switch
rail     = on     # the file list beside the diff, from 139 columns
single   = on     # one file at a time
overview = on     # the file list alone, with no diff under it
staged   = on     # what is staged, beside what is not
wrap     = on     # a long line continues on the row below
notes    = off    # the note rows under their lines; this is the off switch
icons    = on     # a file-type glyph before every listed path (needs a Nerd Font)
links    = off    # paths are clickable file:// links; this is the off switch
persist  = on     # write what you flip in the menu back into this file
hide     = ^target/|\.lock$   # paths to keep out of the pane entirely
```

Same shape as the theme file: one key per line, `#` for a comment, and a key it does not know is an error rather than a shrug. No file is the ordinary case. The keys still work, so a setting is a starting point rather than a decision: `s` gives the whole diff back for as long as you want it. With `persist` on, the menu writes your flips back here and leaves every other line exactly as you wrote it, `hide` and your comments included.

**`hide` is the one setting that takes a value rather than `on` or `off`.** It is a regular expression, and it is *searched* rather than anchored, so `^target/|\.lock$` reads the way it looks and a bare `target` hides every path with that word anywhere in it. A matching file is gone from the list, the diff and the counts, and the header says `12 hidden` beside the changed count so you always know something is being kept from you. If your pattern covers everything that changed, the empty pane says `12 hidden` rather than pretending the tree is clean. The notes server does not take the pattern: it is about what the pane draws, and your agent has no pane. A pattern that does not compile is an error with its line on it, before the screen is taken. `#` still opens a comment, so a pattern that needs a literal one writes `[#]`. There is no key for it, deliberately: a gesture is for what changes while you are watching, and a file is for what was true before you opened the pane.

**`links` and `notes` are the two keys that start on**, so both are written above as the off switches they are. Every listed path is an OSC 8 hyperlink to its file, so a Ctrl+click (or however your terminal opens links) lands in your editor; a terminal that does not speak OSC 8 shows the same text and swallows the link, which is why nothing has to be detected. And a note you left for the agent draws its own rows under the line it is on, which `notes = off` turns down to the mark alone, the way `c` does for a session.

**`follow` is deliberately not a key.** Following the newest change is what makes the pane correct without being touched, so it is not something to turn off in a file. `f` turns it off for a session, which is where that choice belongs.

---
