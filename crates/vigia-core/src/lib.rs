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
//! [`Worktree::changes`] and [`Worktree::diff`] below are the primitives, and
//! they recompute everything they are asked for. A monitor redrawing on every
//! filesystem event must not, so it drives a [`Frame`] instead: the same two
//! calls with the previous frame's answers kept and revalidated, which is I2a.
//!
//! A `Frame` keeps the per-file laziness that I4 needs and gives up the
//! streaming *file list*, because it walks status to completion before reporting
//! one. That is a deliberate trade rather than an oversight: rename tracking is
//! on by default and cannot stream either, so today it costs nothing, and a
//! scrollbar needs the file count anyway. `SPEC.md` §10 tracks it.
//!
//! [`History`] is the one thing here that deliberately outlives the diff. A
//! `Frame` forgets a path the moment it stops being changed, which is how I3 is
//! argued; a churn sparkline is about what was hot a minute ago, so it must not.
//! I10 is what keeps "must not forget" from becoming "never forgets".
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
mod filter;
mod frame;
mod highlight;
mod history;
mod hunk;
mod timing;
mod watch;
mod worktree;

pub use change::{ChangeKind, FileChange};
pub use error::{Error, Result};
pub use frame::{Frame, FrameStats};
pub use highlight::{
    CHECKPOINT_STRIDE, Class, HighlightStats, Highlighter, Pass, RETAINED_HUNKS, Span,
};
pub use history::{
    HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_WINDOW, History, HistoryStats, Recency,
};
pub use hunk::{CONTEXT, FileDiff, FileSpan, Hunk, Line, LineKind};
pub use timing::{FrameTiming, Samples};
pub use watch::{Stop, Tick, WatchOptions, WatchStats, Watcher};
pub use worktree::{ChangeOptions, Changes, Worktree};
