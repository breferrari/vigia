//! What a painted content row costs the allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{App, Glyphs, Pointing, Row, Theme, View, render};
use vigia_core::{LineKind, Standing};

/// Counts every allocation, so a gate can read what a paint asked for.
struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

// SAFETY: every call forwards to `System` unchanged; the count is a side effect.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is `System`'s.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations one paint of `rows` unhighlighted content rows asks for, per row,
/// read as the difference between two row counts so the frame's fixed cost drops out.
#[test]
fn row_allocations() {
    let chrome = App::new().chrome(
        "fixture",
        None,
        vigia::Stood {
            standing: &Standing::Current,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let paint = |rows: u32| {
        let view = View {
            rows: (1..=rows)
                .map(|number| Row::Line {
                    kind: LineKind::Added,
                    number,
                    text: "let value = 1;".to_owned(),
                    spans: Vec::new(),
                    emph: Vec::new(),
                })
                .collect(),
            files: 1,
            ..View::default()
        };
        let area = Rect::new(0, 0, 80, 60);
        let theme = Theme::default();
        let mut buf = Buffer::empty(area);
        let before = ALLOCS.load(Ordering::Relaxed);
        render(&mut buf, area, &view, &theme, Glyphs::default(), &chrome);
        ALLOCS.load(Ordering::Relaxed) - before
    };
    paint(10);
    let per_row = (paint(50) - paint(10)) / 40;
    eprintln!("allocations per content row: {per_row}");
    assert!(
        per_row <= 3,
        "a content row costs {per_row} allocations to paint"
    );
}
