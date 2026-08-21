//! Compiling grammars ahead of the reader, and what that is and is not worth.
//!
//! `syntect` defers a grammar's patterns to `fancy_regex` on first use, so the
//! first parse under one costs 74-362ms where loading the whole dump costs
//! 318µs. `Highlighter::warm_ahead` pays that where nothing is waiting on it.
//!
//! **The claim about warmth is deliberately weak, and the weakness is a
//! finding.** There is no such thing as a warm grammar: compilation is per
//! *pattern*, so warming on a small sample leaves a different file of the same
//! language still paying. Measured in release over a 2.5KB synthetic sibling:
//! `.js` 80.49ms above floor, `.html` 40.10ms, `.cpp` 37.55ms. So what these
//! gates assert about warmth is that it helps **the content it warmed on**,
//! which is the only part that is true.
//!
//! **The claim about coldness is exact, and it is the one
//! [#129](https://github.com/breferrari/vigia/issues/129) is built on.** A
//! grammar nothing has parsed under has every pattern uncompiled, so the frame
//! that first meets it pays the whole cliff: measured at frame scale, one
//! twenty-four line screenful, **123.98ms cold against a 2.40ms floor** under
//! Rust and 694.75ms against 90.65ms under Markdown. The cliff is flat in
//! content size, which is what says it is a compile rather than a parse: a
//! 594-byte screenful costs 631.46ms cold and 0.97ms warm.
//!
//! So the frame path declines to parse under a grammar the warmer has not run
//! over, draws it plain, and says what it wants. The gates below cover both
//! halves, the way out when the file it asked for has gone, and the population
//! sweep that keeps the case a reader is actually watching from flickering.

mod support;

use std::time::Duration;

use support::{Scratch, absolute_gates_apply, budget, exclusively_timed, time};
use vigia_core::{
    Class, Frame, Highlighter, WARM_BYTES, WARM_FILES, WARM_LEADING, WARM_PER_GRAMMAR, WARM_TOTAL,
    leading_paths,
};

#[test]
fn a_path_with_no_grammar_is_skipped_before_it_is_read() {
    // The same answer `syntax_for` gives the frame path for a file type nothing
    // recognises, and it has to be reachable without panicking because the
    // warmer walks whatever status named. Skipped **before** the read, which is
    // what makes an unknown file type free rather than merely harmless.
    let scratch = Scratch::large_diff("warm-no-grammar", 1, 4);
    std::fs::write(scratch.path_of("data.unknownext"), "some bytes\n").expect("write");
    let highlighter = Highlighter::new();

    let warmed = highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["data.unknownext".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, 0,
        "the warmer counted a file `syntect` has no grammar for, so it read \
         one it could not have compiled anything from"
    );
}

#[test]
fn an_empty_changed_set_warms_nothing() {
    let highlighter = Highlighter::new();
    let handle = highlighter.warm_ahead(std::path::PathBuf::from("."), Vec::new(), None);
    assert_eq!(handle.join().expect("the warmer thread").warmed, 0);
}

#[test]
fn many_files_of_one_language_warm_only_a_few() {
    // **The bound that decides what the warmer costs.** Compiling a grammar is
    // one file's work and every later file of the same language buys only a
    // decaying residual, so a changed set of eighty-four Rust files must not be
    // eighty-four parses. Measured before this cap existed: sixty-four files was
    // **1.053s** of held core, about 96% of it re-parsing an already-compiled
    // grammar next to the frame path.
    let files = WARM_FILES + 20;
    let scratch = Scratch::large_diff("warm-one-language", files, 4);
    let highlighter = Highlighter::new();
    let paths: Vec<String> = (0..files).map(|n| format!("src/mod_{n}.rs")).collect();

    let warmed = highlighter
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "the warmer parsed {warmed} of {files} files that all share one \
         grammar, over the {WARM_PER_GRAMMAR} it is allowed"
    );
}

/// And the reads, which is the half of the cap nothing asserted.
///
/// The parse count above is identical whether the cap is consulted before the
/// read or after it, so a run that opened eighty-four files to warm three
/// looked exactly like one that opened three. It happened: the cap moved after
/// the read while the grammar being charged was corrected, every test stayed
/// green, and the I/O amplification the cap exists to prevent was back.
///
/// The bound is `WARM_PER_GRAMMAR` exactly for a single-language set, because
/// nothing here is content-sensitive: `SPEC.md` §6's `.ts` rule is the one case
/// that has to pay a read to learn its grammar, and a `.rs` file never does.
#[test]
fn many_files_of_one_language_are_not_even_read() {
    let files = WARM_FILES + 20;
    let scratch = Scratch::large_diff("warm-one-language-reads", files, 4);
    let highlighter = Highlighter::new();
    let paths: Vec<String> = (0..files).map(|n| format!("src/mod_{n}.rs")).collect();

    let report = highlighter
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        report.read, WARM_PER_GRAMMAR,
        "the warmer opened {} of {files} files that all share one grammar, so \
         the per-grammar cap is being checked after the read rather than \
         before it and saves the parse without saving the I/O",
        report.read
    );
    assert_eq!(
        report.warmed, report.read,
        "every file this run opened should also have been parsed ({} read, {} \
         warmed); a gap means bytes were pulled for a file the cap then threw \
         away",
        report.read, report.warmed
    );
}

