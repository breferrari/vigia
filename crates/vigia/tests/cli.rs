//! The whole CLI surface, which is one positional path and one flag.
//!
//! `SPEC.md` §11 B6 rules that the CLI gains no flags **that configure it**, and
//! §11.1 spells out what is left: a path, `--version`, `-V`, and a refusal for
//! everything else beginning with `-`. That is a small enough surface to assert
//! exhaustively, and it is worth asserting because it is the only part of this
//! program a user meets before the terminal is taken.
//!
//! An integration test rather than a unit one, for the reason `main.rs`'s own
//! docblock gives: the classification lives in the library precisely so a test
//! can reach it, and reaching it the way an external consumer does is what
//! proves the export exists rather than that the function does.

use std::ffi::OsString;
use std::process::Command;

use vigia::{Request, VERSION, request_for};

fn request(arg: &str) -> Request {
    request_for(&[OsString::from(arg)])
}

fn request_all(args: &[&str]) -> Request {
    let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
    request_for(&owned)
}

/// Run the real binary with these arguments and return (status, stdout, stderr).
///
/// `CARGO_BIN_EXE_<name>` is set by cargo for integration tests of a package
/// that declares a binary, so the built executable is on disk at a known path
/// and needs no building here.
///
/// Takes a slice rather than one argument, because the arity refusal is one of
/// the three arms and a one-argument helper cannot reach it, which is how the
/// arm stays unexercised while its classifier is gated.
fn run_binary(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_vigia"))
        .args(args)
        .output()
        .expect("the built binary runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Both conventional spellings answer rather than refuse.
///
/// The amendment `SPEC.md` §11 records, and the reason it is not the thing B6
/// forbids: this prints a line and exits before a terminal is taken, so there is
/// no frame it can change and no state it can leave.
///
/// **Both spellings, not one.** A user who tries `--version` tries `-V`, and a
/// surface that accepts exactly one of the two conventional forms is worse than
/// one that accepts neither, because it teaches a rule that is wrong half the
/// time. `-v` is deliberately **not** among them and is asserted below: lower
/// case `-v` is `--verbose` far more often than it is a version query, and
/// guessing wrong there would silently start watching a path that does not
/// exist.
#[test]
fn a_version_query_is_answered_rather_than_refused() {
    assert_eq!(request("--version"), Request::Version);
    assert_eq!(request("-V"), Request::Version);
}

/// Everything else beginning with `-` is still refused, which is the half of the
/// old ruling that survived the amendment intact.
///
/// `--help` is in this list on purpose rather than by omission. §11.1 leaves the
/// question of implementing it open, and until it is answered `--help` is an
/// option that does not exist, so it gets the same one line as any other. The
/// near-misses are the load-bearing cases: `--Version`, `-v` and a typo differ
/// from an accepted spelling by one character each, and a classifier that
/// reached for `starts_with` or a case-insensitive compare would let at least
/// one of them through while every obvious case still passed.
#[test]
fn an_option_that_is_not_a_version_query_is_still_refused() {
    for arg in [
        "--help",
        "-h",
        "-x",
        "--verison",
        "--version=1",
        "--versions",
        "--Version",
        "-v",
        "-VV",
        "--",
        "-",
    ] {
        assert_eq!(
            request(arg),
            Request::NoSuchOption,
            "{arg} should be refused, not taken as a path or a version query"
        );
    }
}

/// A path is a path however it is spelled, including the shapes that look like
/// options and are not.
///
/// The last two matter more than they look. A relative path beginning `./-` and
/// a file literally named `-x` inside a directory both *contain* a dash without
/// leading with one, and the rule is about the first byte rather than about the
/// presence of the character anywhere.
#[test]
fn a_path_is_watched_however_it_is_spelled() {
    for arg in [
        ".",
        "..",
        "/",
        "~/code/some-repo",
        "C:\\code\\some-repo",
        "a path with spaces",
        "./-x",
        "src/-weird-name",
        "",
    ] {
        assert_eq!(
            request(arg),
            Request::Watch,
            "{arg:?} is a path and should be watched"
        );
    }
}

/// A second argument is refused rather than dropped on the floor.
///
/// **The defect this was written against shipped in the first version of this
/// surface.** `main` read `args_os().nth(1)` and passed one argument to a
/// classifier that could not see a second, so `vigia . --colour=never` watched
/// `.` and discarded the flag in silence. That is strictly worse than the
/// refusal a flag gets on its own: the tool starts, draws, and looks like it
/// accepted the option.
///
/// Zero arguments is `Watch`, because §11.1's path is *optional* and `main`
/// supplies the default. That case is here rather than assumed, since it is the
/// most common invocation there is: `vigia` with nothing after it.
#[test]
fn only_one_argument_is_a_surface_and_a_second_is_refused() {
    assert_eq!(
        request_all(&[]),
        Request::Watch,
        "bare `vigia` watches here"
    );
    assert_eq!(request_all(&["."]), Request::Watch);
    assert_eq!(request_all(&["--version"]), Request::Version);

    for args in [
        vec![".", "--colour=never"],
        vec![".", "."],
        vec!["--version", "--colour=never"],
        vec!["--version", "--version"],
        vec![".", "-x", "-y"],
    ] {
        assert_eq!(
            request_all(&args),
            Request::TooManyArguments,
            "{args:?} is more than one argument and must be refused, not \
             silently truncated to its first"
        );
    }
}

/// A path that is not valid Unicode is watched, not refused.
///
/// **`to_string_lossy` is not the defect this guards.** Mutating `request_for`
/// back to the lossy form the refusal took before
/// [#12](https://github.com/breferrari/vigia/issues/12) leaves this
/// test, and the whole file, green: lossy decoding substitutes `U+FFFD`, which
/// is neither `-` nor `--version`, so both spellings classify every input
/// identically. The mutation survived, and a test whose stated purpose a
/// mutation cannot kill is a test claiming something it does not check.
///
/// What it does check is worth keeping, so the claim is narrowed to it: an
/// argument this process cannot decode is still a **path**. The implementation
/// that would fail is the tempting one, `arg.to_str().ok_or(...)`, refusing what
/// it cannot read; that mutation is killed here. `vigia` opens a repository by
/// path and never needs to interpret the bytes, so refusing them would turn a
/// checkout named in another locale's encoding into an unusable directory.
///
/// Constructed per platform because there is no portable way to spell an invalid
/// argument: on Unix an `OsString` is bytes and any byte is allowed, and on
/// Windows it is UTF-16 where an unpaired surrogate is the equivalent hole.
#[test]
fn a_path_that_is_not_valid_unicode_is_still_a_path() {
    #[cfg(unix)]
    let arg: OsString = {
        use std::os::unix::ffi::OsStringExt;
        // 0xFF is not a valid UTF-8 leading byte anywhere.
        OsString::from_vec(vec![0xFF, b'r', b'e', b'p', b'o'])
    };
    #[cfg(windows)]
    let arg: OsString = {
        use std::os::windows::ffi::OsStringExt;
        // A lone high surrogate, which no valid UTF-16 sequence leaves unpaired.
        OsString::from_wide(&[0xD800, b'r' as u16, b'e' as u16, b'p' as u16, b'o' as u16])
    };

    assert_eq!(request_for(&[arg]), Request::Watch);
}

/// The version a release reports is never the placeholder the workspace sits at
/// between releases.
///
/// **This is the gate with teeth in this file.** `SPEC.md` §9 makes the git tag
/// the one irreversible event in the release: it publishes to crates.io, where a
/// version cannot be withdrawn. A binary that went out reporting `0.0.0` could
/// not be corrected, only replaced, and nothing else in the repository would
/// have noticed, because `0.0.0` builds, tests and installs exactly like any
/// other version.
///
/// Three dotted numeric components rather than a full SemVer parse: cargo
/// already rejects a version it cannot parse at manifest-load time, so the only
/// thing left to assert is the part cargo is happy with and a release is not.
#[test]
fn the_reported_version_is_never_the_placeholder() {
    assert_ne!(
        VERSION, "0.0.0",
        "the workspace version is still the placeholder; a release cut here \
         would claim a crates.io version that can never be withdrawn"
    );

    // **The pre-release suffix is cut before the split, not accommodated
    // inside it.** The first version of this asserted three dotted components
    // over the whole string, and a comment below it claimed `0.1.0-rc.1` was
    // covered. It is not: that splits into *four* parts, so the assertion would
    // have gone red on the release candidate before the one publish that cannot
    // be taken back, and the reason would have been this test rather than
    // anything wrong with the version.
    let core = VERSION.split_once('-').map_or(VERSION, |(core, _)| core);

    let parts: Vec<&str> = core.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "expected three dotted components in {core:?} (from {VERSION})"
    );
    for (i, part) in parts.iter().enumerate() {
        assert!(
            part.chars().all(|c| c.is_ascii_digit()),
            "component {i} of {VERSION} is {part:?}, which is not a number"
        );
    }

    assert!(
        parts.iter().any(|p| !p.starts_with('0')),
        "every component of {VERSION} is zero, which is the placeholder wearing \
         a different spelling"
    );
}

/// The binary prints the version on stdout and exits successfully.
///
/// **`main.rs` said no gate could reach it, and that was wrong.** Its docblock
/// claimed to be "the one place in the crate no gate can reach", which is true
/// of the *library* route and false in general: cargo sets `CARGO_BIN_EXE_vigia`
/// for this test binary, so the real executable can simply be run. Everything
/// above this line tests the classifier; nothing tested that `main` wires it to
/// the right stream and the right exit code, and those are the two things
/// `RELEASE-SMOKE.md` §2 and a packager both actually observe.
///
/// **stdout specifically, not "output".** A version query is a thing scripts
/// read, and one that answered on stderr would satisfy a human running it and
/// break `vigia --version | cut -d' ' -f2`. The exact string is asserted for the
/// same reason `SPEC.md` §11.1 specifies it: `vigia <version>` is a format, not
/// a suggestion.
#[test]
fn the_binary_prints_its_version_and_exits_zero() {
    for spelling in ["--version", "-V"] {
        let (code, stdout, stderr) = run_binary(&[spelling]);
        assert_eq!(code, Some(0), "{spelling} should exit 0, stderr: {stderr}");
        assert_eq!(stdout.trim(), format!("vigia {VERSION}"));
        assert!(
            stderr.is_empty(),
            "{spelling} wrote to stderr: {stderr:?}, so a script reading stdout \
             is not the whole contract"
        );
    }
}

/// The binary refuses an unknown option on stderr and exits non-zero.
///
/// The other half of the same wiring, and the half a packaging script notices:
/// an exit code of zero here would make `vigia --frobnicate` look like it
/// worked. stderr rather than stdout for the symmetric reason above, so that a
/// refusal never contaminates output something is parsing.
///
/// It must also refuse *before* taking a terminal, which is what the empty
/// stdout asserts: an alternate-screen sequence on the way to an error message
/// is the failure `SPEC.md` §11.1 rules against for a non-repository path, one
/// argument shape over.
#[test]
fn the_binary_refuses_an_unknown_option_and_exits_non_zero() {
    let (code, stdout, stderr) = run_binary(&["--colour=never"]);
    assert_eq!(code, Some(1), "an unknown option should exit 1");
    assert!(
        stdout.is_empty(),
        "the refusal reached stdout ({stdout:?}), which is where a version \
         query answers"
    );
    // The property, not the sentence: it must name the option that does exist
    // and say that a path is what it takes. Pinning the exact wording reddens
    // on an addition to the usage line, which is a documentation improvement
    // rather than a regression, and a gate that fires on those
    // teaches people to edit the gate instead of reading it.
    assert!(
        stderr.contains("--version") && stderr.contains("path"),
        "the refusal should name the surface, got {stderr:?}"
    );
}

/// The binary refuses a second argument, on stderr, non-zero, without drawing.
///
/// **The third arm, and the one most easily left with no end-to-end gate.**
/// Testing `Request::TooManyArguments`' classifier without running the actual
/// binary with two arguments — which a one-argument helper cannot — leaves the
/// arm that
/// decides the exit code and the stream for this case was the only one of three
/// that no test reached, in a file written to cover the whole surface.
///
/// The empty stdout matters most here. This is the arm reached by
/// `vigia . --colour=never`, which before the refusal existed *started the
/// program*: if it regressed, the tell would be an alternate-screen escape
/// sequence on stdout rather than a wrong exit code.
#[test]
fn the_binary_refuses_a_second_argument_and_exits_non_zero() {
    let (code, stdout, stderr) = run_binary(&[".", "--colour=never"]);
    assert_eq!(code, Some(1), "two arguments should exit 1");
    assert!(
        stdout.is_empty(),
        "the refusal reached stdout ({stdout:?}), which means the program got \
         far enough to draw something"
    );
    assert!(
        stderr.contains("got 2 arguments"),
        "the refusal should say how many it got, so the reader can see that the \
         count is the problem: {stderr:?}"
    );
}

/// The usage line names every spelling the classifier accepts.
///
/// A refusal describing a smaller surface than the tool has teaches the reader
/// something false at the moment they are already stuck. `-V` was accepted and
/// unmentioned, which is the drift this catches: the two live in different
/// files, `USAGE` in `main.rs` and the accepted set in `lib.rs`.
#[test]
fn the_refusal_names_every_spelling_the_classifier_accepts() {
    let (_, _, usage) = run_binary(&["--frobnicate"]);

    for spelling in ["--version", "-V"] {
        assert_eq!(
            request(spelling),
            Request::Version,
            "{spelling} is accepted, so the usage line must mention it"
        );
        assert!(
            usage.contains(spelling),
            "the refusal accepts {spelling} and does not mention it: {usage:?}"
        );
    }
}
