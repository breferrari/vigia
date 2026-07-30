//! Drawing a [`View`] into a buffer, and nothing else.
//!
//! Pure: same view, same theme, same area, same cells. That is what makes the
//! snapshot suite worth anything, and it is why the frame path, the scroll
//! arithmetic and the terminal all live somewhere else.
//!
//! **No borders, no boxes.** `btop` frames everything, and `btop` has the whole
//! screen. Half a laptop screen beside an agent is the case I6 names, and a box
//! spends two of forty columns and two of twenty-four rows on decoration. Every
//! cell here goes to the diff or to the chrome: one line at the top, and one at
//! the bottom that takes a second only when forty columns cannot hold it.
//!
//! Content is written cell by cell rather than through the widget set. A
//! `Paragraph` would wrap, and wrapping is the one thing a monitor must not do:
//! a wrapped diff line moves every line below it, so the shape of the screen
//! stops meaning anything. Lines are clipped instead.
//!
//! ## I6, which is the whole of the layout
//!
//! `SPEC.md` §11.1 states the rule this module implements: **a thing made of
//! items breaks, a thing made of characters marks its edge, and content is
//! neither.**
//!
//! The hint bar is the only thing on screen made of items, so it is the only
//! thing that breaks — onto [`Footer`]'s second line, and then by dropping whole
//! rungs of [`HINT_RUNGS`]. Everything else is one token and says which end it
//! lost: [`ELIDED`] on the left for a file path, whose tail names the file, and
//! [`CONTINUES`] on the right for everything else, content included.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span as TextSpan;
use vigia_core::{Class, LineKind, Span};

use crate::theme::Theme;
use crate::view::{Row, View};

/// Columns a tab advances to the next multiple of.
///
/// Four, and not configurable: `SPEC.md` names no setting for it, and a monitor
/// that has to be told how to draw a tab has already lost. Expanding matters
/// more than the number does, because a raw `\t` written into a terminal cell
/// renders as nothing and silently misaligns everything after it.
const TAB_STOP: usize = 4;

/// Stands in for a character that cannot be drawn.
///
/// Chosen for reach rather than beauty: U+00B7 is in Latin-1 and in CP437, so it
/// survives the legacy Windows console that `SPEC.md` §10 leaves open, and it
/// does not read as file content the way `.` or `?` would.
const UNPRINTABLE: char = '·';

/// Shown where a path had to lose its head to fit.
const ELIDED: char = '…';

/// Shown where anything else ran past the right edge.
///
/// The two marks are two directions and never overlap: [`ELIDED`] on the left
/// says the beginning is gone, this one on the right says it continues. A
/// reader never has to work out which end they are missing.
const CONTINUES: &str = "›";

/// The footer's left-hand side when there is nothing wrong, widest rung first.
///
/// **Each rung is the one above it minus whole hints**, and nothing is reworded
/// on the way down: a bar that shortened `jk scroll` to `jk scr` is the
/// truncated-to-useless shape I6 forbids, and one that invented a shorter
/// wording would teach a second dialect at exactly the width where the reader
/// has least to go on. `tests/legibility.rs` gates both properties over the
/// rungs it observes by rendering, so this table cannot drift from what ships.
///
/// The drop order is `SPEC.md` §11.1's ruling. `jk scroll` goes first and
/// `f follow` is last standing: `q` and `jk` are pager reflexes and four keys
/// reach quit, while `f` is the one nobody would guess and the only one that
/// restores a state a reader can lose without noticing. It only fires below
/// twenty-nine columns, because above that [`Footer`] gives the bar a line of
/// its own rather than shortening it.
const HINT_RUNGS: [&str; 4] = [
    "q quit · f follow · jk scroll",
    "q quit · f follow",
    "f follow",
    "",
];

/// What joins two hints.
///
/// Exported because `tests/legibility.rs` splits the rendered bar on it to check
/// that every hint on screen is a whole one. A test that restated the separator
/// as its own literal would be a second implementation of the parse, agreeing
/// with itself while disagreeing with the screen. The ladder is deliberately
/// **not** exported for the same reason inverted: a test comparing the rung
/// table against itself proves nothing, so the rungs are observed by rendering.
pub const HINT_SEPARATOR: &str = " · ";

