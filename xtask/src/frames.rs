//! The frame pictures under `assets/frame-*.svg`, drawn from insta snapshots in
//! the visual language of `assets/preview.svg`.
//!
//! Shared by `cargo run -p xtask --bin frames`, which writes the pictures, and by
//! `crates/vigia/tests/package.rs::frame_svgs_match`, which reads them back, so
//! the two cannot disagree about what a row is. Standard library only.

use std::fmt::Write as _;

/// One picture: the snapshot it draws, the size it was rendered at, its rows
/// as zero-based `(start, end)` spans with `end` excluded, the file it is
/// written to, and what it shows.
pub struct Frame {
    pub snapshot: &'static str,
    pub size: &'static str,
    pub rows: &'static [(usize, usize)],
    pub file: &'static str,
    pub label: &'static str,
    /// Names drawn left of the window, each with a leader to its row.
    pub labels: &'static [Label],
}

/// A name for one drawn row, and text that row must contain.
pub struct Label {
    /// The row's index among the frame's drawn rows.
    pub row: usize,
    pub name: &'static str,
    pub marker: &'static str,
}

pub const SNAPSHOTS: &str = "crates/vigia/tests/snapshots";

pub const FRAMES: &[Frame] = &[
    Frame {
        snapshot: "render__glance_frame.snap",
        size: "80x16",
        rows: &[(0, 16)],
        file: "frame-glance.svg",
        label: "The vigia pane, labelled down its left side: the header naming the worktree and how many files changed; the list of changed files with their status letters, paths, sparklines and line counts, the newest marked by a pulse, and heat strips showing where in each file the changes sit; a rule; the diff of the file being read, with its heading, hunk and lines; another rule; and the status bar with key hints, frame time, memory and position.",
        labels: &[
            Label {
                row: 0,
                name: "header",
                marker: "changed",
            },
            Label {
                row: 2,
                name: "list",
                marker: "▸",
            },
            Label {
                row: 5,
                name: "rule",
                marker: "─",
            },
            Label {
                row: 6,
                name: "diff",
                marker: "watch.rs",
            },
            Label {
                row: 14,
                name: "rule",
                marker: "─",
            },
            Label {
                row: 15,
                name: "status",
                marker: "q quit",
            },
        ],
    },
    Frame {
        snapshot: "notes__note_states_frame.snap",
        size: "80x40",
        rows: &[(0, 27), (39, 40)],
        file: "frame-note-states.svg",
        label: "The vigia pane with one changed file carrying four notes: one whose line has left the diff, drawn under the file's heading and marked gone; one the agent has read and answered, marked seen with its reply under it; one on the changed line, marked open; and one whose line was edited under it, marked changed.",
        labels: &[],
    },
    Frame {
        snapshot: "notes__note_box_frame.snap",
        size: "80x24",
        rows: &[(0, 17), (23, 24)],
        file: "frame-note-box.svg",
        label: "The vigia pane with the note box open under a changed line, titled with the file and line it is about, holding a half-typed sentence, with Enter sends and Esc cancels along its bottom edge.",
        labels: &[],
    },
    Frame {
        snapshot: "sheet__sheet_frame.snap",
        size: "100x34",
        rows: &[(0, 34)],
        file: "frame-sheet.svg",
        label: "The vigia pane with the gestures sheet open over it: every key in one column and every mouse gesture in the next, each beside what it does.",
        labels: &[],
    },
    Frame {
        snapshot: "menu__menu_frame.snap",
        size: "80x24",
        rows: &[(0, 24)],
        file: "frame-menu.svg",
        label: "The vigia pane with the config menu open over it: every on and off setting with where it stands, a row to save the settings and a row to reset them.",
        labels: &[],
    },
    Frame {
        snapshot: "render__rename_frame.snap",
        size: "100x14",
        rows: &[(0, 14)],
        file: "frame-rename.svg",
        label: "The vigia pane listing a renamed and edited file as its new path, an arrow and its old path, above its diff.",
        labels: &[],
    },
    Frame {
        snapshot: "render__the_staged_run_at_eighty_columns.snap",
        size: "80x12",
        rows: &[(0, 12)],
        file: "frame-staged.svg",
        label: "The vigia pane with staged changes shown: an unstaged run and a staged run, each under its own labelled rule, one file appearing in both.",
        labels: &[],
    },
    Frame {
        snapshot: "render__a_file_with_no_line_diff_says_why.snap",
        size: "60x8",
        rows: &[(0, 8)],
        file: "frame-no-diff.svg",
        label: "The vigia pane listing three files with no line diff, each saying why: a binary file, an unresolved conflict, and a rename with no edit.",
        labels: &[],
    },
];