#[test]
fn the_path_cap_stops_the_walk_before_a_language_it_has_not_reached() {
    // The outer bound, and it needs a fixture that can *see* it: with every path
    // sharing one grammar the per-grammar cap binds first and a missing path cap
    // would be invisible. So the run is `WARM_FILES` Rust files, which contribute
    // exactly `WARM_PER_GRAMMAR` parses, followed by a Markdown file that is
    // reachable only if the walk runs past its cap.
    let scratch = Scratch::large_diff("warm-path-cap", WARM_FILES, 4);
    std::fs::write(scratch.path_of("README.md"), "# heading\n\ntext\n").expect("write");
    let highlighter = Highlighter::new();

    let mut paths: Vec<String> = (0..WARM_FILES).map(|n| format!("src/mod_{n}.rs")).collect();
    paths.push("README.md".to_owned());

    let warmed = highlighter
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "the warmer parsed {warmed} files, so it walked past the \
         {WARM_FILES} paths it is allowed and reached the Markdown file behind \
         them"
    );
}

#[test]
fn a_path_that_is_not_there_is_skipped_rather_than_fatal() {
    // A file can vanish between status naming it and this thread reaching it,
    // which is ordinary beside an agent. The run has to continue: a warmer that
    // gave up on the first missing path would stop warming exactly when the
    // other pane is busiest. A missing path also must not spend the per-grammar
    // budget, since nothing was compiled from it.
    let scratch = Scratch::large_diff("warm-missing", 2, 4);
    let highlighter = Highlighter::new();

    let warmed = highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec![
                "src/gone.rs".to_owned(),
                "src/mod_0.rs".to_owned(),
                "src/also_gone.rs".to_owned(),
                "src/mod_1.rs".to_owned(),
            ],
            None,
        )
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, 2,
        "the warmer counted {warmed} files where two of the four exist, so a \
         missing path is either fatal or is spending the per-grammar budget"
    );
}

#[test]
fn warming_moves_a_grammars_compile_off_the_parse_that_follows_it() {
    // The only claim `warm` makes, and the one that has to be measured rather
    // than asserted structurally: there is no observable state to check, because
    // the compiled patterns live in `syntect`'s own `OnceCell`s.
    //
    // One highlighter, deliberately: the second timing has to meet the patterns
    // the first compiled, and a fresh `SyntaxSet` would be cold again and measure
    // nothing.
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    // Taken for the reason `crates/vigia/tests/budgets.rs` takes it: this binary
    // also runs `many_files_of_one_language_warm_only_a_few`, which builds an
    // eighty-four file fixture, and a polyglot gate that writes twenty-five files
    // and compiles up to twelve grammars on a thread. A wall clock measured
    // beside either is measuring the neighbour.
    //
    // **It works only because they take it too.** A mutual-exclusion protocol one
    // participant observes is not one, and the two heavy gates hold it across
    // their fixture building for exactly that reason.
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("warm-cost", 1, 40);
    let root = scratch.root().to_path_buf();
    let paths = vec!["src/mod_0.rs".to_owned()];

    let cold = Highlighter::new();
    let cold_parse = time(|| {
        let _ = cold
            .warm_ahead(root.clone(), paths.clone(), None)
            .join()
            .expect("the warmer thread");
    });

    // The same highlighter, so the second run meets its own compiled patterns.
    let after = time(|| {
        cold.warm_ahead(root.clone(), paths.clone(), None)
            .join()
            .expect("the warmer thread");
    });

    eprintln!("note: a first parse is {cold_parse:?}, the same parse warmed is {after:?}");

    // An order of magnitude, not a ratio tuned to this machine. The measured
    // gap is ~93ms against ~0.5ms, so ten times is loose enough to survive a
    // slow runner and tight enough that a `warm` which quietly stopped
    // compiling anything would fail it.
    assert!(
        after * 10 < cold_parse,
        "a warmed parse took {after:?} against {cold_parse:?} cold, so warming \
         is not moving the grammar compile anywhere"
    );
}