/// The smallest body a second footer line may leave behind.
///
/// Two rows, because that is the shortest thing that still reads as a diff: a
/// file heading and one line under it. Below that the footer would be buying
/// legibility with the content it exists to make legible.
const MIN_BODY: u16 = 2;

/// Shown on the footer while the viewport is moving itself.
///
/// The mockup's own words. It sits with the position rather than with the
/// hints because it is **state**, not advice, and a notice replaces the hints:
/// a reader being told a file could not be read still needs to know whether
/// what they are looking at is live.
const FOLLOWING: &str = "follow ▶";

/// The narrowest the text column may get before line numbers are dropped.
///
/// Below this the gutter costs more than it explains, which is the shape of
/// "truncated to useless" that I6 forbids. At forty columns with four-digit line
/// numbers the text still gets thirty-four, so the gutter survives the case the
/// invariant is actually about.
const MIN_TEXT_WIDTH: usize = 24;

/// What the chrome says that the view itself does not know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chrome {
    /// Name of the working tree being watched.
    pub worktree: String,
    /// Something the reader should see instead of the key hints.
    ///
    /// A monitor survives a failed frame rather than exiting, so this is where a
    /// missing blob mid-`git gc` or an unreadable file goes. It replaces the
    /// hints because a reader who has just been told something is wrong does not
    /// need reminding that `q` quits.
    pub notice: Option<String>,
    /// Whether the viewport is moving itself to what just changed.
    ///
    /// Drawn, because I5 is otherwise invisible: a view that has not moved
    /// because nothing changed and one that has not moved because following
    /// was switched off look identical, and the reader's next action differs
    /// completely between them.
    pub following: bool,
}

/// `N/M`, or nothing at all when there is no diff to be positioned within.
///
/// One function rather than two because it is built twice per frame from
/// different inputs: [`Footer::plan`] wants the widest position a file count can
/// produce, and the draw wants the real one. Written twice, the two would
/// eventually disagree about a format the layout is measured against.
fn position_of(file: usize, files: usize) -> String {
    if files == 0 {
        String::new()
    } else {
        // Saturating because [`View`] has public fields and [`render`] is public,
        // so nothing stops a caller handing over a position past the end of its
        // own file list. [`View::collect`] always clamps, but the type does not.
        format!("{}/{files}", file.saturating_add(1))
    }
}

/// The state's ladder, widest rung first.
///
/// `follow ▶  N/M`, then the marker alone, then nothing. The position goes
/// before the marker because the header already carries the file count, so
/// `N/M` is the half a reader can reconstruct; whether the view is still live is
/// not recoverable from anywhere else on the screen.
///
/// Always ends in an empty rung, which is what makes [`widest_fitting`] total.
fn state_rungs(following: bool, position: &str) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    match (following, position.is_empty()) {
        // `follow ▶ ` with nothing after it would read as a truncation rather
        // than a state, so a clean worktree gets the marker on its own.
        (true, false) => {
            rungs.push(format!("{FOLLOWING}  {position}"));
            rungs.push(FOLLOWING.to_owned());
        }
        (true, true) => rungs.push(FOLLOWING.to_owned()),
        (false, false) => rungs.push(position.to_owned()),
        (false, true) => {}
    }
    rungs.push(String::new());
    rungs
}

/// The widest rung of `ladder` that fits in `room`.
///
/// Ladders are written widest first, so this is the first that fits. Every
/// ladder ends in an empty rung, so the fallback is unreachable rather than a
/// silent default.
fn widest_fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    ladder
        .iter()
        .map(AsRef::as_ref)
        .find(|rung| width_of(rung) <= room)
        .unwrap_or("")
}

/// What the footer will draw, and how many rows it needs.
///
/// Planned rather than drawn, because two callers need the answer before there
/// is anything to draw: [`body_height`] has to know how many rows are left for
/// the body, and [`render`] has to put the body somewhere that does not collide
/// with it. Both go through here with the same inputs, so the row budget and the
/// layout are one computation and cannot drift apart.
struct Footer<'a> {
    /// One, or two when a single line cannot hold both halves. Zero on a screen
    /// with no room for a footer at all.
    rows: u16,
    /// Columns the state may take on its line.
    reserved: usize,
    /// The hints rung, or the notice.
    left: &'a str,
    /// Whether `left` is a notice, which is what decides its colour.
    alert: bool,
}

