//! `vigia [path]` — watch a working tree's diff until you stop looking.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use vigia::{Request, VERSION};

fn main() -> ExitCode {
    // Everything after the program name, so the classifier can see how *many*
    // there are. Passing it one argument is what let `vigia . --colour=never`
    // run: a function handed a single argument cannot notice a second.
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // The classification is `vigia::request_for` rather than a test on these strings,
    // because the surface it decides is one B6 rules on and `SPEC.md` §7 wants it where
    // a test can reach it.
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
const USAGE: &str = "It takes one optional path, and --version (or -V) is the \
                     only option. `vigia .` watches this tree.";