/// The rows of an insta snapshot, quotes and trailing spaces dropped.
pub fn snapshot_rows(snapshot: &str) -> Vec<String> {
    let body = snapshot.splitn(3, "---\n").nth(2).unwrap_or_default();
    body.trim_end_matches('\n')
        .lines()
        .map(|row| {
            // A quoted row, which a wide glyph follows with a `Hidden by` note.
            match row
                .strip_prefix('"')
                .and_then(|r| r.rfind('"').map(|end| &r[..end]))
            {
                Some(inner) => inner.trim_end().to_owned(),
                None => row.trim_end().to_owned(),
            }
        })
        .collect()
}

const CELL: f64 = 8.1;
const PITCH: usize = 20;
const TITLE: usize = 46;

/// The picture for `frame`, drawn from the snapshot's text.
pub fn draw(frame: &Frame, snapshot: &str) -> String {
    let drawn = drawn_rows(frame, snapshot);
    let rows: Vec<&str> = drawn.iter().map(String::as_str).collect();
    assert!(
        rows.len() >= 2,
        "{} needs a header and a status row",
        frame.file
    );
    // The content spans the leftmost to the rightmost column any row uses, with
    // the same inset on both sides of every frame, whatever margin the pane drew.
    let drawn = || rows.iter().filter(|r| !r.trim().is_empty() && !is_rule(r));
    let first = drawn()
        .map(|r| r.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    let last_col = drawn().map(|r| r.chars().count()).max().unwrap_or(0);
    let grid = Grid { first };
    let w = ((last_col - first) as f64 * CELL + 2.0 * INSET).ceil() as usize;
    // Room left of the window for the labels and their leaders, when it has any.
    let margin = frame
        .labels
        .iter()
        .map(|label| label.name.chars().count())
        .max()
        .map_or(0, |widest| (widest as f64 * CELL + 40.0).ceil() as usize);
    let width = margin + w + 16;
    let body = (rows.len() - 2) * PITCH + 16;
    let height = 8 + TITLE + body + 30 + 8;
    let h = height - 16;
    let status = 8 + TITLE + body;
    let spans = frame
        .rows
        .iter()
        .map(|&(start, end)| {
            if end - start == 1 {
                format!("{}", start + 1)
            } else {
                format!("{}-{end}", start + 1)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\" role=\"img\"\n     aria-label=\"{}\">",
        escape(frame.label)
    );
    let _ = writeln!(
        out,
        "  <!-- source: {SNAPSHOTS}/{} rows={spans}, rendered at {}. Drawn by the xtask frames binary; package.rs::frame_svgs_match holds it to the snapshot. -->",
        frame.snapshot, frame.size
    );
    let _ = writeln!(
        out,
        "  <defs>\n    <clipPath id=\"win\"><rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{h}\" rx=\"12\"/></clipPath>\n    <style>\n      .m {{ font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, \"DejaVu Sans Mono\", monospace; white-space: pre; }}\n      .fg {{ fill: #e6edf3 }} .dim {{ fill: #7d8590 }} .faint {{ fill: #6e7681 }}\n      .grn {{ fill: #3fb950 }} .red {{ fill: #f85149 }} .cyn {{ fill: #39c5cf }}\n      .con {{ fill: #e3b341 }}\n    </style>\n  </defs>"
    );
    if margin > 0 {
        // The labels sit on a dark card of their own, so they read on a light page.
        let _ = writeln!(
            out,
            "  <rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"16\" fill=\"#010409\" stroke=\"#30363d\"/>\n  <g transform=\"translate({margin},0)\">",
            width - 1,
            height - 1
        );
    }
    let _ = writeln!(
        out,
        "  <rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{h}\" rx=\"12\" fill=\"#0d1117\"/>\n  <g clip-path=\"url(#win)\">\n    <rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{TITLE}\" fill=\"#161b22\"/>\n    <circle cx=\"34\" cy=\"31\" r=\"6\" fill=\"#f85149\"/>\n    <circle cx=\"54\" cy=\"31\" r=\"6\" fill=\"#e3b341\"/>\n    <circle cx=\"74\" cy=\"31\" r=\"6\" fill=\"#3fb950\"/>\n    <rect x=\"8\" y=\"{status}\" width=\"{w}\" height=\"30\" fill=\"#161b22\"/>\n    <line x1=\"8\" y1=\"{status}\" x2=\"{}\" y2=\"{status}\" stroke=\"#21262d\" stroke-width=\"1\"/>",
        8 + w
    );
    let last = rows.len() - 1;
    let mut ys = Vec::with_capacity(rows.len());
    for (k, row) in rows.iter().enumerate() {
        let tones = tones(row, k == 0, k == last);
        let chars: Vec<char> = row.chars().collect();
        let (y, inner) = if k == 0 {
            (36, header(&grid, &chars, &tones))
        } else if k == last {
            (
                status + 20,
                runs(&grid, &chars, &tones, 0, chars.len(), 0.0),
            )
        } else {
            (
                8 + TITLE + 8 + (k - 1) * PITCH + 14,
                runs(&grid, &chars, &tones, 0, chars.len(), 0.0),
            )
        };
        ys.push(y);
        // A rule across the whole pane is drawn over the content's width only,
        // inside the pane's two-cell margins, the same on both sides.
        if is_rule(row) {
            let _ = writeln!(
                out,
                "    <line class=\"rule\" x1=\"{:.1}\" y1=\"{}\" x2=\"{:.1}\" y2=\"{}\" stroke=\"#30363d\" stroke-width=\"1\"/>",
                grid.x(first),
                y - 5,
                grid.x(last_col),
                y - 5
            );
            continue;
        }
        let _ = writeln!(
            out,
            "    <text class=\"m row\" y=\"{y}\" font-size=\"13.5\" xml:space=\"preserve\">{inner}</text>"
        );
    }
    let _ = writeln!(
        out,
        "  </g>\n  <rect x=\"8.5\" y=\"8.5\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"none\" stroke=\"#30363d\"/>",
        w - 1,
        h - 1
    );
    if margin > 0 {
        out.push_str("  </g>\n");
    }
    for label in frame.labels {
        let y = *ys.get(label.row).unwrap_or_else(|| {
            panic!(
                "{}'s {} label points past its {} rows",
                frame.file,
                label.name,
                ys.len()
            )
        });
        let name = label.name;
        let _ = writeln!(
            out,
            "  <text class=\"m fg\" x=\"{}\" y=\"{y}\" font-size=\"12.5\" text-anchor=\"end\">{}</text>\n  <line x1=\"{}\" y1=\"{mid}\" x2=\"{:.1}\" y2=\"{mid}\" stroke=\"#6e7681\" stroke-width=\"1\"/>",
            margin - 16,
            escape(name),
            margin - 10,
            margin as f64 + 4.0,
            mid = y - 4,
        );
    }
    out.push_str("</svg>\n");
    out
}

/// The snapshot's rows that `frame` draws, in order.
pub fn drawn_rows(frame: &Frame, snapshot: &str) -> Vec<String> {
    let every = snapshot_rows(snapshot);
    frame
        .rows
        .iter()
        .flat_map(|&(start, end)| start..end)
        .map(|i| {
            every.get(i).cloned().unwrap_or_else(|| {
                panic!("{} has no row {} for {}", frame.snapshot, i + 1, frame.file)
            })
        })
        .collect()
}

/// The space between the window's edge and the nearest drawn column.
const INSET: f64 = 2.0 * CELL;

/// Where the frame's columns sit in the window.
struct Grid {
    /// The leftmost column any row draws, which lands at the inset.
    first: usize,
}

impl Grid {
    /// Where cell `col` starts.
    fn x(&self, col: usize) -> f64 {
        8.0 + INSET + (col as f64 - self.first as f64) * CELL
    }
}

/// A row that is nothing but a rule across the pane.
fn is_rule(row: &str) -> bool {
    !row.is_empty() && row.chars().all(|c| c == '─')
}

/// Where the header's words start, beside the window controls, as in preview.svg.
const HEADER_X: f64 = 102.0;

/// The header beside the window controls, its right half at its real column.
fn header(grid: &Grid, chars: &[char], tones: &[&'static str]) -> String {
    let lead = chars.iter().take_while(|c| **c == ' ').count();
    let mut gap = None;
    let mut run = 0;
    for (i, c) in chars.iter().enumerate().skip(lead) {
        if *c == ' ' {
            run += 1;
        } else {
            if run >= 3 {
                gap = Some((i - run, i));
                break;
            }
            run = 0;
        }
    }
    let end = gap.map_or(chars.len(), |(end, _)| end);
    // The left half moves to HEADER_X, so every run in it moves by the same amount.
    let shift = HEADER_X - grid.x(lead);
    let left = runs(grid, chars, tones, lead, end, shift);
    match gap {
        Some((end, start)) => {
            assert!(
                HEADER_X + (end - lead) as f64 * CELL < grid.x(start),
                "the header's halves overlap"
            );
            format!(
                "{left} {}",
                runs(grid, chars, tones, start, chars.len(), 0.0)
            )
        }
        None => left,
    }
}

/// `chars[from..to]` as one tspan per run of a tone, each at its own column.
fn runs(
    grid: &Grid,
    chars: &[char],
    tones: &[&'static str],
    from: usize,
    to: usize,
    shift: f64,
) -> String {
    let mut out = String::new();
    let mut i = from;
    while i < to {
        // A run ends at a change of tone or at a gap of two spaces, so every
        // group of words starts at its own column and a wide font cannot carry
        // the end of a row past the window.
        let mut j = i;
        let gap = |at: usize| chars[at] == ' ' && chars.get(at + 1) == Some(&' ');
        while j < to && tones[j] == tones[i] && !(j > i && gap(j)) {
            j += 1;
        }
        // The gap itself goes with the run before it.
        while j < to && tones[j] == tones[i] && gap(j) {
            j += 1;
        }
        let text: String = chars[i..j].iter().collect();
        if !text.trim().is_empty() {
            let _ = write!(
                out,
                "<tspan x=\"{:.1}\" class=\"{}\">{}</tspan>",
                grid.x(i) + shift,
                tones[i],
                escape(&text)
            );
        }
        i = j;
    }
    out
}

/// The first text run that ends past the window's inner right edge, if any.
/// A run's end is its x plus a cell per character.
pub fn overflow(svg: &str) -> Option<String> {
    let width: f64 = svg
        .split("<rect x=\"8\" y=\"8\" width=\"")
        .nth(1)?
        .split('"')
        .next()?
        .parse()
        .ok()?;
    let edge = 8.0 + width;
    for run in svg.split("<tspan x=\"").skip(1) {
        let (x, rest) = run.split_once('"')?;
        let x: f64 = x.parse().ok()?;
        let text = rest.split_once('>')?.1.split("</tspan>").next()?;
        let count = text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .chars()
            .count();
        let end = x + count as f64 * CELL;
        if end > edge {
            return Some(format!(
                "{text:?} ends at {end:.1}, past the edge at {edge:.1}"
            ));
        }
    }
    None
}

/// One colour class per character of `row`.
fn tones(row: &str, first: bool, last: bool) -> Vec<&'static str> {
    let chars: Vec<char> = row.chars().collect();
    let n = chars.len();
    let mut tones = vec![if last { "dim" } else { "fg" }; n];
    // A diff row's tone stops at the first box edge drawn over it.
    let edge = chars.iter().position(|c| "│┌└├".contains(*c)).unwrap_or(n);
    let trimmed = row.trim_start();
    let number = chars
        .iter()
        .skip_while(|c| **c == ' ')
        .take_while(|c| c.is_ascii_digit())
        .count();
    let lead = chars.iter().take_while(|c| **c == ' ').count();
    let sigil = (number > 0).then(|| chars.get(lead + number + 1)).flatten();
    let tone = match sigil {
        Some('+') if chars.get(lead + number) == Some(&' ') => Some("grn"),
        Some('-') if chars.get(lead + number) == Some(&' ') => Some("red"),
        _ if trimmed.starts_with("@@") => Some("cyn"),
        _ => None,
    };
    if let Some(tone) = tone {
        tones[..edge].fill(tone);
    }
    // A diff row's line number is dim, as the gutter draws it.
    let gutter = matches!(chars.get(lead + number + 1), Some(' ' | '+' | '-'))
        && chars.get(lead + number) == Some(&' ')
        && chars.get(lead + number + 2) == Some(&' ');
    if number > 0 && gutter {
        tones[lead..lead + number].fill("dim");
    }
    for (i, c) in chars.iter().enumerate() {
        match c {
            '┌' | '┐' | '└' | '┘' | '│' | '─' | '├' | '┤' => tones[i] = "faint",
            '■' => tones[i] = "dim",
            '▁' | '▂' | '▃' | '▄' | '▅' | '▆' | '▇' | '█' | '▸' | '▾' | '↳' | '✕' | '▲' | '▼'
            | '●' | '▶' | '←' => tones[i] = "cyn",
            _ => {}
        }
    }
    // A run of three or more underscores is an empty sparkline, not text.
    let mut i = 0;
    while i < n {
        let run = chars[i..].iter().take_while(|c| **c == '_').count();
        if run >= 3 {
            tones[i..i + run].fill("faint");
        }
        i += run.max(1);
    }
    let words = words(&chars);
    // The counts at the end of a file row, a scrollbar glyph beside them or not.
    if !first && !last {
        let counted: Vec<&(usize, String)> = words
            .iter()
            .rev()
            .skip_while(|(_, word)| word.chars().all(|c| "▲▼█│".contains(c)))
            .take(2)
            .collect();
        if let [(b, minus), (a, plus)] = counted.as_slice()
            && plus.starts_with('+')
            && minus.starts_with('-')
            && plus[1..].chars().all(|c| c.is_ascii_digit())
            && minus[1..].chars().all(|c| c.is_ascii_digit())
        {
            tones[*a..a + plus.len()].fill("grn");
            tones[*b..b + minus.len()].fill("red");
        }
    }
    // A note's state word on its box edge, and a run's label on its rule.
    if chars.contains(&'─') {
        for (at, word) in &words {
            if ["open", "seen", "changed", "gone", "staged", "unstaged"].contains(&word.as_str()) {
                tones[*at..at + word.chars().count()].fill("con");
            }
        }
    }
    // A file row's kind letter.
    let list_lead = chars
        .iter()
        .take_while(|c| **c == ' ' || **c == '▸')
        .count();
    if chars.get(list_lead).is_some_and(|c| "MADRU".contains(*c))
        && chars.get(list_lead + 1) == Some(&' ')
    {
        tones[list_lead] = "dim";
    }
    tones
}

/// The words of a row with their character offsets.
fn words(chars: &[char]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] != ' ' {
            i += 1;
        }
        out.push((start, chars[start..i].iter().collect()));
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
