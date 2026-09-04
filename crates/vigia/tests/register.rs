//! Gates over the register this repository's code is written in.
//!
//! `SPEC.md` §7's rules are about what a gate measures. These are about what a
//! comment is for, and they exist because the answer had drifted: a comment
//! explains what the code cannot, and the record of a change belongs in the
//! commit message and the tracker, which already hold it.
//!
//! Every gate here is a ratchet: the ceiling is today's measurement and it
//! may only fall. A ratchet cannot fail on the commit that introduces it and
//! cannot be satisfied by adding, which is the property a wall does not have
//! when the work it gates is spread over many commits. Lowering one is how a
//! pass records itself; raising one is refused in the failure message, where
//! whoever is tempted will be standing.
//!
//! Structural tier, in the idiom of `reads.rs`: exact counts, no
//! `VIGIA_BUDGET_SLACK`, running in debug so every `cargo test --workspace`
//! answers.

use std::path::{Path, PathBuf};

/// Comment lines per ten thousand lines of code, per file, as a ceiling that
/// may only fall.
///
/// Integer rather than a float, because this is the structural tier and a ratio
/// equal to its own ceiling must pass: `2.48 > 2.48` is true in binary floating
/// point often enough to have failed this gate on the commit that introduced it.
///
/// Ten-thousandths rather than the hundredths this first counted in, because a
/// coarse unit is slack wearing a ratchet's clothes. At 354 comment lines
/// against 1,318 of code, `view.rs` measured 26 per hundred either way, so
/// thirteen more lines of prose could land before the number moved and a
/// deliberate mutation passed. One line moves this.
///
/// Files absent from this table are unbounded, which is deliberate: a table
/// naming every file would need editing whenever one is added, and the ones
/// worth bounding are the ones that were furthest out when the ratchet landed.
const RATIO_CEILING: [(&str, u64); 9] = [
    ("vigia-core/src/change.rs", 4705),
    ("vigia/src/input.rs", 4387),
    ("vigia/src/app.rs", 4165),
    ("vigia-core/src/history.rs", 3350),
    ("vigia/src/render.rs", 3314),
    ("vigia/src/glyphs.rs", 3947),
    ("vigia/src/lib.rs", 3575),
    ("vigia/src/config.rs", 4000),
    ("vigia/src/view.rs", 2610),
];

/// Comments carrying a date or the narrative of a change.
///
/// Counted per line, which is a blind spot worth naming: a marker split
/// across a line break is invisible to this, and rewrapping a paragraph is what
/// surfaces one. Closing it means joining a comment block before matching, and
/// the block-joining that would take is the same operation that flattened
/// thirty-one files' docblocks when it was tried for a different reason.
///
/// One number over the whole tree rather than per file, because the work that
/// lowers it moves file by file and a per-file table would be edited in every
/// commit of the pass rather than read.
///
/// The ceiling is what this gate counts, which is not what a hand probe
/// with a wider marker list counted: the first value written here was 1,807
/// from such a probe, and it left 161 of slack, so the gate stayed green
/// against a deliberate mutation. A ceiling above the measurement is a bound
/// nothing can reach.
///
/// It is zero, so this is a wall rather than a ratchet from here on. The
/// class is gone from the tree; the only direction left is back.
///
/// A tracker reference is counted separately by [`TRACKER_CEILING`], because the
/// two classes are at different stages and one ceiling for both would let this
/// one creep back as that one falls.
const SESSION_CONTEXT_CEILING: usize = 0;

/// The longest `///` run in the tree, in lines.
///
/// A docblock longer than the item under it means one of the two is wrong, and
/// this bounds the direction that can be measured without a parser.
///
/// `//!` is deliberately outside it. A module header is the one place RFC 505
/// asks for length — it documents a file rather than an item, so there is no
/// item for it to be longer than.
const DOCBLOCK_LINES_CEILING: usize = 31;

