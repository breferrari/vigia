//! Compiling grammars ahead of the reader, and what that is and is not worth.

mod support;

use std::time::Duration;

use support::{Scratch, absolute_gates_apply, budget, exclusively_timed, highlight_window, time};
use vigia_core::{
    Class, Frame, Highlighter, INDEXED_EXTENSION, INDEXED_EXTENSIONS, WARM_BYTES, WARM_FILES,
    WARM_LEADING, WARM_PER_GRAMMAR, WARM_TOTAL, indexed_extensions,
};

#[test]
fn a_path_with_no_grammar_is_skipped_before_it_is_read() {
    // The same answer `syntax_for` gives the frame path for a file type nothing
    // recognises, and it has to be reachable without panicking because the warmer walks
    // whatever status named.
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
    // The bound that decides what the warmer costs. Compiling a grammar is
    // one file's work and every later file of the same language buys only a
    // decaying residual, so a changed set of eighty-four Rust files must not be
    // eighty-four parses. Measured before this cap existed: sixty-four files was
    // 1.053s of held core, about 96% of it re-parsing an already-compiled
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
    // sharing one grammar the per-grammar cap binds first and a missing path cap would
    // be invisible.
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
    // A file can vanish between status naming it and this thread reaching it, which is
    // ordinary beside an agent.
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
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    // Taken for the reason `crates/vigia/tests/budgets.rs` takes it: this binary also
    // runs `many_files_of_one_language_warm_only_a_few`, which builds an eighty-four
    // file fixture, and a polyglot gate that writes twenty-five files and compiles up
    // to twelve grammars on a thread.
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

    // An absolute saving, not a ratio, and the ratio it replaces is a
    // cautionary tale rather than a tightening.
    const WARMED_CEILING: Duration = Duration::from_millis(8);
    const LEAST_SAVING: Duration = Duration::from_millis(5);
    assert!(
        after < WARMED_CEILING,
        "a warmed parse took {after:?}, over the {WARMED_CEILING:?} a warmed \
         parse has to fit in for the frame behind it to be affordable, so \
         warming is not leaving the patterns compiled"
    );
    assert!(
        cold_parse >= after + LEAST_SAVING,
        "warming saved {:?} ({cold_parse:?} cold against {after:?} warmed), \
         under the {LEAST_SAVING:?} that says a compile was moved at all, so \
         `warm` is not compiling anything the parse behind it would have paid \
         for",
        cold_parse.saturating_sub(after)
    );
}

#[test]
fn the_per_grammar_cap_is_per_grammar_and_not_one_shared_counter() {
    // The bound the docs call "the bound that matters" had no test saying it was per
    // grammar at all.
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
    // The per-grammar cap gives a polyglot tree as many budgets as it has languages.
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

    // `assert_eq!` rather than `<=`, because a bound is only evidence when something
    // reached it: `warmed <= WARM_TOTAL` is satisfied by a `warm_ahead` whose loop body
    // was deleted.
    assert_eq!(
        warmed, WARM_TOTAL,
        "the warmer parsed {warmed} files across distinct languages against a \
         total cap of {WARM_TOTAL}, so it either walked past the cap or never \
         reached it and this gate is vacuous"
    );
}

