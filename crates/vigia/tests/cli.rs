//! The whole CLI surface, which is one positional path and one flag.

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
#[test]
fn a_version_query_is_answered_rather_than_refused() {
    assert_eq!(request("--version"), Request::Version);
    assert_eq!(request("-V"), Request::Version);
}

/// Everything else beginning with `-` is still refused, which is the half of the
/// old ruling that survived the amendment intact.
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
#[test]
fn the_reported_version_is_never_the_placeholder() {
    assert_ne!(
        VERSION, "0.0.0",
        "the workspace version is still the placeholder; a release cut here \
         would claim a crates.io version that can never be withdrawn"
    );

    // The pre-release suffix is cut before the split, not accommodated inside it.
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
#[test]
fn the_binary_refuses_an_unknown_option_and_exits_non_zero() {
    let (code, stdout, stderr) = run_binary(&["--colour=never"]);
    assert_eq!(code, Some(1), "an unknown option should exit 1");
    assert!(
        stdout.is_empty(),
        "the refusal reached stdout ({stdout:?}), which is where a version \
         query answers"
    );
    // The property, not the sentence: it must name the option that does exist and say
    // that a path is what it takes.
    assert!(
        stderr.contains("--version") && stderr.contains("path"),
        "the refusal should name the surface, got {stderr:?}"
    );
}

/// The binary refuses a second argument, on stderr, non-zero, without drawing.
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
