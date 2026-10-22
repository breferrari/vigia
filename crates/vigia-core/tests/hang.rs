//! A test that never ends, to show nextest's slow-timeout kills one.

use std::time::Duration;

#[test]
#[ignore = "hangs on purpose: run it with --run-ignored only to see nextest kill it"]
fn hang_fixture() {
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
