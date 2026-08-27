//! This process's own resident set size, cheaply enough to read every frame.

/// Resident set size of this process in bytes, or `None` where there is no way
/// to ask that is cheap enough to ask every frame.
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

#[cfg(test)]
mod tests {
    //! Beside the code rather than in `tests/`, the way `app.rs` and `view.rs`
    //! keep theirs: this needs no repository, no terminal and no fixture.

    use super::*;

    #[test]
    fn a_process_reads_its_own_resident_set_size() {
        let bytes = resident().expect(
            "no reader for this platform. Every tier-1 target has one; if this \
             fires on Linux, macOS or Windows the readout has silently stopped \
             being drawn rather than being drawn wrongly",
        );
        // A floor and a ceiling rather than an exact number, because the value is
        // a real measurement of a real process and nothing here can predict it.
        // What they catch is the failure that matters: a struct the call never
        // filled reads as stack garbage, which lands outside these by orders of
        // magnitude far more often than it lands inside them.
        assert!(
            (1 << 20..1 << 36).contains(&bytes),
            "resident set size is {bytes} bytes, which is not a number a test \
             process has. An uninitialised read looks exactly like this"
        );
    }

    #[test]
    fn the_number_follows_the_process_rather_than_standing_still() {
        // **The half a range check cannot do**, and the reason it is worth its
        // own test: a reader hard-wired to a plausible constant passes the gate
        // above forever. The vault's note on reading RSS makes the same point
        // about the `tasklist` parse, which was trusted only once an injected
        // leak produced a monotonic ramp rather than a flat line.
        const GROWTH: usize = 64 << 20;
        let before = resident().expect("a reader");

        let mut ballast = vec![0u8; GROWTH];
        for page in ballast.chunks_mut(4096) {
            page[0] = 1;
        }
        let after = resident().expect("a reader");

        // A quarter of what was touched, which is deliberately loose. The point
        // is to separate "tracks reality" from "is a constant", and a tight
        // bound would instead be measuring the allocator on three platforms.
        assert!(
            after > before + (GROWTH / 4) as u64,
            "touching {GROWTH} bytes moved the reading from {before} to {after}, \
             which is not enough to say the number follows the process"
        );
        // Kept alive across the second read, or the optimiser is free to drop it
        // before the measurement it exists for.
        std::hint::black_box(&ballast);
    }
}
