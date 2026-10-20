//! The ceiling on a wait for something to happen. A copy of
//! `vigia-core/tests/support/ceiling.rs`, so the pty and update tests stay off
//! the list of tests that read outside this package.

use std::time::Duration;

/// `default`, or less when `VIGIA_TEST_CEILING` (seconds) is lower. A mutation
/// run sets it low, since there a wait that never ends is the kill. It never
/// raises a wait.
pub fn ceiling(default: Duration) -> Duration {
    ceiling_of(default, std::env::var("VIGIA_TEST_CEILING").ok().as_deref())
}

/// [`ceiling`] with the variable's value passed in.
pub fn ceiling_of(default: Duration, value: Option<&str>) -> Duration {
    value
        .and_then(|secs| secs.parse().ok())
        .map_or(default, |secs| default.min(Duration::from_secs(secs)))
}