#[test]
fn a_file_that_is_not_text_is_skipped_before_it_spends_the_budget() {
    // A UTF-16 BOM is the ordinary way a path with a known extension is not text.
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
#[cfg(windows)]
fn rooted_spelling(outside: &std::path::Path, root: &std::path::Path) -> Option<String> {
    // Asserted to round-trip rather than assumed: `get(2..)` strips two bytes, not a
    // prefix, so on a machine whose temp directory is a UNC share or a verbatim path it
    // yields something the guard *accepts* and that joins to nothing under the
    // worktree.
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
    // `PathBuf::join` silently discards the root for an absolute path.
    let scratch = Scratch::large_diff("warm-escape", 1, 4);
    let bait = Scratch::large_diff("warm-escape-bait", 1, 4);
    let outside = bait.path_of("src/mod_0.rs");

    // Every spelling here has to resolve to a file that exists, or refused and
    // not-found are the same answer and the gate is green either way.
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

    // The spelling a blacklist misses, and it has to name a file that really
    // exists. A path pointing at nothing is refused and not-found alike, so a
    // gate built on one is green either way: an `is_absolute() || ParentDir`
    // guard passes against a bait that names nothing.
    // `Path::is_absolute` on Windows wants a prefix *and* a root, so
    // stripping the drive from the bait leaves a path that fails that test and
    // that `join` still resolves to the same real file.
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
    // Two problem sizes whose first `WARM_BYTES` are byte-identical, which is the only
    // shape that can see this bound.
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

    // A file 256 times larger, read to its end, cannot come out inside four times the
    // cost of the capped one.
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

/// I9's budget, restated here because this gate is about a frame rather than
/// about the warmer's own bounds.
const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// Rows one screenful shows, and so lines this gate asks the highlighter for.
const SCREENFUL: usize = 24;

/// The frame a reader actually meets, and the one nothing gated.
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
    let cost =
        time(|| drawn = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).rows);

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
#[test]
fn a_grammar_nothing_has_compiled_draws_plain_and_parses_nothing() {
    let scratch = Scratch::large_diff("warm-defer-plain", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let classes = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;

    assert_eq!(
        highlighter.stats().lines,
        0,
        "the frame parsed lines under a grammar nothing has compiled, so it \
         paid the compile cliff on a frame a reader was waiting on"
    );
    assert!(
        classes.iter().all(|class| *class == Class::Plain),
        "a deferred hunk drew {classes:?} rather than plain, so the entry was \
         built `Parse::Ready` for a grammar the warmer has not run over"
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
#[test]
fn a_warm_turns_the_plain_hunk_into_colour() {
    let scratch = Scratch::large_diff("warm-defer-thaw", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let before = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
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

    let after = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
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

/// A demand that can never be served must still stop.
#[test]
fn a_path_that_vanished_does_not_leave_the_frame_asking() {
    let scratch = Scratch::large_diff("warm-defer-vanished", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
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

    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
    assert!(
        highlighter.wanted().is_empty(),
        "the frame asked again for a grammar the warmer has already failed at, \
         so every frame from here spawns a thread and sends a wake"
    );
}

/// The demand describes this frame, not every frame there has ever been.
#[test]
fn a_settled_frame_asks_for_nothing() {
    let scratch = Scratch::large_diff("warm-defer-settled", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
    let wanted = highlighter.wanted().to_vec();
    highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");

    // Two passes past the warm: the first rebuilds the deferred entry, the
    // second is the steady state a reader sits in for hours.
    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;

    assert!(
        highlighter.wanted().is_empty(),
        "a settled frame is still asking for a warm, so I1's idle cost is a \
         thread and a wake per paint rather than nothing"
    );
}

/// An eager highlighter has no demand to raise, because it never defers.
#[test]
fn an_eager_highlighter_parses_a_grammar_nothing_has_compiled() {
    let scratch = Scratch::large_diff("warm-eager", 1, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();

    let classes = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;

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
#[test]
fn the_indexed_extensions_come_back_commonest_first() {
    let scratch = indexed("warm-leading-rank", &[("rs", 9), ("py", 5), ("go", 2)]);

    let tally = indexed_extensions(scratch.root(), 1);

    let ranked: Vec<(&str, usize)> = tally
        .iter()
        .map(|indexed| (indexed.extension.as_str(), indexed.files))
        .collect();
    assert_eq!(
        ranked,
        [("rs", 9), ("py", 5), ("go", 2)],
        "the tally came back as {ranked:?} rather than by file count, so the \
         warm budget is spent on whatever the index happened to hold first"
    );
}

/// And it is the same answer every time, which a gate needs and a reader wants.
#[test]
fn extensions_with_the_same_count_break_their_tie_the_same_way_every_time() {
    let scratch = indexed("warm-leading-tie", &[("rs", 4), ("py", 4), ("go", 4)]);

    let first = indexed_extensions(scratch.root(), 1);
    for _ in 0..4 {
        assert_eq!(
            indexed_extensions(scratch.root(), 1),
            first,
            "two runs over one index disagreed, so which grammars get warmed is \
             decided by hash order"
        );
    }
    let extensions: Vec<&str> = first
        .iter()
        .map(|indexed| indexed.extension.as_str())
        .collect();
    assert_eq!(
        extensions,
        ["go", "py", "rs"],
        "the tie broke as {extensions:?} rather than on the extension itself"
    );
}

/// The paths are bounded, and so is what it costs to produce them.
#[test]
fn no_more_than_the_asked_for_paths_per_extension_come_back() {
    let scratch = indexed("warm-leading-bound", &[("rs", 40), ("py", 40)]);

    let tally = indexed_extensions(scratch.root(), WARM_PER_GRAMMAR);

    for indexed in &tally {
        assert_eq!(
            indexed.paths.len(),
            WARM_PER_GRAMMAR,
            ".{} kept {} paths of its {} files, against the {WARM_PER_GRAMMAR} \
             asked for",
            indexed.extension,
            indexed.paths.len(),
            indexed.files
        );
        assert_eq!(
            indexed.files, 40,
            ".{} counted {} files, so bounding the paths also bounded the count \
             and the merge can no longer be exact",
            indexed.extension, indexed.files
        );
    }
}

/// Somewhere that is not a repository is nothing to warm, not a failure.
#[test]
fn a_directory_that_is_not_a_repository_leads_with_nothing() {
    let scratch = Scratch::new("warm-leading-bare");
    let outside = scratch.root().join("..").join("definitely-not-here");

    assert!(indexed_extensions(&outside, 1).is_empty());
    assert!(indexed_extensions(scratch.root(), 0).is_empty());
}

/// A language spelled two ways is one grammar, and the budget must see it
/// that way.
#[test]
fn a_language_spelled_two_ways_is_counted_once() {
    let scratch = indexed(
        "warm-leading-merge",
        &[("rs", 9), ("go", 8), ("py", 7), ("yml", 6), ("yaml", 6)],
    );
    let highlighter = Highlighter::new();

    highlighter
        .warm_repository(scratch.root().to_path_buf(), None)
        .join()
        .expect("the warmer thread");

    // Written after the sweep, so these are files it never saw: what is being
    // asserted is the grammar, not the path.
    scratch.write("late.yaml", "key: value\nlist:\n  - 1\n");
    scratch.write("late.py", "def f():\n    return 'x'\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = highlighter;

    let merged = highlight_window(&mut frame, &mut highlighter, "late.yaml", 0, 1).classes;
    assert!(
        merged.iter().any(|class| *class != Class::Plain),
        "YAML is this repository's commonest language across .yml and .yaml \
         together, and it still drew plain, so the two spellings are being \
         counted as two languages"
    );

    let outranked = highlight_window(&mut frame, &mut highlighter, "late.py", 0, 1).classes;
    assert!(
        outranked.iter().all(|class| *class == Class::Plain),
        "Python is fourth by grammar and was compiled anyway, so the \
         {WARM_LEADING} cap is not being spent on the leaders"
    );
}

/// The product claim: a language the repository leads with is compiled before
/// anybody writes to it, and the tail is not.
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

    let leading = highlight_window(&mut frame, &mut highlighter, "src/late.rs", 0, 1).classes;
    assert!(
        leading.iter().any(|class| *class != Class::Plain),
        "the repository is mostly Rust and a Rust file still drew plain, so the \
         population sweep bought the reader nothing"
    );

    let tail = highlight_window(&mut frame, &mut highlighter, "src/late.css", 0, 1).classes;
    assert!(
        tail.iter().all(|class| *class == Class::Plain),
        "a grammar past the {WARM_LEADING} the cap allows was compiled anyway, \
         so the sweep is spending memory on the tail it exists to refuse"
    );
}

/// A shebang script's grammar is knowable only from its first line, and the
/// warmer gave up before it could be marked.
#[test]
fn a_grammar_only_the_first_line_knows_does_not_spin_the_warmer() {
    let scratch = Scratch::new("warm-shebang");
    scratch.write("bin/setup", "#!/usr/bin/env python\nold = 1\n");
    scratch.commit_all("baseline");
    scratch.write(
        "bin/setup",
        "#!/usr/bin/env python\nimport os\n\n\ndef main():\n    return os.getcwd()\n",
    );

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let first = shebang_classes(&mut frame, &mut highlighter);
    assert!(
        first.iter().all(|class| *class == Class::Plain),
        "the script drew coloured on the frame that met it, so nothing is being \
         deferred here and the rest of this gate proves nothing"
    );
    assert_eq!(
        highlighter.wanted(),
        ["bin/setup"],
        "the frame drew plain and asked for nothing, so this fixture cannot \
         reach the case"
    );

    // Served exactly the way the shell serves it.
    let wanted = highlighter.wanted().to_vec();
    let report = highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");
    assert_eq!(
        report.warmed, 1,
        "the warmer compiled {} files for a script whose grammar its own first \
         line names, so an extensionless path is still unwarmable",
        report.warmed
    );

    let second = shebang_classes(&mut frame, &mut highlighter);
    assert!(
        highlighter.wanted().is_empty(),
        "the frame asked again for a grammar the warmer has already run over, \
         so every frame from here spawns a thread and sends a wake on a tree \
         nobody is touching, which is I1's budget breached"
    );
    assert!(
        second.iter().any(|class| *class != Class::Plain),
        "the script never gained its colour, so the warm marked a grammar it \
         had not compiled"
    );
}

/// Drive one screenful of the shebang fixture, passing the first line the frame
/// path passes.
fn shebang_classes(frame: &mut Frame, highlighter: &mut Highlighter) -> Vec<Class> {
    let index = frame
        .files()
        .iter()
        .position(|change| change.path == "bin/setup")
        .expect("bin/setup is a changed file");
    let mut classes = Vec::new();
    let mut pass = highlighter.pass();
    let (_, diff) = frame.diff(index).expect("diff");
    let hunk = diff.hunks.first().expect("the fixture has a hunk");
    for line in 0..hunk.lines.len() {
        classes.extend(
            pass.spans("bin/setup", 0, hunk, line, Some("#!/usr/bin/env python"))
                .iter()
                .map(|span| span.class),
        );
    }
    drop(pass);
    classes
}

/// A hostile index cannot turn one background scan into an unbounded
/// allocation.
#[test]
fn an_index_of_many_distinct_extensions_is_bounded() {
    let scratch = Scratch::new("warm-index-flood");
    let flood = INDEXED_EXTENSIONS + 200;
    for n in 0..flood {
        scratch.write(&format!("f{n}.x{n}"), "body\n");
    }
    scratch.git(&["add", "-A"]);

    let tally = indexed_extensions(scratch.root(), 1);

    assert_eq!(
        tally.len(),
        INDEXED_EXTENSIONS,
        "{flood} distinct extensions produced {} entries, so the tally is \\
         bounded by the index rather than by this crate",
        tally.len()
    );
}

/// An extension no grammar could register is not worth a `HashMap` entry.
#[test]
fn an_extension_longer_than_any_grammar_registers_is_skipped() {
    let scratch = Scratch::new("warm-index-long");
    let long = "z".repeat(INDEXED_EXTENSION + 1);
    scratch.write(&format!("bloated.{long}"), "body\\n");
    scratch.write("ordinary.rs", "fn main() {}\\n");
    scratch.git(&["add", "-A"]);

    let tally = indexed_extensions(scratch.root(), 1);

    let extensions: Vec<&str> = tally
        .iter()
        .map(|indexed| indexed.extension.as_str())
        .collect();
    assert_eq!(
        extensions,
        ["rs"],
        "the tally came back as {extensions:?}, so an extension no grammar can \\
         register is still being kept"
    );
}

/// The count stays exact past the cap, which is the half that decides the
/// merge.
#[test]
fn an_extension_already_tracked_keeps_counting_past_the_cap() {
    let scratch = Scratch::new("warm-index-exact");
    // Written first, so it is in the tally before the flood fills it.
    for n in 0..3 {
        scratch.write(&format!("aaa_early_{n}.rs"), "fn main() {}\\n");
    }
    for n in 0..(INDEXED_EXTENSIONS + 50) {
        scratch.write(&format!("mid_{n}.y{n}"), "body\\n");
    }
    for n in 0..4 {
        scratch.write(&format!("zzz_late_{n}.rs"), "fn main() {}\\n");
    }
    scratch.git(&["add", "-A"]);

    let tally = indexed_extensions(scratch.root(), 1);

    let rust = tally
        .iter()
        .find(|indexed| indexed.extension == "rs")
        .expect("the tally kept the extension it saw first");
    assert_eq!(
        rust.files, 7,
        "Rust counted {} of its 7 files, so the tally's cap is silently \\
         truncating a count the merge ranks on",
        rust.files
    );
}

/// The grammar charged is the one compiled, not the one the extension named.
#[test]
fn a_qt_translation_file_charges_xml_and_not_typescript() {
    // Committed thin and then written full, so every one of the three differs
    // from the index and reaches the frame. Written identically on both sides
    // they are unchanged files, and a frame has nothing to draw for them.
    let scratch = Scratch::new("warm-qt-retarget");
    for path in ["ui/strings.ts", "ui/app.ts", "ui/config.xml"] {
        scratch.write(
            path, "
",
        );
    }
    scratch.commit_all("baseline");
    scratch.write("ui/strings.ts", QT_TRANSLATION);
    scratch.write("ui/app.ts", TYPESCRIPT);
    scratch.write("ui/config.xml", XML);

    let highlighter = Highlighter::new();
    let report = highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["ui/strings.ts".to_owned()],
            None,
        )
        .join()
        .expect("the warmer thread");
    assert_eq!(
        report.warmed, 1,
        "the Qt file was not warmed at all, so this gate cannot say which \\
         grammar it charged"
    );

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = highlighter;

    let xml = highlight_window(&mut frame, &mut highlighter, "ui/config.xml", 0, 1).classes;
    assert!(
        xml.iter().any(|class| *class != Class::Plain),
        "XML drew plain after a Qt translation file compiled it, so the warm \\
         charged the grammar the extension named rather than the one it ran"
    );

    let typescript = highlight_window(&mut frame, &mut highlighter, "ui/app.ts", 0, 1).classes;
    assert!(
        typescript.iter().all(|class| *class == Class::Plain),
        "TypeScript drew coloured off a Qt translation file's warm, so its \\
         budget was spent on XML's work and a real TypeScript file will pay the \\
         compile on a frame the reader is waiting on"
    );
}

/// A Qt `.ts` translation file: TypeScript by extension, XML by first line.
const QT_TRANSLATION: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE TS>
<TS version="2.1" language="pt_BR">
  <context>
    <name>MainWindow</name>
    <message>
      <source>Watch</source>
      <translation>Vigia</translation>
    </message>
  </context>
</TS>
"#;

const TYPESCRIPT: &str = r#"export interface Watcher {
  readonly root: string;
  start(): Promise<void>;
}

export class Pane implements Watcher {
  constructor(public readonly root: string) {}
  async start(): Promise<void> {
    const rows = [1, 2, 3].map((n) => n * 2);
    console.log(`rows: ${rows.length}`);
  }
}
"#;

const XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config name="vigia">
  <pane width="80" height="24"/>
  <!-- a comment -->
  <theme>dark</theme>
</config>
"#;

/// A hunk that scrolled away while it was plain comes back in colour.
#[test]
fn a_hunk_recovered_after_its_warm_comes_back_coloured() {
    let scratch = Scratch::large_diff("warm-recovered", 2, SCREENFUL / 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();

    let plain = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
    assert!(
        plain.iter().all(|class| *class == Class::Plain),
        "the first pass was already coloured, so nothing is being deferred and \
         the recovery below proves nothing"
    );

    let wanted = highlighter.wanted().to_vec();
    highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");

    // The pass that retires it: `mod_0` is not claimed here, so the sweep in
    // `Pass::drop` moves it out of `entries` and into the retired queue.
    let _ = highlight_window(&mut frame, &mut highlighter, "src/mod_1.rs", 0, 1).classes;

    let recovered = highlight_window(&mut frame, &mut highlighter, "src/mod_0.rs", 0, 1).classes;
    assert!(
        recovered.iter().any(|class| *class != Class::Plain),
        "a hunk recovered from the retired queue kept the plain spans it was \
         retired with, so a reader who scrolled away and back is the one reader \
         whose screen never gains its colour"
    );
    assert!(
        highlighter.wanted().is_empty(),
        "the recovered hunk asked for a warm it has already been given"
    );
}
