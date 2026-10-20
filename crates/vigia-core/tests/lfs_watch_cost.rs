//! What watching an LFS store outside the worktree costs on Linux.

mod support;

#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use support::Scratch;
#[cfg(target_os = "linux")]
use vigia_core::WatchOptions;

/// inotify watches this process holds, from `/proc/self/fdinfo`.
#[cfg(target_os = "linux")]
fn inotify_watches() -> usize {
    std::fs::read_dir("/proc/self/fdinfo")
        .expect("fdinfo")
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .map(|info| {
            info.lines()
                .filter(|line| line.starts_with("inotify wd:"))
                .count()
        })
        .sum()
}

/// What an out-of-tree store of 5,000 objects costs to watch: one inotify watch
/// per directory in it, and the time to arm them. CI's numbers go to the job
/// summary. Alone in its binary, since the count is the whole process's.
#[cfg(target_os = "linux")]
#[test]
fn lfs_store_watch_cost() {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    let holder = support::TempDir::new("watch-lfs-cost");
    let store = holder.path().join("store");
    let hash =
        |n: u64, seed: u64| BuildHasherDefault::<DefaultHasher>::default().hash_one((n, seed));
    let mut dirs = std::collections::BTreeSet::new();
    for n in 0..5_000 {
        let oid: String = (0..4)
            .map(|seed| format!("{:016x}", hash(n, seed)))
            .collect();
        let dir = store.join("objects").join(&oid[0..2]).join(&oid[2..4]);
        std::fs::create_dir_all(&dir).expect("fan-out");
        std::fs::write(dir.join(&oid), "x").expect("object");
        dirs.insert(dir.clone());
        dirs.insert(dir.parent().expect("first level").to_path_buf());
    }
    let directories = dirs.len() + 1;
    let scratch = Scratch::new("watch-lfs-cost-repo");
    scratch.write("a.txt", "x\n");
    scratch.write(".gitattributes", "*.png filter=lfs\n");
    scratch.commit_all("initial");
    let scratch = scratch.settled();
    let worktree = scratch.worktree();
    let started = Instant::now();
    let control = worktree.watch(WatchOptions::default()).expect("watch");
    let control_time = started.elapsed();
    let control_watches = inotify_watches();
    drop(control);

    scratch.git(&["config", "lfs.storage", &store.to_string_lossy()]);
    let worktree = scratch.worktree();
    let started = Instant::now();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");
    // The store arms on a thread of its own; the arm ends when it holds one.
    while watcher.store_watched().is_empty() {
        if started.elapsed() > Duration::from_secs(60) {
            // A failed arm ticks, and only the tick sets the notice.
            let _ = watcher.next_tick();
            panic!("the store never armed: {:?}", watcher.take_store_notice());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let time = started.elapsed();
    let watches = inotify_watches().saturating_sub(control_watches);
    let line = format!(
        "lfs_store_watch_cost: 5000 objects, {directories} directories, {watches} store watches, \
         arm {:?} against {control_time:?} without the store\n",
        time.saturating_sub(control_time)
    );
    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .open(summary)
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }
    assert_eq!(watches, directories, "{line}");
}