/// Comments citing a tracker issue or pull request, as a ceiling that may only
/// fall.
///
/// Not zero, and that is why it is a ratchet where the one above is a wall.
/// A citation splits by the state of what it cites, which no offline gate can
/// read: one of an open issue is a live forward pointer of exactly the kind
/// `SPEC.md` §10's own bullets carry, and one of a closed issue is the commit's
/// history restated in a file nobody reads it from. Every citation left in the
/// tree is of the first kind, so this cannot fall further without the tracker
/// moving first, and it must not rise.
const TRACKER_CEILING: usize = 9;

fn crates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every `.rs` file under `crates/`, sorted, so a failure names the same file
/// twice running.
fn sources() -> Vec<(String, Vec<String>)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    let root = crates_root();
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let body = std::fs::read_to_string(&path).ok()?;
            let name = path
                .strip_prefix(&root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some((name, body.lines().map(str::to_owned).collect()))
        })
        .collect()
}

fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//")
}

/// Comment lines this repository chose to write, which is what the ratio bounds.
///
/// `# Errors`, `# Panics` and `# Safety` sections are excluded, capped at
/// [`COMPELLED_LINES`] each. They are not discretionary: `clippy`'s
/// `missing_errors_doc`, `missing_panics_doc`, `missing_safety_doc` and
/// `undocumented_unsafe_blocks` fail the build without them, and CI runs
/// `-D warnings`. Counting them here put two gates in opposition, and the only
/// way a session could satisfy both was to delete an ordinary comment that
/// explained something real in order to pay for a section a lint demanded. That
/// is the ratchet buying worse code, which is the opposite of its job — it
/// exists to refuse session narrative and essay, not API documentation.
///
/// **The cap is what keeps this from being a hole.** Everything past
/// [`COMPELLED_LINES`] in such a section counts normally, so an essay cannot be
/// smuggled in under an `# Errors` heading. Adopted 2026-08-28 with the C-FAILURE
/// lints; a first spelling with no cap was rejected for exactly that reason.
fn discretionary(lines: &[String]) -> usize {
    let mut count = 0;
    let mut compelled = 0;
    for line in lines {
        if !is_comment(line) {
            continue;
        }
        let body = line.trim_start().trim_start_matches('/').trim();
        if matches!(body, "# Errors" | "# Panics" | "# Safety") {
            compelled = COMPELLED_LINES;
            continue;
        }
        if compelled > 0 {
            compelled -= 1;
            continue;
        }
        count += 1;
    }
    count
}

/// Lines a compelled documentation section may spend before it counts like any
/// other prose. Six is the longest of the thirty-two written when the C-FAILURE
/// lints were adopted: a heading, a blank, and four lines of text.
const COMPELLED_LINES: usize = 6;

/// The gate refuses to run against an empty tree, because a ceiling nothing
/// reached is satisfied the way an empty room satisfies a fire code.
fn non_vacuous(files: &[(String, Vec<String>)]) {
    assert!(
        files.len() > 50,
        "only {} source files found under {}, so this gate is passing on \
         nothing",
        files.len(),
        crates_root().display()
    );
}

#[test]
fn the_comment_to_code_ratio_only_falls() {
    let files = sources();
    non_vacuous(&files);

    let mut breaches = Vec::new();
    let mut measured = 0;
    for (name, ceiling) in RATIO_CEILING {
        let Some((_, lines)) = files.iter().find(|(n, _)| n == name) else {
            panic!("{name} is in the ceiling table and not in the tree");
        };
        let comments = discretionary(lines);
        let code = lines
            .iter()
            .filter(|l| !l.trim().is_empty() && !is_comment(l))
            .count();
        assert!(code > 0, "{name} has no code lines");

        let ten_thousandths = (comments as u64 * 10_000) / code as u64;
        measured += 1;
        if ten_thousandths > ceiling {
            breaches.push(format!(
                "  {name}: {comments} comment lines against {code} of code is \
                 {}.{:02} per hundred ({ten_thousandths}), over the {ceiling} \
                 ceiling ({}.{:02})",
                ten_thousandths / 100,
                ten_thousandths % 100,
                ceiling / 100,
                ceiling % 100
            ));
        }
    }

    assert_eq!(
        measured,
        RATIO_CEILING.len(),
        "not every ceiling was measured"
    );
    assert!(
        breaches.is_empty(),
        "the comment ratio rose in {} file(s). Lower the ceiling when a pass \
         lowers the ratio; do not raise it to admit prose:\n{}",
        breaches.len(),
        breaches.join("\n")
    );
}