impl<'a> Footer<'a> {
    /// Decide the footer's shape from the width, the state, and the file count.
    ///
    /// **From the file count, never the scroll position.** `{files}/{files}` is
    /// the widest position that count can produce, so reserving it means the
    /// footer cannot gain or lose a row while a reader scrolls from file 9 to
    /// file 10. A layout that reflowed under scrolling would be worse than one
    /// that is occasionally a column meaner than it had to be, and the meanness
    /// only shows below seventeen columns.
    fn plan(area: Rect, chrome: &'a Chrome, files: usize) -> Self {
        let width = usize::from(area.width);
        if area.height < 2 {
            return Self {
                rows: 0,
                reserved: 0,
                left: "",
                alert: false,
            };
        }

        // The last file's position is the widest this count can produce, since
        // no position numbers higher than the count itself.
        let widest = position_of(files.saturating_sub(1), files);
        let reserved = width_of(widest_fitting(
            &state_rungs(chrome.following, &widest),
            width,
        ));
        // The gap keeps the state from touching the hints, and is only owed when
        // there is a state to keep away from them.
        let taken = if reserved == 0 { 0 } else { reserved + 1 };

        // A second line is worth taking only if it buys something: there has to
        // be a state to move up to it, and a body still worth showing
        // underneath. One header, two footer rows and `MIN_BODY` is the shortest
        // screen where both hold.
        //
        // **Measured against the hints, never against a notice.** A notice is
        // transient — a file that vanished between being named and being read,
        // a repository mid-`git gc` — so letting it decide the height would jog
        // the reader's diff down a row and back every time one flickered. That
        // is the same thing I5 ruled out for a terminal resize: a monitor does
        // not move content for something that expresses no intent. It also means
        // this height is a function of width, follow state and file count alone,
        // so a caller that sampled the chrome before a notice was raised still
        // gets the answer the renderer will use.
        let grows =
            width_of(HINT_RUNGS[0]) + taken > width && reserved > 0 && area.height >= 3 + MIN_BODY;
        let rows = if grows { 2 } else { 1 };

        let room = if grows {
            width
        } else {
            width.saturating_sub(taken)
        };
        // A notice is one token: it takes whatever room the line gives it and
        // marks the cut. The hints are a list, so they drop whole rungs instead.
        let (left, alert) = match &chrome.notice {
            Some(notice) => (notice.as_str(), true),
            None => (widest_fitting(&HINT_RUNGS, room), false),
        };

        Self {
            rows,
            reserved,
            left,
            alert,
        }
    }
}

/// Body height available for rows in this area, which is what a caller has to
/// ask [`View::collect`] for.
///
/// One line goes to the header and one or two to the footer, so this needs the
/// same inputs the footer is planned from: `files` is
/// [`vigia_core::Frame::files`]'s length, which a caller knows before collecting
/// anything and which equals [`View::files`] afterwards.
///
/// Saturating rather than clamped so a one-row terminal asks for nothing instead
/// of underflowing.
pub fn body_height(area: Rect, chrome: &Chrome, files: usize) -> usize {
    let footer = Footer::plan(area, chrome, files);
    usize::from(area.height).saturating_sub(1 + usize::from(footer.rows))
}

/// Draw a whole screen: one header line, the body, and one or two footer lines.
///
/// Any area is legal, including one too short for a body and one column wide. A
/// monitor that panics when a pane is dragged narrow is worse than one that
/// draws something cramped.
pub fn render(buf: &mut Buffer, area: Rect, view: &View, theme: &Theme, chrome: &Chrome) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Planned from `view.files`, which on the path that matters is the same
    // number `body_height`'s caller passed: `View::collect` copies it straight
    // off the frame and changes nothing. Where they can differ is a caller
    // redrawing a *stale* view after a failed collect, and that costs nothing,
    // because the plan and the draw below both read this one value and are
    // therefore consistent with each other whatever it holds.
    let footer = Footer::plan(area, chrome, view.files);

    let mut painter = Painter {
        buf,
        theme,
        gutter: 0,
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if footer.rows > 0 {
        painter.footer(area, view, chrome, &footer);
    }

    let rows = area.height.saturating_sub(1 + footer.rows);
    if rows > 0 {
        painter.body(
            Rect {
                y: area.y + 1,
                height: rows,
                ..area
            },
            view,
        );
    }
}