#[test]
fn the_per_grammar_cap_is_per_grammar_and_not_one_shared_counter() {
    // **The bound the docs call "the bound that matters" had no test saying it
    // was per grammar at all.** Replacing the map with a single counter left
    // every other gate here green, because each of their fixtures is one
    // language. `SPEC.md` §7's ASCII-fixture rule one axis over: a
    // single-language fixture cannot tell a per-grammar cap from a global one.
    let scratch = Scratch::large_diff("warm-two-languages", 1, 4);
    let mut paths = Vec::new();
    for n in 0..WARM_PER_GRAMMAR + 1 {
        for (ext, body) in [("rs", "fn a() {}\n"), ("md", "# h\n\ntext\n")] {
            let name = format!("pair_{n}.{ext}");
            std::fs::write(scratch.path_of(&name), body).expect("write");
            paths.push(name);
        }
    }

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed,
        2 * WARM_PER_GRAMMAR,
        "two languages offering {WARM_PER_GRAMMAR} files each warmed {warmed}, \
         not {}, so the cap is one shared counter rather than one per grammar",
        2 * WARM_PER_GRAMMAR
    );
}

#[test]
fn a_polyglot_changed_set_is_bounded_in_total_and_not_only_per_language() {
    // The per-grammar cap gives a polyglot tree as many budgets as it has
    // languages. Measured before `WARM_TOTAL` existed: fifty extensions warmed
    // forty-three files in 3.93s of held core, against the 1.053s worst case the
    // per-grammar cap was reasoned about with.
    let scratch = Scratch::large_diff("warm-polyglot", 1, 4);
    let exts = [
        "rs", "md", "py", "js", "go", "toml", "json", "yaml", "c", "cpp", "h", "rb", "sh", "php",
        "java", "cs", "css", "html", "xml", "sql", "lua", "swift", "hs", "clj", "erl",
    ];
    let mut paths = Vec::new();
    for (n, ext) in exts.iter().enumerate() {
        let name = format!("file_{n}.{ext}");
        std::fs::write(scratch.path_of(&name), "text\nmore text\n").expect("write");
        paths.push(name);
    }
    assert!(
        paths.len() > WARM_TOTAL,
        "the fixture offers {} files against a total cap of {WARM_TOTAL}, so it \
         cannot reach the bound it exists to test",
        paths.len()
    );

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread")
        .warmed;

    // `assert_eq!` rather than `<=`, because a bound is only evidence when
    // something reached it: `warmed <= WARM_TOTAL` is satisfied by a `warm_ahead`
    // whose loop body was deleted. The fixture offers twice the cap, so equality
    // is what says the run both reached it and stopped there.
    assert_eq!(
        warmed, WARM_TOTAL,
        "the warmer parsed {warmed} files across distinct languages against a \
         total cap of {WARM_TOTAL}, so it either walked past the cap or never \
         reached it and this gate is vacuous"
    );
}

#[test]
fn a_file_that_is_not_text_is_skipped_before_it_spends_the_budget() {
    // A UTF-16 BOM is the ordinary way a path with a known extension is not
    // text. It trims to the empty string and can compile nothing, so counting it
    // both overstates the result and burns one of three per-grammar slots. The
    // sibling rule is already asserted for a path that vanished.
    let scratch = Scratch::large_diff("warm-not-text", 5, 4);
    std::fs::write(scratch.path_of("src/utf16.rs"), [0xFFu8, 0xFE, 0x66, 0x00]).expect("write");

    let alone = Highlighter::new()
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/utf16.rs".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread")
        .warmed;
    assert_eq!(
        alone, 0,
        "a file that is not text counted as a warm, so the result counts files \
         opened rather than grammars compiled"
    );

    // And placed first it must not cost a real file its slot.
    let mut paths = vec!["src/utf16.rs".to_owned()];
    paths.extend((0..5).map(|n| format!("src/mod_{n}.rs")));
    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths, None)
        .join()
        .expect("the warmer thread")
        .warmed;
    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "with an unreadable file first the warmer parsed {warmed} real files \
         against {WARM_PER_GRAMMAR}, so it spent a slot on one that compiled \
         nothing"
    );
}

/// The escape spelling this platform has that the others do not.
///
/// **A function per platform rather than a `cfg` block inside the test**, so the
/// test body is the same on all three tier-1 targets. Written inline, the Unix
/// build compiled the `push` away, left `let mut spellings` never mutated, and
/// `-D warnings` turned that into a build failure on two of the three targets
/// while passing locally on the third.
///
/// Windows is the platform with an extra spelling because `Path::is_absolute`
/// there wants a prefix *and* a root, so stripping the drive leaves a path that
/// fails that test and that `join` still resolves to the same real file. On Unix
/// an absolute path has no prefix, so that case is the plain absolute one the
/// caller already has and there is nothing to add.
#[cfg(windows)]
fn rooted_spelling(outside: &std::path::Path, root: &std::path::Path) -> Option<String> {
    // Asserted to round-trip rather than assumed: `get(2..)` strips two bytes,
    // not a prefix, so on a machine whose temp directory is a UNC share or a
    // verbatim path it yields something the guard *accepts* and that joins to
    // nothing under the worktree. The gate would then pass while asserting
    // nothing, which is the failure it exists to prevent one level up.
    outside
        .to_string_lossy()
        .get(2..)
        .filter(|rooted| root.join(rooted) == outside)
        .map(str::to_owned)
}

