//! The watch and diff engine behind `vigia`.
//!
//! Everything that can be proven without a terminal: opening a repository,
//! streaming working-tree-vs-index changes, and turning them into hunks. No
//! `ratatui` and no terminal I/O, which is what makes every invariant in
//! `SPEC.md` except I6 and I8 testable headlessly.
//!
//! [`Worktree::changes`] and [`Worktree::diff`] are the primitives and
//! recompute everything they are asked for. A monitor redrawing on every
//! filesystem event drives a [`Frame`] instead, which keeps the previous
//! frame's answers and revalidates them (I2a). [`History`] is the one thing
//! here that outlives the diff, bounded by I10.
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
mod emphasis;
mod error;
mod filter;
mod frame;
mod highlight;
mod history;
mod hunk;
mod notes;
mod timing;
mod watch;
mod worktree;

pub use change::{ChangeKind, FileChange, Origin};
pub use emphasis::{Emphasis, mark};
pub use error::{Error, Result};
pub use frame::{Frame, FrameStats};
pub use highlight::{
    CHECKPOINT_STRIDE, Class, HighlightStats, Highlighter, Pass, RETAINED_HUNKS, Span, WARM_BYTES,
    WARM_FILES, WARM_LEADING, WARM_PER_GRAMMAR, WARM_TOTAL, WarmReport, Warmed,
};
pub use history::{
    Churn, HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_SAMPLE, HISTORY_SAMPLES,
    HISTORY_WINDOW, History, HistoryStats, PULSE_SAMPLES, Recency, SPARK_GROUPS, scale_of,
};
pub use hunk::{CONTEXT, FileDiff, FileSpan, Hunk, Line, LineKind};
pub use notes::{Listing, NEAR, Note, Placement, Side, Status, Store, StoreWatch, key, resolve};
pub use timing::{FrameTiming, Samples};
pub use watch::{Stop, Tick, WatchOptions, WatchStats, Watcher};
pub use worktree::{
    ChangeOptions, Changes, INDEXED_EXTENSION, INDEXED_EXTENSIONS, INDEXED_PATH, Indexed, Worktree,
    indexed_extensions,
};
