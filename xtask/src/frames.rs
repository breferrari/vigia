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
}

pub const SNAPSHOTS: &str = "crates/vigia/tests/snapshots";

pub const FRAMES: &[Frame] = &[
    Frame {
        snapshot: "notes__note_states_frame.snap",
        size: "80x40",
        rows: &[(0, 27), (39, 40)],
        file: "frame-note-states.svg",
        label: "The vigia pane with one changed file carrying four notes: one whose line has left the diff, drawn under the file's heading and marked gone; one the agent has read and answered, marked seen with its reply under it; one on the changed line, marked open; and one whose line was edited under it, marked changed.",
    },
    Frame {
        snapshot: "notes__note_box_frame.snap",
        size: "80x24",
        rows: &[(0, 17), (23, 24)],
        file: "frame-note-box.svg",
        label: "The vigia pane with the note box open under a changed line, titled with the file and line it is about, holding a half-typed sentence, with Enter sends and Esc cancels along its bottom edge.",
    },
    Frame {
        snapshot: "sheet__sheet_frame.snap",
        size: "100x34",
        rows: &[(0, 34)],
        file: "frame-sheet.svg",
        label: "The vigia pane with the gestures sheet open over it: every key in one column and every mouse gesture in the next, each beside what it does.",
    },
    Frame {
        snapshot: "menu__menu_frame.snap",
        size: "80x24",
        rows: &[(0, 24)],
        file: "frame-menu.svg",
        label: "The vigia pane with the config menu open over it: every on and off setting with where it stands, a row to save the settings and a row to reset them.",
    },
    Frame {
        snapshot: "render__rename_frame.snap",
        size: "100x14",
        rows: &[(0, 14)],
        file: "frame-rename.svg",
        label: "The vigia pane listing a renamed and edited file as its new path, an arrow and its old path, above its diff.",
    },
    Frame {
        snapshot: "render__the_staged_run_at_eighty_columns.snap",
        size: "80x12",
        rows: &[(0, 12)],
        file: "frame-staged.svg",
        label: "The vigia pane with staged changes shown: an unstaged run and a staged run, each under its own labelled rule, one file appearing in both.",
    },
    Frame {
        snapshot: "render__a_file_with_no_line_diff_says_why.snap",
        size: "60x8",
        rows: &[(0, 8)],
        file: "frame-no-diff.svg",
        label: "The vigia pane listing three files with no line diff, each saying why: a binary file, an unresolved conflict, and a rename with no edit.",
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
    let every = snapshot_rows(snapshot);
    let rows: Vec<&str> = frame
        .rows
        .iter()
        .flat_map(|&(start, end)| start..end)
        .map(|i| {
            every.get(i).map_or_else(
                || panic!("{} has no row {} for {}", frame.snapshot, i + 1, frame.file),
                String::as_str,
            )
        })
        .collect();
    assert!(
        rows.len() >= 2,
        "{} needs a header and a status row",
        frame.file
    );
    let cols = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let width = (16.0 + cols as f64 * CELL + 16.0).round() as usize;
    let body = (rows.len() - 2) * PITCH + 16;
    let height = 8 + TITLE + body + 30 + 8;
    let (w, h) = (width - 16, height - 16);
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
    let _ = writeln!(
        out,
        "  <rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{h}\" rx=\"12\" fill=\"#0d1117\"/>\n  <g clip-path=\"url(#win)\">\n    <rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{TITLE}\" fill=\"#161b22\"/>\n    <circle cx=\"34\" cy=\"31\" r=\"6\" fill=\"#f85149\"/>\n    <circle cx=\"54\" cy=\"31\" r=\"6\" fill=\"#e3b341\"/>\n    <circle cx=\"74\" cy=\"31\" r=\"6\" fill=\"#3fb950\"/>\n    <rect x=\"8\" y=\"{status}\" width=\"{w}\" height=\"30\" fill=\"#161b22\"/>\n    <line x1=\"8\" y1=\"{status}\" x2=\"{}\" y2=\"{status}\" stroke=\"#21262d\" stroke-width=\"1\"/>",
        8 + w
    );
    let last = rows.len() - 1;
    for (k, row) in rows.iter().enumerate() {
        let tones = tones(row, k == 0, k == last);
        let chars: Vec<char> = row.chars().collect();
        let (x, y, inner) = if k == 0 {
            (96, 36, header(&chars, &tones))
        } else if k == last {
            (16, status + 20, spans_of(&chars, &tones, 0, chars.len()))
        } else {
            (
                16,
                8 + TITLE + 8 + (k - 1) * PITCH + 14,
                spans_of(&chars, &tones, 0, chars.len()),
            )
        };
        let _ = writeln!(
            out,
            "    <text class=\"m row\" x=\"{x}\" y=\"{y}\" font-size=\"13.5\" xml:space=\"preserve\">{inner}</text>"
        );
    }
    let _ = writeln!(
        out,
        "  </g>\n  <rect x=\"8.5\" y=\"8.5\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"none\" stroke=\"#30363d\"/>\n</svg>",
        w - 1,
        h - 1
    );
    out
}

/// The header beside the window controls, its right half at its real column.
fn header(chars: &[char], tones: &[&'static str]) -> String {
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
    match gap {
        Some((end, start)) => format!(
            "{} <tspan x=\"{:.1}\" class=\"{}\">{}</tspan>",
            spans_of(chars, tones, lead, end),
            16.0 + start as f64 * CELL,
            tones[start],
            escape(&chars[start..].iter().collect::<String>())
        ),
        None => spans_of(chars, tones, lead, chars.len()),
    }
}

/// `chars[from..to]` as one tspan per run of a tone.
fn spans_of(chars: &[char], tones: &[&'static str], from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut i = from;
    while i < to {
        let mut j = i;
        while j < to && tones[j] == tones[i] {
            j += 1;
        }
        let text: String = chars[i..j].iter().collect();
        let _ = write!(
            out,
            "<tspan class=\"{}\">{}</tspan>",
            tones[i],
            escape(&text)
        );
        i = j;
    }
    out
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