/// A buffer, a palette, and the one measurement the body rows share.
struct Painter<'a> {
    buf: &'a mut Buffer,
    theme: &'a Theme,
    /// Digits reserved for line numbers, or zero when there is no room.
    gutter: usize,
}

impl Painter<'_> {
    /// Write `text` at `x`, clipped to `limit` columns, and return the next
    /// column.
    fn put(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) -> u16 {
        if limit == 0 {
            return x;
        }
        let (next, _) = self.buf.set_stringn(x, y, text, limit, style);
        next
    }

    /// Write `text` clipped to `limit`, and say so when it did not fit.
    ///
    /// This is I6's rule for everything that is one token rather than a list:
    /// the worktree name, a notice, a hunk header, a note, the empty-state line
    /// and a line of file content. None of them can drop an item the way the
    /// hint bar can, and none has an identifying half the way a path does, so
    /// the honest thing is to fill the room and mark the edge.
    ///
    /// The mark gets a **reserved** column rather than overwriting the last one,
    /// and the reason is not the one it looks like. Overwriting cannot corrupt
    /// the row: `ratatui` refuses to write into the continuation cell a
    /// two-column glyph covers. What it does instead is **drop the mark**, so a
    /// row filled to its last column by a wide glyph is drawn as one that simply
    /// ends. Reserving the column is what guarantees the mark always lands.
    /// `tests/legibility.rs` gates it, and a plain ASCII line never reaches the
    /// case because it ends on a one-column character.
    fn put_marked(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) {
        if limit == 0 || text.is_empty() {
            return;
        }
        if width_of(text) <= limit {
            self.put(x, y, text, limit, style);
            return;
        }
        // `limit` is always derived from a screen width, so it fits in a `u16`.
        self.put(x, y, text, limit - 1, style);
        self.buf
            .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, style);
    }

    /// Write a sequence of styled runs under **one** limit, marking the edge.
    ///
    /// The many-run form of [`Painter::put_marked`], and it has to be one call
    /// rather than a `put_marked` per run. The limit belongs to the row, but the
    /// mark belongs to whichever run happens to reach the edge, and a per-run
    /// version would either mark every run or none of them.
    ///
    /// The mark gets a reserved column for exactly the reason `put_marked` gives:
    /// `ratatui` refuses to write into the continuation cell of a two-column
    /// glyph, so overwriting the last column silently **drops** the mark and a
    /// clipped line is drawn as one that simply ends. `tests/legibility.rs`
    /// sweeps every width for that.
    ///
    /// `total` is the runs' width, which the caller already accumulated while
    /// building them. Passed in rather than re-derived: measuring it here walks
    /// every line a second time and undoes the ASCII fast path [`printable`]
    /// exists for.
    fn put_runs_marked(
        &mut self,
        x: u16,
        y: u16,
        runs: &[(String, Style)],
        total: usize,
        limit: usize,
    ) {
        if limit == 0 {
            return;
        }

        let overflows = total > limit;
        let budget = if overflows { limit - 1 } else { limit };

        // The style the mark inherits: whichever run ran out of room, so a
        // clipped comment is marked in the comment's colour rather than in
        // whatever the row started with.
        //
        // Seeded from the **first** run rather than from a theme default,
        // because at `limit == 1` the budget is zero and the loop below writes
        // nothing at all. Seeded with `context` instead, a one-column diff row
        // marked in `Reset` where `put_marked` on the same content would have
        // used the caller's style, and the two spellings of one rule had already
        // drifted apart at the one width nothing tests.
        let mut marked_in = runs.first().map_or(self.theme.context, |(_, style)| *style);
        let end = x + budget as u16;
        let mut at = x;
        for (text, style) in runs {
            if at >= end {
                break;
            }
            marked_in = *style;
            at = self.put(at, y, text, usize::from(end - at), *style);
        }

        if overflows {
            self.buf
                .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, marked_in);
        }
    }

    /// Write `text` so that it ends at the right edge of `area`.
    ///
    /// Dropped entirely rather than truncated when it does not fit. Half a count
    /// on the right of a line is noise; its absence is at least honest.
    fn put_right(&mut self, area: Rect, text: &str, style: Style) -> usize {
        let width = width_of(text);
        if width == 0 || width > usize::from(area.width) {
            return 0;
        }
        self.buf.set_stringn(
            area.x + area.width - width as u16,
            area.y,
            text,
            width,
            style,
        );
        // The gap keeps the right-hand text from touching whatever is drawn from
        // the left, which at forty columns happens constantly.
        width + 1
    }

    /// One line of chrome: something on the left, something on the right, and the
    /// right-hand side wins the space.
    ///
    /// The header and the footer are the same shape, and having them share it is
    /// not only brevity. The right-hand text is placed first so that a long name
    /// on the left loses characters to it rather than the other way round: the
    /// number is what changes, and what changes is what a glance is for. Written
    /// twice, one of them eventually stops doing that.
    fn status_line(&mut self, area: Rect, left: &str, style: Style, right: &str) {
        self.buf.set_style(area, self.theme.chrome_dim);
        let taken = self.put_right(area, right, self.theme.chrome_dim);
        let room = usize::from(area.width).saturating_sub(taken);
        self.put_marked(area.x, area.y, left, room, style);
    }

    fn header(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        let files = match view.files {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        // The worktree name and nothing else on the left. A title bar reading
        // `vigia` spends six of forty columns telling the reader which program
        // they started, which is the one thing they already know.
        //
        // The header never takes a second line the way the footer does. A name
        // is not a list and has nowhere to break, so a second line could not
        // guarantee a fit and would spend a body row on a maybe.
        self.status_line(area, &chrome.worktree, self.theme.chrome, &files);
    }

    /// The footer, on the bottom one or two rows of `area`.
    ///
    /// Takes the whole area rather than its own rows, because which rows it owns
    /// is what [`Footer::plan`] decided.
    fn footer(&mut self, area: Rect, view: &View, chrome: &Chrome, footer: &Footer<'_>) {
        let position = position_of(view.top.file, view.files);
        // Clamped to what was reserved, not to the width. The plan handed the
        // rest of the line to the hints, and the drawn position can be narrower
        // than the widest one reserved for it, so a state sized to the width
        // would draw over them.
        let rungs = state_rungs(chrome.following, &position);
        let state = widest_fitting(&rungs, footer.reserved);

        let style = if footer.alert {
            self.theme.alert
        } else {
            self.theme.chrome_dim
        };
        let bottom = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };

        if footer.rows == 2 {
            // State above, hints below. The hints keep the bottom row they had
            // at eighty columns, so narrowing a pane moves the new line in
            // rather than moving the old one out from under the reader.
            let upper = Rect {
                y: bottom.y - 1,
                ..bottom
            };
            self.status_line(upper, "", self.theme.chrome_dim, state);
            self.status_line(bottom, footer.left, style, "");
        } else {
            self.status_line(bottom, footer.left, style, state);
        }
    }

    fn body(&mut self, area: Rect, view: &View) {
        if view.files == 0 {
            self.put_marked(
                area.x,
                area.y,
                "working tree clean",
                usize::from(area.width),
                self.theme.chrome_dim,
            );
            return;
        }

        self.gutter = gutter_width(view, usize::from(area.width));
        for (offset, row) in view.rows.iter().take(usize::from(area.height)).enumerate() {
            let y = area.y + offset as u16;
            match row {
                Row::File {
                    path,
                    from,
                    kind,
                    churn,
                } => self.file_row(Rect { y, ..area }, *kind, path, from.as_deref(), *churn),
                Row::Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                } => {
                    let text = format!(
                        "@@ -{} +{} @@",
                        span(*old_start, *old_lines),
                        span(*new_start, *new_lines)
                    );
                    // Marked rather than clipped, and this is the row where it
                    // matters most: `@@ -258,7 +25` is not a shortened header,
                    // it is a header naming a different line.
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.hunk);
                }
                Row::Note(note) => {
                    let text = format!("  {note}");
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.note);
                }
                Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                } => {
                    self.line_row(Rect { y, ..area }, *kind, *number, text, spans);
                }
            }
        }
    }

    /// `M src/frame.rs                             +12 -3`
    fn file_row(
        &mut self,
        area: Rect,
        kind: char,
        path: &str,
        from: Option<&str>,
        churn: Option<(u32, u32)>,
    ) {
        let counts = churn
            .map(|(added, removed)| format!("+{added} -{removed}"))
            .unwrap_or_default();
        let taken = self.put_right(area, &counts, self.theme.chrome_dim);
        let mut room = usize::from(area.width).saturating_sub(taken);

        let letter = format!("{kind} ");
        let x = self.put(area.x, area.y, &letter, room, self.theme.kind);
        room = room.saturating_sub(usize::from(x - area.x));

        let mut label = path.to_owned();
        if let Some(from) = from {
            // Which file it *was* is the whole content of a rename, so it is
            // part of the label rather than something to reveal on a keypress.
            label.push_str(" ← ");
            label.push_str(from);
        }
        self.put(x, area.y, &elide_head(&label, room), room, self.theme.path);
    }

    /// `  128 +    let value = 1;`
    ///
    /// **The sigil carries the diff, the text carries the syntax**, which is
    /// `SPEC.md` §11.1's ruling and the mockup's own layout: added, removed and
    /// context lines are highlighted identically, and only the `+` or `-` says
    /// which is which. What the picture uses to make that legible is a row
    /// background tint, and sixteen foreground-only colours cannot draw one, so
    /// the signal here is thinner than in the picture until #11 lands a
    /// truecolour path.
    fn line_row(&mut self, area: Rect, kind: LineKind, number: u32, text: &str, spans: &[Span]) {
        let (diff, sigil) = match kind {
            LineKind::Added => (self.theme.added, '+'),
            LineKind::Removed => (self.theme.removed, '-'),
            LineKind::Context => (self.theme.context, ' '),
        };

        let mut x = area.x;
        let mut room = usize::from(area.width);
        if self.gutter > 0 {
            let gutter = self.gutter;
            let numbered = format!("{number:>gutter$} ");
            x = self.put(x, area.y, &numbered, room, self.theme.gutter);
            room = room.saturating_sub(gutter + 1);
        }

        let mut runs = Vec::with_capacity(spans.len() + 2);
        runs.push((sigil.to_string(), diff));

        // Tab stops are counted from the start of the line's own content, not
        // from the left edge of the screen. The gutter and the sigil shift every
        // row by the same amount, so including them would align tabs to the
        // buffer and leave the file's indentation looking nothing like it does in
        // an editor. The counter therefore runs **across** span boundaries: a tab
        // in the middle of a line advances to the next stop measured from the
        // line's own start, not from the start of whatever run it landed in.
        let mut column = 0usize;
        let mut at = 0usize;
        for span in spans {
            let end = (at + span.len).min(text.len());
            let Some(piece) = text.get(at..end) else {
                // A span boundary that is not a character boundary. It should not
                // happen and it must not panic, because the alternative to one
                // uncoloured line is a monitor that dies on a file. The rest of
                // the line is drawn unclassified.
                break;
            };
            if !piece.is_empty() {
                runs.push((printable(piece, &mut column), self.theme.class(span.class)));
            }
            at = end;
        }
        if at < text.len() {
            // Whatever the spans did not reach, which is the whole line when
            // there are none: an unrecognised file type, or a row a test built
            // by hand.
            //
            // Styled through `class` rather than reaching for `context`
            // directly, so that "an empty span list and one `Plain` span reach
            // the screen identically" stays one rule instead of two expressions
            // that happen to agree. #11 gives the classes their own palette, and
            // the first `Plain` that is not `context` would otherwise leave this
            // path quietly on the old colour.
            runs.push((
                printable(&text[at..], &mut column),
                self.theme.class(Class::Plain),
            ));
        }

        // Content is the one thing that can neither break nor elide: wrapping it
        // would move every line below it, and no part of a line is its
        // identifying part the way a path's tail is. So it says it continues and
        // nothing more. `SPEC.md` §11.1 rules that this is not what I6 means by
        // a truncated label.
        // The sigil is one column and is pushed before the counter starts, so
        // the runs' total width is the counter plus it.
        self.put_runs_marked(x, area.y, &runs, column + 1, room);
    }
}

