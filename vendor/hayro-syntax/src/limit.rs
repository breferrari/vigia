//! A limit on how many bytes stream decoding may produce on one thread.
//!
//! Decoding holds each stream's output whole, and a small file can name a
//! stream that decodes to far more memory than the file's size. With a
//! limit set, a decode that would pass it fails as an invalid stream does.

#[cfg(feature = "std")]
std::thread_local! {
    static LEFT: core::cell::Cell<Option<u64>> = const { core::cell::Cell::new(None) };
    static PASSED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// A decode inside [`with_decode_limit`] would have passed its limit, and
/// failed as an invalid stream does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimitExceeded;

/// Run `work` with at most `bytes` of decoded stream data allowed on this
/// thread, counted across every stream it decodes, and restore the limit
/// that was set before, if any. An error when any decode inside it would
/// have passed the limit, since the document then reads as if that stream
/// were missing.
#[cfg(feature = "std")]
pub fn with_decode_limit<T>(
    bytes: u64,
    work: impl FnOnce() -> T,
) -> Result<T, DecodeLimitExceeded> {
    struct Restore(Option<u64>, bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            LEFT.with(|left| left.set(self.0));
            PASSED.with(|passed| passed.set(self.1));
        }
    }
    let _restore = Restore(
        LEFT.with(|left| left.replace(Some(bytes))),
        PASSED.with(|passed| passed.replace(false)),
    );
    let done = work();
    if PASSED.with(core::cell::Cell::get) {
        Err(DecodeLimitExceeded)
    } else {
        Ok(done)
    }
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
