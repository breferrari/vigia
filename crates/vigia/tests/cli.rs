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

use vigia::{Request, VERSION, request_for};

fn request(arg: &str) -> Request {
    request_for(&OsString::from(arg))
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
        "C:\\Dev\\vigia",
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

/// A non-UTF-8 argument is classified from the bytes the user actually typed.
///
/// The defect this exists against is `to_string_lossy`, which is what the
/// refusal used before [#12](https://github.com/breferrari/vigia/issues/12):
/// it replaces what it cannot decode with `U+FFFD`, so the classification would
/// be made from a string that is not the argument. Here the *first byte* is
/// already undecodable, which is the case where a lossy conversion and the raw
/// bytes disagree about what the argument starts with.
///
/// Constructed per platform because there is no portable way to spell an invalid
/// argument: on Unix an `OsString` is bytes and any byte is allowed, and on
/// Windows it is UTF-16 where an unpaired surrogate is the equivalent hole.
/// Neither is exotic. A repository checked out under a name a different locale
/// wrote reaches this path on the first argument.
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

    assert_eq!(request_for(&arg), Request::Watch);
}

/// The version a release reports is never the placeholder the workspace sits at
/// between releases.
///
/// **This is the gate with teeth in this file.** `SPEC.md` §9 makes the git tag
/// the one irreversible event in the release: it publishes to crates.io, where a
/// version cannot be withdrawn. A binary that went out reporting `0.0.0` could
/// not be corrected, only superseded, and nothing else in the repository would
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

    let parts: Vec<&str> = VERSION.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "expected three dotted components, got {VERSION}"
    );
    // The pre-release suffix rides on the patch component (`0.1.0-rc.1`), so
    // only the leading digits of the last one are required to be numeric.
    for (i, part) in parts.iter().enumerate() {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        assert!(
            !digits.is_empty(),
            "component {i} of {VERSION} does not begin with a number"
        );
    }

    assert!(
        parts.iter().any(|p| !p.starts_with('0')),
        "every component of {VERSION} is zero, which is the placeholder wearing \
         a different spelling"
    );
}
