//! Gates over the register this repository's code is written in.
//!
//! `SPEC.md` §7's rules are about what a gate measures. These are about what a
//! comment is for, and they exist because the answer had drifted: a comment
//! explains what the code cannot, and the record of a change belongs in the
//! commit message and the tracker, which already hold it.
//!
//! Every gate here is a **ratchet**: the ceiling is today's measurement and it
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

/// Comment lines per hundred lines of code, per file, as a ceiling that may
/// only fall.
///
/// Integer hundredths rather than a float, because this is the structural tier
/// and a ratio equal to its own ceiling must pass: `2.48 > 2.48` is true in
/// binary floating point often enough to have failed this gate on the commit
/// that introduced it.
///
/// Files absent from this table are unbounded, which is deliberate: a table
/// naming every file would need editing whenever one is added, and the ones
/// worth bounding are the ones that were furthest out when the ratchet landed.
const RATIO_CEILING: [(&str, u64); 9] = [
    ("vigia-core/src/change.rs", 192),
    ("vigia/src/input.rs", 240),
    ("vigia/src/app.rs", 246),
    ("vigia-core/src/history.rs", 233),
    ("vigia/src/render.rs", 194),
    ("vigia/src/glyphs.rs", 191),
    ("vigia/src/lib.rs", 185),
    ("vigia/src/config.rs", 163),
    ("vigia/src/view.rs", 159),
];

/// Comments carrying a date or the narrative of a change.
///
/// One number over the whole tree rather than per file, because the work that
/// lowers it moves file by file and a per-file table would be edited in every
/// commit of the pass rather than read.
///
/// The ceiling is what **this gate** counts, which is not what a hand probe
/// with a wider marker list counted: the first value written here was 1,807
/// from such a probe, and it left 161 of slack, so the gate stayed green
/// against a deliberate mutation. A ceiling above the measurement is a bound
/// nothing can reach.
///
/// **It is zero, so this is a wall rather than a ratchet from here on.** The
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
const DOCBLOCK_LINES_CEILING: usize = 70;

/// Comments citing a tracker issue or pull request, as a ceiling that may only
/// fall.
///
/// **Not zero, and that is why it is a ratchet where the one above is a wall.**
/// A citation splits by the state of what it cites, which no offline gate can
/// read: **89** of these name an issue that is still open, which is a live
/// forward pointer of exactly the kind `SPEC.md` §10's own bullets carry, and
/// **1,099** name one that is closed, which is the commit's history restated in
/// a file nobody reads it from. Only the second class is going.
const TRACKER_CEILING: usize = 1_246;

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
        let comments = lines.iter().filter(|l| is_comment(l)).count();
        let code = lines
            .iter()
            .filter(|l| !l.trim().is_empty() && !is_comment(l))
            .count();
        assert!(code > 0, "{name} has no code lines");

        let hundredths = (comments as u64 * 100) / code as u64;
        measured += 1;
        if hundredths > ceiling {
            breaches.push(format!(
                "  {name}: {comments} comment lines against {code} of code is \
                 {hundredths} per hundred, over the {ceiling} ceiling"
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
    // Each marker names a comment describing the **change** rather than the
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
            (1..=4).contains(&digits)
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

/// A date that says **when something changed**, rather than one that stamps a
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
