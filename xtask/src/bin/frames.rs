//! `cargo run -p xtask --bin frames`: write every `assets/frame-*.svg` from the
//! snapshot it draws. Run it after `cargo insta accept` changes one of them.

#[path = "../frames.rs"]
#[allow(dead_code)]
mod frames;

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for frame in frames::FRAMES {
        let source = root.join(frames::SNAPSHOTS).join(frame.snapshot);
        let snapshot = std::fs::read_to_string(&source)
            .unwrap_or_else(|e| panic!("read {}: {e}", source.display()));
        let target = root.join("assets").join(frame.file);
        std::fs::write(&target, frames::draw(frame, &snapshot))
            .unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
        println!("{}", frame.file);
    }
}
