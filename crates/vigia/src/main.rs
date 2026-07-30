//! `vigia [path]` — watch a working tree's diff until you stop looking.
//!
//! Everything is in the library beside this file; `SPEC.md` §7 makes the
//! snapshot suite the proof for I5 and I6, and a test cannot import a `main.rs`.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arg = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| OsString::from("."));

    // `SPEC.md` names no flags, so there are none. Saying that plainly is worth
    // three lines: without it `vigia --help` reports "not a git repository:
    // --help", which reads as a bug in the path handling rather than an answer.
    if arg.to_string_lossy().starts_with('-') {
        eprintln!("vigia: takes a path and no options; `vigia .` watches this tree");
        return ExitCode::FAILURE;
    }

    match vigia::run(Path::new(&arg)) {
        Ok(()) => ExitCode::SUCCESS,
        // The terminal is already restored: the session guard is inside `run`
        // and drops before this line.
        Err(e) => {
            eprintln!("vigia: {e}");
            ExitCode::FAILURE
        }
    }
}