#[test]
fn a_comment_carries_no_record_of_its_own_change() {
    let files = sources();
    non_vacuous(&files);

    // A reference to the contract is legitimate and is deliberately not
    // matched: a pointer to a section, a ruling id or an invariant id says
    // "this code implements that rule", which is what a comment is for.
    //
    // Each marker names a comment describing the change rather than the
    // code. Two phrasings that look like markers and are not were tried and
    // dropped, because they made the gate measure a proxy: "the reader asked"
    // is how `Action`'s own docblock describes input, and a bare "round"
    // matches "round a number to zero" and "a round trip".
    let markers: [&str; 12] = [
        "first draft",
        "earlier draft",
        "an earlier version",
        "audit round",
        "round one",
        "round two",
        "round three",
        "round four",
        "used to",
        "previously",
        "superseded",
        "this session",
    ];

    let mut hits = 0;
    let mut worst: Vec<(usize, String)> = Vec::new();
    for (name, lines) in &files {
        if name == "vigia/tests/register.rs" {
            continue;
        }
        let mut here = 0;
        for line in lines.iter().filter(|l| is_comment(l)) {
            let lower = line.to_lowercase();
            if dates_a_change(&lower) || markers.iter().any(|marker| contains_word(&lower, marker))
            {
                here += 1;
            }
        }
        hits += here;
        if here > 0 {
            worst.push((here, name.clone()));
        }
    }

    worst.sort_by_key(|(count, _)| std::cmp::Reverse(*count));
    let top: Vec<String> = worst
        .iter()
        .take(6)
        .map(|(n, name)| format!("  {name}: {n}"))
        .collect();

    assert_eq!(
        hits,
        SESSION_CONTEXT_CEILING,
        "{hits} comment lines carry a date or the narrative of a change, over \
         the {SESSION_CONTEXT_CEILING} ceiling. Those belong in the commit \
         message and the tracker. Worst files:
{}",
        top.join("\n")
    );
}