#[cfg(not(windows))]
fn rooted_spelling(_outside: &std::path::Path, _root: &std::path::Path) -> Option<String> {
    None
}

#[test]
fn the_warmer_reads_nothing_outside_the_worktree() {
    // `PathBuf::join` silently discards the root for an absolute path. Not
    // reachable from the shell, which passes what status reported, but
    // `warm_ahead` is public on a public type with no other precondition.
    // The bait lives in its own `Scratch` rather than at a fixed name beside the
    // worktree: the temp directory is shared, and a fixed name is one panic away
    // from being left behind, against the soak's zero-retained-temp-files claim.
    let scratch = Scratch::large_diff("warm-escape", 1, 4);
    let bait = Scratch::large_diff("warm-escape-bait", 1, 4);
    let outside = bait.path_of("src/mod_0.rs");

    // **Every spelling here has to resolve to a file that exists**, or refused
    // and not-found are the same answer and the gate is green either way. That
    // is not hypothetical: an earlier version pointed the `..` spelling at
    // nothing, and adding `ParentDir` back to the whitelist left it passing.
    let sibling = bait
        .root()
        .file_name()
        .expect("the bait scratch has a directory name")
        .to_string_lossy()
        .into_owned();
    let mut spellings = vec![
        // Absolute, which `PathBuf::join` discards the root for.
        outside.to_string_lossy().into_owned(),
        // And up-and-over into the sibling scratch, which is the `ParentDir`
        // arm of the guard and the only one that needs no platform spelling.
        format!("../{sibling}/src/mod_0.rs"),
    ];
    assert!(
        scratch.root().join(&spellings[1]).exists(),
        "the `..` spelling resolves to nothing, so refusing it and failing to \
         find it are the same answer and this gate cannot fail"
    );

    // **The spelling a blacklist misses, and it has to name a file that really
    // exists.** A path pointing at nothing is refused and not-found alike, so a
    // gate built on one is green either way: reverting this guard to its earlier
    // `is_absolute() || ParentDir` form left an earlier version of this test
    // passing. `Path::is_absolute` on Windows wants a prefix *and* a root, so
    // stripping the drive from the bait leaves a path that fails that test and
    // that `join` still resolves to the same real file.
    //
    // On Unix an absolute path has no prefix, so the case is already the one a
    // line above and there is nothing extra to spell.
    spellings.extend(rooted_spelling(&outside, scratch.root()));

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), spellings, None)
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, 0,
        "the warmer read {warmed} files from outside the worktree it was given"
    );
}

#[test]
fn a_file_cut_mid_character_still_parses() {
    // The cut `WARM_BYTES` makes lands mid-character on anything that is not
    // ASCII, which the `valid_up_to` trim absorbs. Dropping that trim is one
    // edit and nothing else here would notice.
    //
    // **Only the trim, not the bound.** This file is valid UTF-8 throughout, so
    // it warms either way and an unbounded read passes it; the bound is gated by
    // `the_read_is_bounded_by_bytes_and_not_by_the_size_of_the_file`, which is
    // the one that can see it.
    let scratch = Scratch::large_diff("warm-bounded-read", 1, 4);
    let mut text = String::from("// ");
    while text.len() < WARM_BYTES.saturating_sub(2) {
        text.push('a');
    }
    // A four-byte character straddling the cut.
    text.push('\u{1F600}');
    // Far past the bound, so an unbounded read would take all of it.
    while text.len() < WARM_BYTES * 8 {
        text.push_str("\nfn filler() { let s = \"x\"; }");
    }
    std::fs::write(scratch.path_of("src/straddle.rs"), &text).expect("write");

    let warmed = Highlighter::new()
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/straddle.rs".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, 1,
        "a file whose bounded read lands mid-character was skipped, so any file \
         with a wide glyph near {WARM_BYTES} bytes warms nothing"
    );
}

