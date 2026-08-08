//! `vigia [path]` — watch a working tree's diff until you stop looking.
//!
//! Everything is in the library beside this file; `SPEC.md` §7 makes the
//! snapshot suite the proof for I5 and I6, and a test cannot import a `main.rs`.
//! That is why the argument is *classified* by [`vigia::request_for`] rather
//! than here: this file is the one place in the crate no gate can reach, so it
//! holds the dispatch and none of the decisions.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use vigia::{Request, VERSION};

fn main() -> ExitCode {
    let arg = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| OsString::from("."));

    // The classification is `vigia::request_for` rather than a test on this
    // string, because this file cannot be reached by a test (`SPEC.md` §7) and
    // the surface it decides is one B6 rules on. Saying "no such option" plainly
    // is worth the arm: without it `vigia --colour=never` reports "not a git
    // repository: --colour=never", which reads as a bug in the path handling
    // rather than as an answer.
    match vigia::request_for(&arg) {
        // Before anything else happens. No terminal is taken, no repository is
        // opened, and nothing is watched, which is exactly what makes this not
        // the kind of flag B6 forbids.
        Request::Version => {
            println!("vigia {VERSION}");
            ExitCode::SUCCESS
        }
        Request::NoSuchOption => {
            eprintln!(
                "vigia: takes a path; --version is the only option. `vigia .` watches this tree"
            );
            ExitCode::FAILURE
        }
        Request::Watch => match vigia::run(Path::new(&arg)) {
            Ok(()) => ExitCode::SUCCESS,
            // The terminal is already restored: the session guard is inside
            // `run` and drops before this line.
            Err(e) => {
                eprintln!("vigia: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