/// A `#` followed by one to four digits, which is how this repository spells a
/// tracker reference. Deliberately not `I4` or `B19`, which name the contract.
fn has_issue_number(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.iter().enumerate().any(|(i, b)| {
        *b == b'#' && {
            let digits = bytes[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            // The digit run has to end the token. A hex colour is `#` and digits
            // too: `#3fb950` is `#3` followed by a letter, and counting it made
            // this gate report citations in a palette file that cites nothing.
            let ends = bytes
                .get(i + 1 + digits)
                .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_');
            (1..=4).contains(&digits) && ends
        }
    })
}

/// Whether `needle` appears in `haystack` on both its own word boundaries.
///
/// `contains` alone is what a first spelling of these markers used, and it is
/// wrong in the quiet direction: `used to` is inside `refused to` and `round
/// one` is inside `around one`, so five ordinary sentences counted as records
/// of a change and the ceiling would have been set above them.
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(at, _)| {
        let before = haystack[..at].chars().next_back();
        let after = haystack[at + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A date that says when something changed, rather than one that stamps a
/// measurement.
///
/// The distinction is the gate's whole subject, and a bare date test does not
/// make it: `Probed through GlyphTypeface, 2026-08-17` and `288 samples from
/// 2026-08-05 22:22` are the provenance a measured figure is supposed to carry,
/// and this repository's own rules ask for them. `corrected 2026-08-16` and
/// `ruled 2026-08-15` are the commit message's content in the wrong file.
fn dates_a_change(lower: &str) -> bool {
    const VERBS: [&str; 9] = [
        "ruled",
        "corrected",
        "reversed",
        "amended",
        "landed",
        "shipped",
        "overruled",
        "expired",
        "until",
    ];
    has_iso_date(lower) && VERBS.iter().any(|verb| contains_word(lower, verb))
}

/// `YYYY-MM-DD` in this century.
fn has_iso_date(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.windows(10).any(|w| {
        w[0] == b'2'
            && w[1] == b'0'
            && w[2..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[7] == b'-'
            && w[8..10].iter().all(u8::is_ascii_digit)
    })
}

#[test]
fn no_docblock_runs_longer_than_the_ceiling() {
    let files = sources();
    non_vacuous(&files);

    let mut longest = 0;
    let mut where_at = String::new();
    for (name, lines) in &files {
        if name == "vigia/tests/register.rs" {
            continue;
        }
        let mut run = 0;
        let mut started = 0;
        for (number, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") {
                if run == 0 {
                    started = number + 1;
                }
                run += 1;
                if run > longest {
                    longest = run;
                    where_at = format!("{name}:{started}");
                }
            } else {
                run = 0;
            }
        }
    }

    assert!(
        longest <= DOCBLOCK_LINES_CEILING,
        "the longest doc comment is {longest} lines at {where_at}, over the \
         {DOCBLOCK_LINES_CEILING} ceiling. A docblock longer than the item it \
         documents means one of the two is wrong"
    );
}

#[test]
fn a_comment_cites_no_more_of_the_tracker_than_it_did() {
    let files = sources();
    non_vacuous(&files);

    let mut hits = 0;
    let mut worst: Vec<(usize, String)> = Vec::new();
    for (name, lines) in &files {
        if name == "vigia/tests/register.rs" {
            continue;
        }
        let here = lines
            .iter()
            .filter(|line| is_comment(line))
            .filter(|line| line.contains("github.com/breferrari/vigia") || has_issue_number(line))
            .count();
        hits += here;
        if here > 0 {
            worst.push((here, name.clone()));
        }
    }

    worst.sort_by_key(|(count, _)| std::cmp::Reverse(*count));
    let top: Vec<String> = worst
        .iter()
        .take(6)
        .map(|(n, name)| format!("  {name}: {n}"))
        .collect();

    assert!(
        hits <= TRACKER_CEILING,
        "{hits} comment lines cite the tracker, over the {TRACKER_CEILING} \
         ceiling. A citation of an issue that is still open is a live forward \
         pointer and is fine; one of a closed issue is the commit's history in \
         a file nobody reads it from. Worst files:\n{}",
        top.join("\n")
    );
}

/// Every markdown file in the repository, so the gate reads what a session reads.
///
/// Walked rather than asked of `git`, which is [`sources`]'s own idiom: a gate
/// that shells out cannot run where the shell is absent, and this one has no
/// reason to know what is tracked.
fn markdown() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") && !name.contains("NOTICE") {
                out.push(path);
            }
        }
    }

    let root = crates_root().join("..");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let body = std::fs::read_to_string(&path).ok()?;
            let name = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            Some((name, body))
        })
        .collect()
}