#[test]
fn the_read_is_bounded_by_bytes_and_not_by_the_size_of_the_file() {
    // **Two problem sizes whose first `WARM_BYTES` are byte-identical**, which is
    // the only shape that can see this bound. Every structural assertion here is
    // satisfied whether or not the read is capped — a warm counts the same either
    // way — and mutating the `take` away survived all of them. What separates the
    // two is wall clock against file size, so it takes the absolute tier.
    //
    // `SPEC.md` §7's rule about not comparing a part against the whole it belongs
    // to applies directly: the shared term is the first `WARM_BYTES`, and it
    // cancels, so what is left is exactly the tail an unbounded read would take.
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("warm-read-bound", 1, 4);
    let head: String =
        std::iter::repeat_n("fn f() { let s = \"x\"; }\n", WARM_BYTES / 24 + 1).collect::<String>();
    assert!(
        head.len() >= WARM_BYTES,
        "the shared prefix is {} bytes, under the {WARM_BYTES} the read is capped \
         at, so the two fixtures differ inside the window rather than beyond it",
        head.len()
    );

    let mut big = head.clone();
    while big.len() < WARM_BYTES * 256 {
        big.push_str("fn g() { let s = \"y\"; }\n");
    }
    std::fs::write(scratch.path_of("src/small.rs"), &head).expect("write");
    std::fs::write(scratch.path_of("src/big.rs"), &big).expect("write");

    // One highlighter, warmed first, so neither timing carries the one-off
    // grammar compile and what is left is read plus parse.
    let highlighter = Highlighter::new();
    highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/mod_0.rs".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread");

    let small = time(|| {
        highlighter
            .warm_ahead(
                scratch.root().to_path_buf(),
                vec!["src/small.rs".to_owned()],
                None,
            )
            .join()
            .expect("the warmer thread");
    });
    let large = time(|| {
        highlighter
            .warm_ahead(
                scratch.root().to_path_buf(),
                vec!["src/big.rs".to_owned()],
                None,
            )
            .join()
            .expect("the warmer thread");
    });

    eprintln!(
        "note: warming {} bytes took {small:?}; warming {} bytes took {large:?}",
        head.len(),
        big.len()
    );

    // A file 256 times larger, read to its end, cannot come out inside four
    // times the cost of the capped one. Loose on purpose: the point is to catch
    // a read proportional to the file, which is two orders of magnitude, not to
    // track microseconds on a shared runner.
    assert!(
        large <= small * 4,
        "warming a {}-byte file took {large:?} against {small:?} for the \
         {}-byte one with the same first {WARM_BYTES} bytes, so the read follows \
         the file rather than the bound",
        big.len(),
        head.len()
    );
}

/// Make `link` inside the worktree point at `target`, or report that this
/// platform would not.
///
/// Windows needs `SeCreateSymbolicLinkPrivilege` or developer mode, so a
/// hosted runner may refuse. Refusing is reported rather than silently turning
/// the gate below into a pass, which is the shape `SPEC.md` §7 keeps naming.
fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}