fn width_of(text: &str) -> usize {
    TextSpan::raw(text).width()
}

/// One side of a hunk header, in git's own shorthand.
///
/// Git omits the count when a side covers exactly one line, and a reader
/// calibrated on `git diff` reads its absence as "one". Reproducing that is
/// cheaper than teaching them a second dialect.
fn span(start: u32, lines: u32) -> String {
    if lines == 1 {
        format!("{start}")
    } else {
        format!("{start},{lines}")
    }
}

/// Digits to reserve for line numbers, or zero to draw none.
///
/// Sized from the largest number actually on screen rather than from the file,
/// so the gutter does not widen for content nobody can see.
fn gutter_width(view: &View, width: usize) -> usize {
    let largest = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Line { number, .. } => Some(*number),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let digits = largest.max(1).ilog10() as usize + 1;
    // The gutter costs its digits plus a space, and the sigil costs one more.
    if width.saturating_sub(digits + 2) >= MIN_TEXT_WIDTH {
        digits
    } else {
        0
    }
}

/// Keep the tail of `text`, marking the loss, when it will not fit.
///
/// The tail, because the end of a path is the part that identifies the file. A
/// column reading `crates/vigia-core/…` names nothing, which is exactly the
/// truncated-to-useless label I6 forbids. This is the **only** direction on the
/// screen that keeps its end rather than its start, and it is why the two marks
/// are different characters: a path says its head is gone, everything else says
/// its tail continues.
///
/// One column is enough to say so. At `room == 1` the whole path is gone and the
/// result is a bare [`ELIDED`], which is honest about naming nothing rather than
/// showing an arbitrary first character as if it were a name.
fn elide_head(text: &str, room: usize) -> String {
    if width_of(text) <= room {
        return text.to_owned();
    }
    if room == 0 {
        return String::new();
    }

    // Walked backwards, accumulating width, rather than forwards testing each
    // suffix. The forward form re-measures the whole tail once per candidate,
    // which is quadratic in the path length and runs once per visible file row.
    let budget = room - 1;
    let mut start = text.len();
    let mut kept_width = 0usize;
    for (i, c) in text.char_indices().rev() {
        let next = kept_width + width_of(&text[i..i + c.len_utf8()]);
        if next > budget {
            break;
        }
        kept_width = next;
        start = i;
    }

    let mut kept = String::with_capacity(text.len() - start + ELIDED.len_utf8());
    kept.push(ELIDED);
    kept.push_str(&text[start..]);
    kept
}

