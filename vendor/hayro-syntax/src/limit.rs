//! A limit on how many bytes stream decoding may produce on one thread.
//!
//! Decoding holds each stream's output whole, and a small file can name a
//! stream that decodes to far more memory than the file's size. With a
//! limit set, a decode that would pass it fails as an invalid stream does.

use alloc::vec::Vec;

#[cfg(feature = "std")]
std::thread_local! {
    static LEFT: core::cell::Cell<Option<u64>> = const { core::cell::Cell::new(None) };
    static PASSED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// A decode inside [`with_decode_limit`] would have passed its limit, and
/// failed as an invalid stream does.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimitExceeded;

/// Run `work` with at most `bytes` of decoded stream data allowed on this
/// thread, counted across every stream it decodes. An error when any decode
/// inside it would have passed the limit, since the document then reads as if
/// that stream were missing. Inside another limit, the smaller of the two
/// holds, and what `work` decodes is spent from the outer one too.
#[cfg(feature = "std")]
pub fn with_decode_limit<T>(
    bytes: u64,
    work: impl FnOnce() -> T,
) -> Result<T, DecodeLimitExceeded> {
    struct Restore {
        outer: Option<u64>,
        passed: bool,
        start: u64,
        clamped: bool,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            let end = LEFT.with(core::cell::Cell::get).unwrap_or(0);
            let spent = self.start - end;
            let inner_passed = PASSED.with(core::cell::Cell::get);
            LEFT.with(|left| left.set(self.outer.map(|outer| outer - spent)));
            PASSED.with(|passed| passed.set(self.passed || (self.clamped && inner_passed)));
        }
    }
    let outer = LEFT.with(core::cell::Cell::get);
    let start = outer.map_or(bytes, |outer| outer.min(bytes));
    let _restore = Restore {
        outer,
        passed: PASSED.with(|passed| passed.replace(false)),
        start,
        clamped: outer.is_some_and(|outer| outer < bytes),
    };
    LEFT.with(|left| left.set(Some(start)));
    let done = work();
    if PASSED.with(core::cell::Cell::get) {
        Err(DecodeLimitExceeded)
    } else {
        Ok(done)
    }
}

/// Inflate raw deflate `data` with the fallback inflater under a limit of `bytes`: whether
/// it stopped at the limit, and the most capacity its output reached. For
/// tests of the limit.
#[doc(hidden)]
#[cfg(feature = "std")]
pub fn fallback_capacity(bytes: u64, data: &[u8]) -> (bool, usize) {
    let mut seen = (false, 0);
    let _ = with_decode_limit(bytes, || seen = crate::filter::lzw_flate::flate::capacity(data));
    seen
}

/// Note that a decode would have passed the limit.
fn passed() {
    #[cfg(feature = "std")]
    PASSED.with(|passed| passed.set(true));
}

/// What may still be decoded, or `None` with no limit set.
pub(crate) fn left() -> Option<u64> {
    #[cfg(feature = "std")]
    return LEFT.with(core::cell::Cell::get);
    #[cfg(not(feature = "std"))]
    return None;
}

/// Whether `bytes` more may be decoded, without spending them.
pub(crate) fn admits(bytes: u64) -> bool {
    let admitted = left().is_none_or(|left| bytes <= left);
    if !admitted {
        passed();
    }
    admitted
}

/// Spend `bytes` of the limit, or say it would pass it.
pub(crate) fn spend(bytes: u64) -> bool {
    #[cfg(feature = "std")]
    return LEFT.with(|left| match left.get() {
        None => true,
        Some(have) if bytes <= have => {
            left.set(Some(have - bytes));
            true
        }
        Some(_) => {
            left.set(Some(0));
            passed();
            false
        }
    });
    #[cfg(not(feature = "std"))]
    {
        let _ = bytes;
        true
    }
}

/// Make room in `out` for `more` bytes, or say they would pass the limit.
/// Capacity grows only to powers of two and never past what is left, so the
/// largest buffer a growth copies from is a power of two under the limit: at
/// most half of it, when the limit is a power of two.
pub(crate) fn room(out: &mut Vec<u8>, more: usize) -> bool {
    let need = out.len().saturating_add(more);
    if !admits(need as u64) {
        return false;
    }
    if need > out.capacity() {
        let mut capacity = need.checked_next_power_of_two().unwrap_or(need);
        if let Some(left) = left() {
            capacity = capacity.min(usize::try_from(left).unwrap_or(usize::MAX));
        }
        out.reserve_exact(capacity - out.len());
    }
    true
}
