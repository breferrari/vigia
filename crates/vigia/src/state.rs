//! Where the shell keeps what outlives a process: the notes store
//! (`SPEC.md` §11.2 B21).

use std::path::{Path, PathBuf};

use vigia_core::{Result, Store};

use crate::theme::home_file;

/// The store for the worktree at `workdir` under the reader's state root, as
/// the pane and the server both open it, or `None` with no home to keep it
/// under.
pub fn store_for(workdir: &Path, lookup: impl Fn(&str) -> Option<String>) -> Option<Result<Store>> {
    state_root(cfg!(windows), lookup).map(|root| Store::open(&root, workdir))
}

/// The sentence for a store with no home, naming what would give it one.
#[must_use]
pub fn no_home() -> String {
    let variables = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME or XDG_STATE_HOME"
    };
    format!("no home to keep a note in: set {variables}")
}

/// The directory `vigia` keeps state under, or `None` when the environment
/// names no home.
///
/// `XDG_STATE_HOME` when it is set, non-empty and absolute, else
/// `~/.local/state`, which is the XDG base directory rule; `LOCALAPPDATA` on
/// Windows, where there is no XDG and that is the per-user local root. The
/// platform and the environment are parameters so a test can ask about both
/// without touching the process environment, as every other lookup here does.
#[must_use]
pub fn state_root(windows: bool, lookup: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    if windows {
        return set("LOCALAPPDATA").map(|local| Path::new(&local).join("vigia").join("state"));
    }
    if let Some(xdg) = set("XDG_STATE_HOME").filter(|value| Path::new(value).is_absolute()) {
        return Some(Path::new(&xdg).join("vigia"));
    }
    home_file(".local/state/vigia", &lookup)
}