/// Make one line of file content safe to write into terminal cells.
///
/// Two hazards, both from content nobody wrote for a display. A tab occupies one
/// cell and advances nothing, so everything after it in the row sits at the
/// wrong column. And a control character written straight through can move the
/// cursor or open an escape sequence, which corrupts the whole screen rather
/// than one row.
///
/// Columns are counted from the start of the **line**, which is where the file
/// counts them from too, so `column` is threaded in by the caller and carried
/// across the runs one line is made of. A per-run counter would reset at every
/// syntax boundary and align a tab to the token before it rather than to the
/// line, which is invisible until a file indents with tabs and then wrong on
/// every row of it.
fn printable(text: &str, column: &mut usize) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            '\t' => {
                let stop = TAB_STOP - (*column % TAB_STOP);
                out.extend(std::iter::repeat_n(' ', stop));
                *column += stop;
            }
            c if c.is_control() => {
                out.push(UNPRINTABLE);
                *column += 1;
            }
            c if c.is_ascii() => {
                out.push(c);
                *column += 1;
            }
            c => {
                out.push(c);
                // Only the non-ASCII tail pays for a width lookup, which keeps
                // the common line off the measuring path entirely.
                *column += width_of(&text[i..i + c.len_utf8()]);
            }
        }
    }
    out
}
