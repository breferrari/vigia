//! This process's own resident set size, cheaply enough to read every frame.
//!
//! `SPEC.md` §5.1 draws a memory cell on the status bar, and the thing that had
//! to be settled before it could ship was not what to draw but **what reading it
//! costs**. A monitor's frame budget is 16ms (I9), and a readout that spends a
//! meaningful slice of the budget it sits beside is worse than an absent one.
//!
//! ## Why this is not the reader `soak.rs` had
//!
//! `crates/vigia/tests/soak.rs` answered the same question first, for I3, and
//! reached for the shell: `ps` on macOS and `tasklist` on Windows. That is right
//! *there* and wrong here, and the difference is the sample rate. The soak takes
//! 288 samples across a window measured in hours, so a process spawn is free.
//! This runs once per painted frame.
//!
//! Measured on the reference machine, twelve spawns of
//! `tasklist /FI "PID eq <pid>" /NH /FO CSV`: min 41.4ms, **median 42.8ms**, max
//! 81.2ms. That is **2.7x the whole frame budget** for the read alone, before
//! anything is diffed, highlighted or painted. No sampling interval rescues it
//! either: the frame that pays it is still a visible hitch, and a cached value
//! is stale by an amount the reader cannot see.
//!
//! So the soak keeps its subprocesses and this module does not use them.
//!
//! ## Why it costs no new crate
//!
//! Two of the three answers are `unsafe` FFI, and both go through a crate `gix`
//! already puts in that target's graph: `libc` reaches the Apple graph through
//! `gix-hash` to `sha1-checked` to `cpufeatures`, and `windows-sys` 0.61 reaches
//! the Windows graph through `gix-sec`. `cargo tree -i` says so and the lock
//! diff that added them says so louder: two edges, zero packages. `SPEC.md` §6
//! carries the warning that nearly cost this the Windows half, which is that a
//! dependency's cost is a fact about the lock file rather than something to
//! recall.
//!
//! Linux needs neither. `/proc/self/status` is a file read.
//!
//! ## What it does *not* fix
//!
//! The number is **as of the last change**, not as of now, on every platform.
//! This shell wakes only on a filesystem event, so a pane left open on an idle
//! tree keeps showing whatever was true when the last write landed. Refreshing
//! it would need a wake nothing schedules, which is the timer I1 forbids, and
//! the escape the pulse used against the same wall does not transfer here: a
//! filesystem event names paths and never bytes, so there is no event identity
//! to define a magnitude against. Ruled and accepted in `SPEC.md` §5.1 rather
//! than solved.

/// Resident set size of this process in bytes, or `None` where there is no way
/// to ask that is cheap enough to ask every frame.
///
/// `None` is a real answer rather than a failure: the caller draws nothing, and
/// `SPEC.md` §11.1 makes that safe by keeping the footer's height independent of
/// whether this cell exists. Every tier-1 target answers `Some`; a platform
/// outside those three reports unavailable instead of guessing, for the same
/// reason `soak.rs` prints "unavailable" for file handles off Linux rather than
/// letting two platforms read as covered when they are not.
///
/// One documented entry point over four `#[cfg]` bodies, rather than four
/// `#[cfg]`-ed public functions: the contract above is the same on every
/// platform and is the thing a caller reads, while which syscall answers it is
/// not. Written the other way, three of the four copies are unreachable from any
/// given build's documentation and drift without anyone seeing it.
pub fn resident() -> Option<u64> {
    read()
}

#[cfg(target_os = "linux")]
fn read() -> Option<u64> {
    // `VmRSS` rather than `/proc/self/statm`, which is a shorter read and about
    // 3x cheaper. `statm` reports pages, so it would need `sysconf(_SC_PAGESIZE)`
    // and therefore `unsafe` and `libc` on the one platform that currently needs
    // neither. Hard-coding 4096 is not available: arm64 Linux ships 16K and 64K
    // page sizes. At 0.2% of the frame budget the cheaper read buys nothing worth
    // that, and this is the exact reader `soak.rs` has already driven across
    // 288-sample series.
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[cfg(target_os = "macos")]
fn read() -> Option<u64> {
    // `proc_pidinfo` rather than `task_info(mach_task_self(), ...)`, which is the
    // other route to the same number: this one is a plain function call against a
    // pid, where the mach route needs a task port and gets the type of
    // `mach_task_self` wrong in a way that compiles.
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = size_of::<libc::proc_taskinfo>() as libc::c_int;

    // SAFETY: `proc_pidinfo` writes at most `size` bytes into `buffer` and
    // reports how many it wrote. `size` is derived from the very type the
    // pointer refers to, so the buffer cannot be short, and the pointer is to a
    // live local that outlives the call. The pid is this process's own, which
    // always exists. Nothing here is retained past the call.
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };

    // A partial write is the documented failure mode, and it is **not** signalled
    // by a negative return: the call reports the byte count it managed, so
    // checking only for `> 0` would read an uninitialised struct on a short one.
    if written != size {
        return None;
    }

    // SAFETY: the call reported that it filled exactly `size` bytes, which is the
    // whole struct.
    Some(unsafe { info.assume_init() }.pti_resident_size)
}

#[cfg(windows)]
fn read() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        // The struct is versioned by its own size, which is how the ABI stays
        // compatible across Windows releases. Getting this wrong fails the call
        // rather than corrupting anything, which is why it is set from the type
        // rather than written as a number.
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };

    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no closing
    // and is always valid for the calling process. `counters` is a live local of
    // exactly the type the third argument declares, and `cb` tells the call how
    // large it is, so it cannot write past the end. The call retains nothing.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &raw mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };

    // `WorkingSetSize` rather than `PagefileUsage`, because the working set is
    // what `tasklist` reports and what I3's budget is written against. The two
    // differ by several MiB on the same process, so mixing them would read as
    // drift. `soak.rs` documents the same trap one layer up.
    (ok != 0).then_some(counters.WorkingSetSize as u64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read() -> Option<u64> {
    None
}
