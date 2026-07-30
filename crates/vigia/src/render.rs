//! Drawing a [`View`] into a buffer, and nothing else.
//!
//! Pure: same view, same theme, same area, same cells. That is what makes the
//! snapshot suite worth anything, and it is why the frame path, the scroll
//! arithmetic and the terminal all live somewhere else.
//!
//! **No borders, no boxes.** `btop` frames everything, and `btop` has the whole
//! screen. Half a laptop screen beside an agent is the case I6 names, and a box
//! spends two of forty columns and two of twenty-four rows on decoration. Every
//! cell here goes to the diff or to one line of chrome at each end.
//!
//! Content is written cell by cell rather than through the widget set. A
//! `Paragraph` would wrap, and wrapping is the one thing a monitor must not do:
//! a wrapped diff line moves every line below it, so the shape of the screen
//! stops meaning anything. Lines are clipped instead.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use vigia_core::LineKind;

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
}

/// Body height available for rows in this area, which is what a caller has to
/// ask [`View::collect`] for.
///
/// One line goes to the header and one to the footer. Saturating rather than
/// clamped so a one-row terminal asks for nothing instead of underflowing.
pub fn body_height(area: Rect) -> usize {
    usize::from(area.height).saturating_sub(2)
}

/// Draw a whole screen: one header line, the body, one footer line.
///
/// Any area is legal, including one too short for a body and one column wide. A
/// monitor that panics when a pane is dragged narrow is worse than one that
/// draws something cramped.
pub fn render(buf: &mut Buffer, area: Rect, view: &View, theme: &Theme, chrome: &Chrome) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut painter = Painter {
        buf,
        theme,
        gutter: 0,
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if area.height >= 2 {
        painter.footer(
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
            view,
            chrome,
        );
    }

    if area.height >= 3 {
        painter.body(
            Rect {
                y: area.y + 1,
                height: area.height - 2,
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

    fn header(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        self.buf.set_style(area, self.theme.chrome_dim);

        let files = match view.files {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        // The count is placed first so a long worktree name loses characters to
        // it rather than the other way round: the number is what changes, and
        // what changes is what a glance is for.
        let taken = self.put_right(area, &files, self.theme.chrome_dim);
        let room = usize::from(area.width).saturating_sub(taken);

        // The worktree name and nothing else. A title bar reading `vigia` spends
        // six of forty columns telling the reader which program they started,
        // which is the one thing they already know.
        self.put(area.x, area.y, &chrome.worktree, room, self.theme.chrome);
    }

    fn footer(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        self.buf.set_style(area, self.theme.chrome_dim);

        let position = if view.files == 0 {
            String::new()
        } else {
            format!("{}/{}", view.top.file + 1, view.files)
        };
        let taken = self.put_right(area, &position, self.theme.chrome_dim);
        let room = usize::from(area.width).saturating_sub(taken);

        match &chrome.notice {
            Some(notice) => self.put(area.x, area.y, notice, room, self.theme.alert),
            None => self.put(
                area.x,
                area.y,
                "q quit  jk scroll",
                room,
                self.theme.chrome_dim,
            ),
        };
    }

    fn body(&mut self, area: Rect, view: &View) {
        if view.files == 0 {
            self.put(
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
                    self.put(area.x, y, &text, usize::from(area.width), self.theme.hunk);
                }
                Row::Note(note) => {
                    let text = format!("  {note}");
                    self.put(area.x, y, &text, usize::from(area.width), self.theme.note);
                }
                Row::Line { kind, number, text } => {
                    self.line_row(Rect { y, ..area }, *kind, *number, text);
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
    fn line_row(&mut self, area: Rect, kind: LineKind, number: u32, text: &str) {
        let (style, sigil) = match kind {
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

        // Tab stops are counted from the start of the line's own content, not
        // from the left edge of the screen. The gutter and the sigil shift every
        // row by the same amount, so including them would align tabs to the
        // buffer and leave the file's indentation looking nothing like it does in
        // an editor.
        let body = format!("{sigil}{}", printable(text));
        self.put(x, area.y, &body, room, style);
    }
}

fn width_of(text: &str) -> usize {
    Span::raw(text).width()
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
/// truncated-to-useless label I6 forbids.
fn elide_head(text: &str, room: usize) -> String {
    if width_of(text) <= room || room <= 1 {
        return text.to_owned();
    }

    let budget = room - 1;
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| width_of(&text[i..]) <= budget)
        .unwrap_or(text.len());

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
/// Columns are counted from the start of `text`, which is where the file counts
/// them from too.
fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '\t' => {
                let stop = TAB_STOP - (column % TAB_STOP);
                out.extend(std::iter::repeat_n(' ', stop));
                column += stop;
            }
            c if c.is_control() => {
                out.push(UNPRINTABLE);
                column += 1;
            }
            c if c.is_ascii() => {
                out.push(c);
                column += 1;
            }
            c => {
                out.push(c);
                // Only the non-ASCII tail pays for a width lookup, which keeps
                // the common line off the measuring path entirely.
                column += width_of(&text[i..i + c.len_utf8()]);
            }
        }
    }
    out
}
