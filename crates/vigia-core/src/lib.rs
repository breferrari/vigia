//! The watch and diff engine behind `vigia`.
//!
//! This crate holds everything that can be proven without a terminal: opening
//! a repository, streaming working-tree-vs-index changes, and turning them
//! into hunks. There is no `ratatui` here and no terminal I/O, which is what
//! makes every invariant in `SPEC.md` except I6 and I8 testable headlessly.
//!
//! The shape of the API is itself a commitment to the spec. Changes arrive as
//! an iterator rather than a collection because I4 makes first paint a budget,
//! and content is fetched per file rather than up front because a monitor must
//! be able to draw the top of a large diff without having read the bottom.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let worktree = vigia_core::Worktree::discover(".")?;
//! for change in worktree.changes()? {
//!     let change = change?;
//!     let diff = worktree.diff(&change)?;
//!     println!("{} +{} -{}", diff.path, diff.added, diff.removed);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod change;
mod error;
mod hunk;
mod timing;
mod watch;
mod worktree;

pub use change::{ChangeKind, FileChange};
pub use error::{Error, Result};
pub use hunk::{CONTEXT, FileDiff, Hunk, Line, LineKind};
pub use timing::{FrameTiming, Samples};
pub use watch::{Stop, Tick, WatchOptions, WatchStats, Watcher};
pub use worktree::{ChangeOptions, Changes, Worktree};
