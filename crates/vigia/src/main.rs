//! `vigia [path]` — watch a working tree's diff until you stop looking.
//!
//! Everything is in the library beside this file; `SPEC.md` §7 makes the
//! snapshot suite the proof for I5 and I6, and a test cannot *import* a
//! `main.rs`. That is why the argument is classified by [`vigia::request_for`]
//! rather than here: this file holds the dispatch and none of the decisions.
//!
//! **It is not unreachable, though, and saying so was wrong.** An earlier
//! version of this note called it "the one place in the crate no gate can
//! reach", which confuses *cannot be imported* with *cannot be tested*: cargo
//! sets `CARGO_BIN_EXE_vigia` for integration tests, so the built executable can
//! be run like any other program. `tests/cli.rs` does exactly that, and the
//! difference is not academic, because what only this file decides is which
//! stream each answer goes to and what the process exits with, which is the part
//! a packaging script and `RELEASE-SMOKE.md` §2 actually observe.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use vigia::{Request, VERSION};

fn main() -> ExitCode {
    // Everything after the program name, so the classifier can see how *many*
    // there are. Passing it one argument is what let `vigia . --colour=never`
    // run: a function handed a single argument cannot notice a second.
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // The classification is `vigia::request_for` rather than a test on these
    // strings, because the surface it decides is one B6 rules on and `SPEC.md`
    // §7 wants it where a test can reach it. Saying "no such option" plainly is
    // worth the arm: without it `vigia --colour=never` reports "not a git
    // repository: --colour=never", which reads as a bug in the path handling
    // rather than as an answer.
    match vigia::request_for(&args) {
        // Before anything else happens. No terminal is taken, no repository is
        // opened, and nothing is watched, which is exactly what makes this not
        // the kind of flag B6 forbids.
        Request::Version => {
            println!("vigia {VERSION}");
            ExitCode::SUCCESS
        }
        Request::NoSuchOption => {
            eprintln!("vigia: no such option. {USAGE}");
            ExitCode::FAILURE
        }
        Request::TooManyArguments => {
            eprintln!("vigia: got {} arguments. {USAGE}", args.len());
            ExitCode::FAILURE
        }
        // The default path lives here, with the other dispatch, because
        // `request_for` answers what was *asked* and an absent argument is not a
        // different question.
        Request::Watch => {
            let path = args.first().map_or(Path::new("."), Path::new);
            match vigia::run(path) {
                Ok(()) => ExitCode::SUCCESS,
                // The terminal is already restored: the session guard is inside
                // `run` and drops before this line.
                Err(e) => {
                    eprintln!("vigia: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// The one-line surface description both refusals end with.
///
/// Shared so the two can never drift into describing different tools, which is
/// the failure a reader meets at exactly the moment they are already confused.
/// Every accepted spelling is named, `-V` included, because a refusal that
/// describes a smaller surface than the tool has teaches the reader something
/// false at the moment they are already stuck.
/// `tests/cli.rs::the_refusal_names_every_spelling_the_classifier_accepts`
/// keeps this in step with the classifier, which lives in another file.
const USAGE: &str = "It takes one optional path, and --version (or -V) is the \
                     only option. `vigia .` watches this tree.";
