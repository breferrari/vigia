# Changelog

Every released version of `vigia`, newest first. The date is the day the release was cut.

Before 1.0, a minor release can change behaviour. Anything that moves a key, a gesture or the default look is called out here.

## [0.40.0] - 2026-09-08

- A resolved note shows the agent's line before it leaves, and a tab wraps where it draws

## [0.39.0] - 2026-09-07

- The note is enclosed, its answer descends from it, and the pane's motions move to the DSL
- The guard reads a release's notes, and stops refusing their path
- A release says what changed, and CHANGELOG.md lists every version

## [0.38.0] - 2026-09-07

### Changed

- Notes draw in six inks of their own, so a note reads as a note rather than as diff.
- The prompt line says what each note is waiting on.

## [0.37.0] - 2026-09-07

### Added

- Notes on a diff line. The line number becomes a note icon under the pointer, and the note draws in a bordered box under its line.
- `vigia mcp` serves those notes to a coding agent over stdio, from a store kept per worktree.
- Enter posts a note straight into the running session's socket.
- The pane wakes on its own when the agent writes to the note store.

### Fixed

- A file changed in both the staged and the unstaged run takes one note placement, not one per run.

## [0.36.0] - 2026-09-04

### Fixed

- A file's height is recounted when it settles rather than at the next event, so the scrollbar stops sizing itself against a file still being written.
- The scrollbar a wrapped diff draws when it is shorter than its region is now reachable by the pointer.

## [0.35.2] - 2026-09-04

### Changed

- The footer's text crossfades, eased at both ends and long enough to watch.

## [0.35.1] - 2026-09-04

### Changed

- Every footer voice moves on the glyph channel, and moves for long enough to see.

## [0.35.0] - 2026-09-04

### Added

- The pane says so when a newer version of `vigia` has been published.

### Changed

- The footer gets three voices, and each arrives and leaves in its own way.

## [0.34.0] - 2026-09-04

### Changed

- A change arrives by coalescing rather than appearing all at once.

## [0.33.1] - 2026-09-03

### Fixed

- A fresh dependency resolve no longer picks up tinyvec 1.13.0, which does not compile.

## [0.33.0] - 2026-09-03

### Changed

- Letting go of a drag sends the selected rows, so the separate `y` step is gone.
- `Esc` closes the gestures sheet.

### Fixed

- A page step and the scrollbar thumb count rows of the diff rather than rows of the pane.
- One entry that cannot be read is one entry, not a broken frame.

## [0.32.0] - 2026-09-03

### Added

- Drag over the diff to select rows, and `y` sends their real text rather than what the terminal happens to have on screen.
- `y` copies the caret file's path.
- The gestures sheet says that text can be selected, which had been true and undocumented.

### Fixed

- The masthead graph's spike no longer sits inside the band.

## [0.31.1] - 2026-08-26

### Fixed

- The caret mark stays on the last edited file.
- gix 0.87, off the version that needed a yanked crate.

## [0.31.0] - 2026-08-26

### Added

- `w` wraps a long line so it can be read to its end, capped at two rows.

## [0.30.3] - 2026-08-26

### Fixed

- A list that is entirely staged says so, and reads as staged.

## [0.30.2] - 2026-08-26

### Fixed

- A file is never drawn without the label of the run it belongs to.

## [0.30.1] - 2026-08-26

### Fixed

- The pane stops showing what is no longer there.

## [0.30.0] - 2026-08-26

### Added

- Theming. A theme file decides what the pane is allowed to look like, and every theme key has a documented row.
- File paths are clickable links in terminals that support OSC 8.

### Changed

- The showcase look becomes the default.
- Colour ramps interpolate in Oklab, so their steps read evenly.
- Block glyphs use octants on terminals known to draw them, chosen by terminal version.
- The diff colours follow the delta formula.

## [0.29.1] - 2026-08-25

### Changed

- The staged mark moves onto the kind letter, which gives a column back to the path.

## [0.29.0] - 2026-08-25

### Added

- `a` draws the staged run beside the unstaged one.

## [0.28.0] - 2026-08-25

### Added

- The pane starts in the view the reader configured rather than in the built-in default.

### Fixed

- The gestures sheet names the two gestures it had been leaving out.

## [0.27.0] - 2026-08-25

### Added

- `s` pins the diff to one file.
- `←` and `→` move between changed files.

### Fixed

- The gestures sheet swallows the key press that closes it, so the press no longer reaches the pane behind.

## [0.26.0] - 2026-08-24

### Added

- `r` toggles the left rail.

## [0.25.1] - 2026-08-24

### Fixed

- The gestures sheet pages, so a small pane stops dropping gestures in silence.