#[test]
fn the_warmer_reads_nothing_through_a_symlink_out_of_the_worktree() {
    // The hole a lexical check cannot see, and the one Copilot's review found.
    // Every component of `link/mod_0.rs` is `Normal`, so the whitelist passes
    // it, and the read then lands wherever the link points, because the warmer
    // reads with `fs::read` and `fs::read` follows a link. Note that
    // `Worktree::read_worktree` stopped following one in
    // [#15](https://github.com/breferrari/vigia/issues/15): the diff path and
    // the warmer answer different questions about a link, so this gate cannot
    // be retired on the strength of that change.
    let scratch = Scratch::large_diff("warm-symlink", 1, 4);
    let bait = Scratch::large_diff("warm-symlink-bait", 1, 4);

    let link = scratch.path_of("link");
    if !link_dir(&bait.path_of("src"), &link) {
        eprintln!(
            "note: this platform would not create a directory symlink, so the \
             symlink escape is unchecked here; it is checked wherever one can be \
             made"
        );
        return;
    }

    // Non-vacuity: the link has to actually reach the bait, or `warmed == 0`
    // below means "nothing there" rather than "refused".
    assert!(
        link.join("mod_0.rs").exists(),
        "the symlink does not reach the bait, so this gate cannot fail"
    );

    let warmed = Highlighter::new()
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["link/mod_0.rs".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread")
        .warmed;

    assert_eq!(
        warmed, 0,
        "the warmer read {warmed} files through a symlink pointing out of the \
         worktree, so its bound is lexical and the docs claiming otherwise are \
         wrong"
    );
}

/// Every class one screenful of the first hunk of `path` draws.
///
/// The shape a frame drives the highlighter in, reduced to the one thing these
/// gates assert. Returns a class per span rather than a set, so a caller can say
/// *all plain* and *some not plain* without either collapsing into the other.
fn classes_of(frame: &mut Frame, highlighter: &mut Highlighter, path: &str) -> Vec<Class> {
    let index = frame
        .files()
        .iter()
        .position(|change| change.path == path)
        .unwrap_or_else(|| panic!("{path} is not a changed file"));

    let mut classes = Vec::new();
    let mut pass = highlighter.pass();
    let (_, diff) = frame.diff(index).expect("diff");
    let hunk = diff.hunks.first().expect("the fixture has a hunk");
    for line in 0..hunk.lines.len() {
        classes.extend(
            pass.spans(path, 0, hunk, line, None)
                .iter()
                .map(|s| s.class),
        );
    }
    drop(pass);
    classes
}

/// I9's budget, restated here because this gate is about a frame rather than
/// about the warmer's own bounds.
const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// Rows one screenful shows, and so lines this gate asks the highlighter for.
const SCREENFUL: usize = 24;

/// Highlight every line of the first hunk of `path`, the way a frame does.
///
/// Returns the rows drawn, so a window that drew nothing cannot satisfy a gate
/// by costing nothing.
fn draw_one_screenful(frame: &mut Frame, highlighter: &mut Highlighter, path: &str) -> usize {
    let index = frame
        .files()
        .iter()
        .position(|change| change.path == path)
        .unwrap_or_else(|| panic!("{path} is not a changed file"));

    let mut pass = highlighter.pass();
    let (_, diff) = frame.diff(index).expect("diff");
    let hunk = diff.hunks.first().expect("the fixture has a hunk");
    let mut drawn = 0;
    for line in 0..hunk.lines.len() {
        pass.spans(path, 0, hunk, line, None);
        drawn += 1;
    }
    drop(pass);
    drawn
}

/// **The frame a reader actually meets, and the one nothing gated.**
///
/// `SPEC.md` §7 rules a grammar's first parse onto the cold path that I9
/// excludes by definition, and that ruling was made when the only way to reach
/// one was a keypress: §10 lists `G`, a follow jump and a scroll, all of which a
/// reader asked for. [#129](https://github.com/breferrari/vigia/issues/129)
/// reports it arriving on **no** keypress, from three directions and on two
/// platforms, because the agent in the other pane writes a file in a language
/// this process has not met and the frame that draws it pays the compile.
///
/// Measured on the reference machine, release, twenty-four lines: **123.98ms
/// cold against 2.40ms warm** under Rust, and 694.75ms against 90.65ms under
/// Markdown. The cliff is flat in content size, which is what says it is a
/// compile rather than a parse: a 594-byte screenful of Markdown costs 631.46ms
/// cold and 0.97ms warm, a 650x penalty on half a kilobyte.
///
/// So this is I9 over the frame that meets a grammar, and it is the gate the
/// deferral has to earn. Watched failing before the fix at 93.73ms.
#[test]
fn a_frame_that_meets_a_new_grammar_holds_the_frame_budget() {
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("warm-first-frame", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    // Fresh, so every pattern under this grammar is genuinely uncompiled: a
    // `SyntaxSet` owns its own `OnceCell`s, so a highlighter another test warmed
    // is a different set and cannot leak into this one.
    let mut highlighter = Highlighter::new();

    let mut drawn = 0;
    let cost = time(|| drawn = draw_one_screenful(&mut frame, &mut highlighter, "src/mod_0.rs"));

    assert_eq!(
        drawn, SCREENFUL,
        "the fixture drew {drawn} rows rather than a screenful, so the number \
         below is not a frame's worth of work"
    );
    assert!(
        cost <= budget(FRAME_BUDGET),
        "the first frame to meet a grammar cost {cost:?} against I9's \
         {FRAME_BUDGET:?}, so a reader whose agent writes a new language pays \
         the grammar compile on a frame they did not ask for"
    );
}

/// The structural half of the gate above, and the one that survives a fast
/// machine.
///
/// A wall clock says the frame was quick; this says **why**, and it is the claim
/// a mutation has to break rather than merely slow down. `lines` counts what the
/// parser actually ran, so zero of them across a full screenful is the deferral
/// working, and the classes being all `Plain` is what the reader sees while it
/// does.
#[test]
fn a_grammar_nothing_has_compiled_draws_plain_and_parses_nothing() {
    let scratch = Scratch::large_diff("warm-defer-plain", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let classes = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");

    assert_eq!(
        highlighter.stats().lines,
        0,
        "the frame parsed lines under a grammar nothing has compiled, so it \
         paid the compile cliff on a frame a reader was waiting on"
    );
    assert!(
        classes.iter().all(|class| *class == Class::Plain),
        "a deferred hunk drew {classes:?} rather than plain, so `sides` was \
         built for a grammar the warmer has not run over"
    );
    assert_eq!(
        highlighter.wanted(),
        ["src/mod_0.rs"],
        "the frame drew plain and asked for nothing, so the colour it is owed \
         would never arrive"
    );
}

/// And the other direction, which is what makes the gate above a deferral rather
/// than a regression.
///
/// **Both halves or neither.** A change that simply stopped highlighting would
/// pass the test above and fail this one, which is the pairing `SPEC.md` §7 asks
/// for wherever a gate asserts an absence.
#[test]
fn a_warm_turns_the_plain_hunk_into_colour() {
    let scratch = Scratch::large_diff("warm-defer-thaw", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let before = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    assert!(
        before.iter().all(|class| *class == Class::Plain),
        "the first pass was already coloured, so nothing here is being deferred \
         and the second pass proves nothing"
    );

    // The demand the frame raised, served the way the shell serves it.
    let wanted = highlighter.wanted().to_vec();
    highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");

    let after = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    assert!(
        after.iter().any(|class| *class != Class::Plain),
        "the hunk was still plain after its grammar was compiled, so a deferred \
         entry is never rebuilt and the pane loses its colour for the session"
    );
    assert!(
        highlighter.wanted().is_empty(),
        "the frame asked for a warm it has already been given, which is a wake \
         per frame for as long as the hunk is on screen"
    );
}

/// **A demand that can never be served must still stop.**
///
/// The frame path draws a hunk out of the *diff*, which survives the file the
/// diff was taken from. So a path that vanishes between the frame asking and the
/// warmer opening it would be asked for again on the next frame, and every ask
/// spawns a thread and every thread sends a wake: a livelock, at full CPU, on a
/// monitor whose whole claim is that it costs nothing while idle.
///
/// The warmer therefore marks a grammar it merely *had a run at*, not only one
/// it compiled. What that costs is that such a grammar is parsed cold once,
/// which is exactly what shipped before any of this existed.
#[test]
fn a_path_that_vanished_does_not_leave_the_frame_asking() {
    let scratch = Scratch::large_diff("warm-defer-vanished", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let _ = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    let wanted = highlighter.wanted().to_vec();
    assert_eq!(
        wanted.len(),
        1,
        "the frame asked for {wanted:?}, so the run below is not the one this \
         gate means to make fail"
    );

    // Gone before the warmer reaches it, which beside an agent is ordinary
    // rather than exotic: a rename, a `git checkout`, a build cleaning up.
    scratch.remove("src/mod_0.rs");
    let report = highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");
    assert_eq!(
        report.warmed, 0,
        "the file was removed and the warmer still parsed something, so this \
         gate is not testing the case it names"
    );

    let _ = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    assert!(
        highlighter.wanted().is_empty(),
        "the frame asked again for a grammar the warmer has already failed at, \
         so every frame from here spawns a thread and sends a wake"
    );
}

/// The demand describes **this** frame, not every frame there has ever been.
///
/// A list that accumulated would hand the warmer paths the reader scrolled off
/// minutes ago, and would never be empty, so the shell would spawn a warm after
/// every paint for the life of the process.
#[test]
fn a_settled_frame_asks_for_nothing() {
    let scratch = Scratch::large_diff("warm-defer-settled", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let _ = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    let wanted = highlighter.wanted().to_vec();
    highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");

    // Two passes past the warm: the first rebuilds the deferred entry, the
    // second is the steady state a reader sits in for hours.
    let _ = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");
    let _ = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");

    assert!(
        highlighter.wanted().is_empty(),
        "a settled frame is still asking for a warm, so I1's idle cost is a \
         thread and a wake per paint rather than nothing"
    );
}

/// An eager highlighter has no demand to raise, because it never defers.
///
/// The affordance the render gates use, asserted rather than assumed: they build
/// one and then assert colour, so a version of it that deferred would make every
/// one of them vacuous at once.
#[test]
fn an_eager_highlighter_parses_a_grammar_nothing_has_compiled() {
    let scratch = Scratch::large_diff("warm-eager", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();

    let classes = classes_of(&mut frame, &mut highlighter, "src/mod_0.rs");

    assert!(
        classes.iter().any(|class| *class != Class::Plain),
        "an eager highlighter drew plain, so every gate that builds one to \
         assert a colour is asserting nothing"
    );
    assert!(
        highlighter.wanted().is_empty(),
        "an eager highlighter raised a demand nobody is going to serve"
    );
}

/// A repository of `counts` files per extension, committed, so the index holds
/// them.
///
/// `leading_paths` reads the **index** rather than walking the worktree, which
/// is what makes it free beside a frame path that already opens one. So a
/// fixture whose files are merely written proves nothing here, and this commits.
fn indexed(name: &str, counts: &[(&str, usize)]) -> Scratch {
    let scratch = Scratch::new(name);
    for (extension, count) in counts {
        for n in 0..*count {
            scratch.write(
                &format!("src/file_{n}.{extension}"),
                "fn main() {}\nlet x = 1;\n",
            );
        }
    }
    scratch.commit_all("baseline");
    scratch
}

/// Extensions come back commonest first, which is the whole of the ranking.
///
/// **What a repository leads with is the only thing that makes a speculative
/// warm affordable.** Warming the tail costs +58.00 MiB over ten grammars,
/// measured on the reference machine; warming the language a repository is
/// mostly made of moves memory the session was near-certain to spend anyway. A
/// ranking that came back in index order instead would spend the whole budget on
/// whatever sorts first.
#[test]
fn the_leading_extensions_come_back_commonest_first() {
    let scratch = indexed("warm-leading-rank", &[("rs", 9), ("py", 5), ("go", 2)]);

    let paths = leading_paths(scratch.root(), 3, 1);

    let extensions: Vec<&str> = paths
        .iter()
        .filter_map(|path| std::path::Path::new(path).extension()?.to_str())
        .collect();
    assert_eq!(
        extensions,
        ["rs", "py", "go"],
        "the ranking came back as {extensions:?} rather than by file count, so \
         the warm budget is spent on whatever the index happened to hold first"
    );
}

/// And it is the same answer every time, which a gate needs and a reader wants.
///
/// Two extensions with the same count have no order of their own, and a
/// `HashMap`'s iteration order is deliberately not one: without a tie-break the
/// three grammars warmed would differ run to run, so a session's memory and its
/// first coloured frame would both be luck.
#[test]
fn extensions_with_the_same_count_break_their_tie_the_same_way_every_time() {
    let scratch = indexed("warm-leading-tie", &[("rs", 4), ("py", 4), ("go", 4)]);

    let first = leading_paths(scratch.root(), 3, 1);
    for _ in 0..4 {
        assert_eq!(
            leading_paths(scratch.root(), 3, 1),
            first,
            "two runs over one index disagreed, so which grammars get warmed is \
             decided by hash order"
        );
    }
    let extensions: Vec<&str> = first
        .iter()
        .filter_map(|path| std::path::Path::new(path).extension()?.to_str())
        .collect();
    assert_eq!(
        extensions,
        ["go", "py", "rs"],
        "the tie broke as {extensions:?} rather than on the extension itself"
    );
}

/// The output is bounded, and so is what it costs to produce.
///
/// **A bound on the walk, not only on the answer**, which is the same
/// distinction `WARM_BYTES` records one function over: retaining every path and
/// truncating afterwards is a copy of the index, and an index is the one
/// structure in this process that scales with the repository rather than with
/// the window. I3 is a claim about a process left open for days.
#[test]
fn no_more_than_the_asked_for_paths_per_extension_come_back() {
    let scratch = indexed("warm-leading-bound", &[("rs", 40), ("py", 40)]);

    let paths = leading_paths(scratch.root(), 2, WARM_PER_GRAMMAR);

    assert_eq!(
        paths.len(),
        2 * WARM_PER_GRAMMAR,
        "eighty indexed files produced {} paths against the {} asked for",
        paths.len(),
        2 * WARM_PER_GRAMMAR
    );
}

/// Somewhere that is not a repository is nothing to warm, not a failure.
///
/// This runs on a detached thread that upholds nothing, so every way it can fail
/// has to end in an empty answer rather than in a panic: the workspace builds
/// with `panic = "abort"`, so a panic here takes the monitor with it.
#[test]
fn a_directory_that_is_not_a_repository_leads_with_nothing() {
    let scratch = Scratch::new("warm-leading-bare");
    let outside = scratch.root().join("..").join("definitely-not-here");

    assert!(leading_paths(&outside, 3, 1).is_empty());
    assert!(leading_paths(scratch.root(), 0, 1).is_empty());
    assert!(leading_paths(scratch.root(), 3, 0).is_empty());
}

/// **The product claim: a language the repository leads with is compiled before
/// anybody writes to it, and the tail is not.**
///
/// This is the half of [#129](https://github.com/breferrari/vigia/issues/129)
/// that keeps the pane from flickering at all in the case being watched. A
/// monitor opened beside an agent that has not started yet has an empty changed
/// set, so the warm over *that* has nothing to do; the index still knows the
/// repository is mostly Rust.
///
/// Both directions, because a warm with no cap would pass the first assertion
/// and spend the +58.00 MiB the cap exists to refuse.
#[test]
fn the_repositorys_leading_grammars_are_compiled_and_its_tail_is_not() {
    let scratch = indexed(
        "warm-leading-product",
        &[("rs", 9), ("py", 7), ("go", 5), ("css", 3)],
    );
    let highlighter = Highlighter::new();

    highlighter
        .warm_repository(scratch.root().to_path_buf(), None)
        .join()
        .expect("the warmer thread");

    // Written after the warm, so these are files the sweep never saw: what is
    // being asserted is the *grammar*, not the path.
    scratch.write("src/late.rs", "fn late() -> u32 { 1 }\n");
    scratch.write("src/late.css", "a { color: red; }\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = highlighter;

    let leading = classes_of(&mut frame, &mut highlighter, "src/late.rs");
    assert!(
        leading.iter().any(|class| *class != Class::Plain),
        "the repository is mostly Rust and a Rust file still drew plain, so the \
         population sweep bought the reader nothing"
    );

    let tail = classes_of(&mut frame, &mut highlighter, "src/late.css");
    assert!(
        tail.iter().all(|class| *class == Class::Plain),
        "a grammar past the {WARM_LEADING} the cap allows was compiled anyway, \
         so the sweep is spending memory on the tail it exists to refuse"
    );
}