/// No prose paragraph in a tracked markdown file spans more than one line.
///
/// A wall rather than a ratchet, because the tree is clean and the class is
/// mechanical: GitHub renders a single newline inside a paragraph as a real
/// line break, so a body wrapped at eighty arrives at its reader broken
/// mid-sentence. 811 forced breaks across sixteen of this repository's own pull
/// requests before anyone measured it.
///
/// **Gated rather than written down, because it had been written down twice
/// and lost twice.** An instruction cannot beat a corpus: these documents held
/// 7,351 hard-wrapped lines, and a session reads them before it writes
/// anything. Removing the examples is what makes the rule hold.
///
/// Code comments are deliberately out of scope. Nothing renders them, so a
/// break there corrupts nothing, and they sit beside code held near a hundred
/// columns where one long line reads worse.
///
/// Structure is not prose and is never counted: YAML frontmatter, fenced code,
/// tables, list items, headings, blockquotes and thematic breaks.
/// Every row of a markdown table has the cells its header declared.
///
/// A row one cell short renders as a table with a hole in it, and nothing else
/// here looks: the prose-width gate reads paragraphs, and a table row is not
/// one. Two were found by hand in one pass, one of them written by that pass.
#[test]
fn no_table_row_is_missing_a_cell() {
    let mut broken = Vec::new();
    let files = markdown();
    assert!(
        !files.is_empty(),
        "no markdown was read, so this proves nothing"
    );

    for (name, text) in &files {
        // A width per table rather than per file: the documents carry several,
        // and a run of rows ends at the first line that is not one.
        let mut want: Option<usize> = None;
        let mut fenced = false;
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                want = None;
                continue;
            }
            if fenced || !line.starts_with('|') {
                want = None;
                continue;
            }
            // An escaped pipe is a character in a cell, not a cell boundary.
            let cells = line.replace(r"\|", "").matches('|').count();
            match want {
                None => want = Some(cells),
                // The rule under the header, whose cells are dashes.
                Some(_) if line.trim_matches(['|', '-', ':', ' ']).is_empty() => {}
                Some(width) if cells != width => {
                    broken.push(format!(
                        "  {name}:{} has {cells} cells, its table has {width}",
                        n + 1
                    ));
                }
                Some(_) => {}
            }
        }
    }

    assert!(
        broken.is_empty(),
        "a table row does not match its header, so the document renders with a hole in it:
{}",
        broken.join(
            "
"
        )
    );
}

#[test]
fn no_prose_paragraph_is_hard_wrapped() {
    let files = markdown();
    assert!(
        files.len() > 5,
        "only {} markdown file(s) found, so this gate is passing on nothing",
        files.len()
    );

    let structural = |line: &str| {
        let t = line.trim_start();
        t.is_empty()
            || t.starts_with('|')
            || t.starts_with('#')
            || t.starts_with('>')
            || t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("+ ")
            || t.starts_with("---")
            || t.starts_with("===")
            // A raw HTML block is layout, and joining `<table>` onto `<tr>` is
            // the same class of damage as joining a form's fields.
            || t.starts_with('<')
            || t.chars().next().is_some_and(|c| c.is_ascii_digit())
                && t.contains(". ")
                && t.split_once(". ").is_some_and(|(n, _)| {
                    n.chars().all(|c| c.is_ascii_digit())
                })
    };

    let mut breaches = Vec::new();
    for (name, body) in &files {
        // Frontmatter is `key: value` structure. Joining it once made a skill's
        // name and description one field.
        let body = body.strip_prefix("---\n").map_or(body.as_str(), |rest| {
            rest.find("\n---\n")
                .map_or(body.as_str(), |at| &rest[at + 5..])
        });
        let mut fence = false;
        // An HTML comment is not prose either. The issue template's `<!-- -->`
        // block holds the fields a reporter fills in one per line, and joining
        // them made `Terminal and version: OS: vigia --version:` one line.
        let mut html = false;
        let mut prose = 0usize;
        for (n, line) in body.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fence = !fence;
                prose = 0;
                continue;
            }
            if line.contains("<!--") {
                html = true;
            }
            let closing = html && line.contains("-->");
            if html || closing {
                if closing {
                    html = false;
                }
                prose = 0;
                continue;
            }
            if fence || structural(line) {
                prose = 0;
                continue;
            }
            prose += 1;
            if prose == 2 {
                breaches.push(format!("  {name}:{}", n + 1));
            }
        }
    }

    assert!(
        breaches.is_empty(),
        "{} paragraph(s) are hard wrapped. One paragraph is one line: a newline \
         inside one renders as a break the author did not write. Join them; do \
         not reflow to a wider column:\n{}",
        breaches.len(),
        breaches.join("\n")
    );
}