## [0.25.0] - 2026-08-24

### Changed

- The file list becomes a left rail beside the diff.

### Fixed

- The gestures sheet spends the width it has, so a short pane stops dropping the mouse group.

## [0.24.0] - 2026-08-22

### Fixed

- The churn band's yardstick stops being set by one loud burst, and the braille rung comes back with it.

## [0.23.0] - 2026-08-22

### Changed

- Follow lands on the change itself rather than on the filename above it.
- Under a level, the churn band draws at the block rung.

### Fixed

- A screenful of prose stops costing 100ms a frame.

## [0.22.0] - 2026-08-21

### Fixed

- The frame path stops parsing under a grammar nothing has compiled, so the first Markdown file in a session no longer costs half a second on the frame that draws it.

## [0.21.0] - 2026-08-21

### Changed

- The file list deepens on a tall pane.
- The sparkline's bucket count follows the pane width.

## [0.20.1] - 2026-08-19

### Fixed

- The vendored grammars ship their licence texts, not just their names.

## [0.20.0] - 2026-08-19

### Changed

- The grammar set becomes one vetted dump covering every modern language.

## [0.19.0] - 2026-08-18

### Changed

- The churn band runs under the scrollbar, and the track moves with it.
- The glance elements draw a level.

### Fixed

- The published crate carries the licence text.

## [0.18.0] - 2026-08-18

### Changed

- The glance elements become a graph.

## [0.17.0] - 2026-08-18

### Changed

- The sparkline draws twelve buckets of ten seconds.
- The churn band draws in columns rather than in cells.

## [0.16.0] - 2026-08-18

### Added

- A braille rung above the block ramp for the sparkline.

## [0.15.0] - 2026-08-17

### Changed

- The churn band gets its left bar and reaches the bar's own column.
- The header gets air, and the sigil gets a column of its own.

## [0.14.0] - 2026-08-17

### Changed

- The gestures sheet is opaque.

## [0.13.0] - 2026-08-17

### Changed

- The bottom bar keeps `q`, `f` and `?` and nothing else.

## [0.12.0] - 2026-08-17

### Added

- `?` opens a sheet of every gesture, over the pane.

## [0.11.1] - 2026-08-17

- Internal changes only. Nothing a user of the pane can see moved.

## [0.11.0] - 2026-08-17

### Changed

- The churn band starts hidden, and `m` is how it arrives.

## [0.10.0] - 2026-08-16

### Added

- The worktree churn band, in a masthead that names the branch.
- The glance row gets slices and a colour ramp.

## [0.9.0] - 2026-08-16

### Changed

- The pointer takes the bar's colour, and the caret's row takes the bold.

## [0.8.0] - 2026-08-16

### Added

- The scrollbar thumb and every listed file take a hover mark.

## [0.7.0] - 2026-08-16

### Added

- A hover mark on the scrollbars, and the takeover asks the terminal for focus events.

## [0.6.0] - 2026-08-16

### Changed

- A scroll lights one arrow on one bar.

## [0.5.0] - 2026-08-16

### Fixed

- A drag keeps the bar it started on, and a gesture in progress is lit.
- The scrollbar's track and its buttons were invisible, at 1.24:1 contrast.

## [0.4.0] - 2026-08-15

### Added

- The scrollbar grows a step button at each end, and one click is one step.

## [0.3.0] - 2026-08-15

### Changed

- A blank row closes every file's block but the last.
- The sigil stands off its line.
- The counters take the diff's green and red, and a zero keeps the grey.

## [0.2.1] - 2026-08-15

### Fixed

- The file list draws the rank the digit keys address.

## [0.2.0] - 2026-08-14

### Added

- `n` and `p` step a file, and the digit keys name a drawn row.
- `d` and `u` are the half page.

### Changed

- The pane stands back from its own edge, and its furniture does not.

## [0.1.0] - 2026-08-09

First public release. `vigia` shows the working tree diff in a terminal pane and redraws it as the tree changes.

### Added

- The working tree diff, fullscreen, redrawn on filesystem events rather than on a timer.
- Follow mode, on by default, so the newest change is scrolled to without being asked.
- Syntax highlighting, with the first frame drawn plain so startup stays imperceptible.
- A per-file heat strip and a sparkline, over a churn history bounded to 256 paths and 120 seconds.
- Keyboard and mouse scrolling, and a scrollbar.
- 256-colour and monochrome degradation paths.
- The terminal restored on every exit the process can observe, including an external kill.
- Legible at 40 columns.
- Prebuilt binaries for Linux, macOS and Windows, `cargo install vigia`, and a Homebrew tap.
