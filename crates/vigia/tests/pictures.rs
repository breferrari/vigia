//! A changed image draws pictures under its size line. `SPEC.md` §11.1.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::io::Cursor;
use std::time::Duration;

use image::{ImageEncoder, ImageFormat, Rgba, RgbaImage};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui_image::picker::{Picker, ProtocolType};
use support::{Scratch, animated_gif, claimed_png, png};
use vigia::pictures::{Inks, Kind, Pictures};
use vigia::{Action, App, Glyphs, Pointing, Row, Theme, View, body_layout, render};
use vigia_core::{Frame, Highlighter, History, Standing};

/// The dark palette's pane and its removed bar.
const INKS: Inks = Inks {
    canvas: (0x0d, 0x11, 0x17),
    changed: (0xf8, 0x51, 0x49),
};

/// What a reader's pane holds: the app, the built pictures, and its size.
struct Pane {
    app: App,
    pictures: Pictures,
    highlighter: Highlighter,
    history: History,
    width: u16,
    height: u16,
}

impl Pane {
    fn new(width: u16, height: u16) -> Self {
        Self::drawn_by(width, height, ProtocolType::Halfblocks)
    }

    /// A pane whose pictures are encoded for `protocol` at a 10x20 cell.
    fn drawn_by(width: u16, height: u16, protocol: ProtocolType) -> Self {
        let mut app = App::new();
        app.draw_pictures(true);
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(protocol);
        Self {
            app,
            pictures: Pictures::new(INKS, picker, Box::new(|| {})),
            highlighter: Highlighter::eager(),
            history: History::new(),
            width,
            height,
        }
    }

    fn chrome(&self) -> vigia::Chrome {
        self.app.chrome(
            "fixture",
            None,
            vigia::Stood {
                standing: &Standing::Current,
                now: 0,
            },
            Pointing::default(),
            Default::default(),
            "",
        )
    }

    fn body(&self, frame: &Frame) -> vigia::Body {
        body_layout(
            Rect::new(0, 0, self.width, self.height),
            &self.chrome(),
            frame.files().len(),
            vigia::list_rows_wanted(frame.files()),
        )
    }

    /// One frame: the view, with the pictures attached as the shell attaches them.
    fn view(&mut self, frame: &mut Frame) -> View {
        let body = self.body(frame);
        let mut view = self
            .app
            .view(frame, &mut self.highlighter, &self.history, body)
            .expect("view");
        self.pictures.attach(&mut view.rows, frame);
        view
    }

    /// A frame after every queued picture is built.
    fn settled(&mut self, frame: &mut Frame) -> View {
        self.view(frame);
        assert!(
            self.pictures.settle(Duration::from_secs(60)),
            "the pictures were not built"
        );
        self.view(frame)
    }

    fn scroll(&mut self, frame: &mut Frame, rows: isize) {
        let height = self.body(frame).diff;
        self.app
            .apply(Action::Scroll(rows), frame, height)
            .expect("scroll");
    }

    fn draw(&self, view: &View) -> TestBackend {
        self.draw_with(view, &self.chrome())
    }

    fn draw_with(&self, view: &View, chrome: &vigia::Chrome) -> TestBackend {
        let mut terminal =
            Terminal::new(TestBackend::new(self.width, self.height)).expect("terminal");
        terminal
            .draw(|f| {
                let area = f.area();
                render(
                    f.buffer_mut(),
                    area,
                    view,
                    &Theme::dark(),
                    Glyphs::default(),
                    chrome,
                );
            })
            .expect("draw");
        terminal.backend().clone()
    }
}

/// Each picture row on screen as `(part, built)`.
fn picture_rows(view: &View) -> Vec<(u16, bool)> {
    view.rows
        .iter()
        .filter_map(|row| match row {
            Row::Picture { part, ready, .. } => Some((*part, ready.is_some())),
            _ => None,
        })
        .collect()
}

/// The size line and what its pictures came to.
fn sized(view: &View) -> Option<(String, Option<String>, Kind, usize)> {
    view.rows.iter().find_map(|row| match row {
        Row::Sized {
            line, key, ready, ..
        } => Some((
            line.clone(),
            ready.as_ref().and_then(|ready| ready.0.suffix()),
            key.shape.kind,
            ready.as_ref().map_or(0, |ready| ready.0.panels.len()),
        )),
        _ => None,
    })
}

fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
    support::encode_image(image, format)
}

/// A screen-like picture: bands of colour and a box in the middle.
fn screen_like(width: u32, height: u32, accent: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let band = (y * 6 / height) as u8;
        if (width / 4..width * 3 / 4).contains(&x) && (height / 3..height / 2).contains(&y) {
            Rgba([accent[0], accent[1], accent[2], 255])
        } else {
            Rgba([200 - band * 20, 210 - band * 10, 220, 255])
        }
    })
}

/// A repository with `path` committed as `old` and rewritten as `new`.
fn changed(name: &str, path: &str, old: &[u8], new: &[u8]) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(path, old);
    scratch.commit_all("baseline");
    scratch.write(path, new);
    scratch
}

#[test]
fn three_panels_drawn() {
    let scratch = changed(
        "pictures-three",
        "screen.png",
        &encode(&screen_like(120, 260, [40, 90, 200]), ImageFormat::Png),
        &encode(&screen_like(120, 260, [200, 90, 40]), ImageFormat::Png),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    let first = pane.view(&mut frame);
    let reserved = picture_rows(&first);
    assert!(!reserved.is_empty(), "no rows were held for the pictures");
    assert!(
        reserved.iter().all(|(_, built)| !built),
        "a picture was ready before anything built it"
    );

    let view = pane.settled(&mut frame);
    assert_eq!(
        picture_rows(&view).len(),
        reserved.len(),
        "the layout moved when the picture arrived"
    );
    assert!(picture_rows(&view).iter().all(|(_, built)| *built));
    let (line, found, kind, panels) = sized(&view).expect("a size line");
    assert!(line.starts_with("PNG 120×260"), "the line reads {line:?}");
    assert_eq!((kind, panels), (Kind::Three, 3));
    assert!(
        found.is_some_and(
            |found| found.ends_with("px changed (6.73%)") || found.contains("px changed")
        ),
        "the line does not say how many pixels changed"
    );
    // Drawn as half-blocks under the line.
    let drawn = pane.draw(&view);
    let halves = drawn
        .buffer()
        .content()
        .iter()
        .filter(|cell| matches!(cell.symbol(), "▀" | "▄"))
        .count();
    assert!(halves > 100, "{halves} half-block cells were drawn");
}

#[test]
fn no_pictures_below_truecolour() {
    let scratch = changed(
        "pictures-no-truecolour",
        "screen.png",
        &png(120, 260, 1),
        &png(120, 260, 2),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    pane.app.draw_pictures(false);

    let view = pane.view(&mut frame);
    assert!(
        picture_rows(&view).is_empty(),
        "pictures drew below truecolour"
    );
    assert!(
        view.rows
            .iter()
            .any(|row| matches!(row, Row::Reason(line) if line.starts_with("PNG"))),
        "the size line is gone"
    );
}

#[test]
fn no_pictures_short() {
    let scratch = changed(
        "pictures-short",
        "screen.png",
        &png(120, 260, 1),
        &png(120, 260, 2),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 9);

    let view = pane.view(&mut frame);
    assert!(
        picture_rows(&view).is_empty(),
        "pictures drew on a 9-row pane"
    );
}

#[test]
fn pixels_unchanged_line() {
    let image = screen_like(64, 64, [40, 90, 200]);
    let fast = {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        )
        .write_image(image.as_raw(), 64, 64, image::ExtendedColorType::Rgba8)
        .expect("encode");
        bytes
    };
    let best = {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Paeth,
        )
        .write_image(image.as_raw(), 64, 64, image::ExtendedColorType::Rgba8)
        .expect("encode");
        bytes
    };
    assert_ne!(fast, best, "the fixture's two encodings are the same bytes");
    let scratch = changed("pictures-unchanged", "icon.png", &fast, &best);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    let view = pane.settled(&mut frame);
    assert_eq!(
        sized(&view).and_then(|(_, found, _, _)| found).as_deref(),
        Some("pixels unchanged")
    );
}

#[test]
fn changed_count_line() {
    let old = screen_like(100, 100, [40, 90, 200]);
    let mut new = old.clone();
    new.put_pixel(50, 10, Rgba([0, 0, 0, 255]));
    let scratch = changed(
        "pictures-count",
        "icon.png",
        &encode(&old, ImageFormat::Png),
        &encode(&new, ImageFormat::Png),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    let view = pane.settled(&mut frame);
    assert_eq!(
        sized(&view).and_then(|(_, found, _, _)| found).as_deref(),
        Some("1 px changed (0.01%)")
    );
}

/// The first frame only, and nothing scheduled after it.
#[test]
fn gif_first_frame() {
    let first = screen_like(40, 40, [40, 90, 200]);
    let gif = |second: &RgbaImage| animated_gif(&[first.clone(), second.clone()]);
    let scratch = changed(
        "pictures-gif",
        "spinner.gif",
        &gif(&screen_like(40, 40, [10, 10, 10])),
        &gif(&screen_like(40, 40, [250, 250, 250])),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    let view = pane.settled(&mut frame);
    assert_eq!(
        sized(&view).and_then(|(_, found, _, _)| found).as_deref(),
        Some("pixels unchanged"),
        "a frame past the first was compared"
    );
    // The built picture itself, not the process-wide decode count, which tests
    // running beside this one move.
    let ready = |view: &View| {
        view.rows.iter().find_map(|row| match row {
            Row::Sized { ready, .. } => ready.clone(),
            _ => None,
        })
    };
    let built = ready(&view).expect("a built picture");
    for _ in 0..5 {
        let again = pane.view(&mut frame);
        assert_eq!(
            ready(&again),
            Some(built.clone()),
            "a settled pane built again"
        );
    }
}

#[test]
fn added_and_deleted_bars() {
    let scratch = Scratch::new("pictures-bars");
    scratch.write("gone.png", png(120, 260, 1));
    scratch.commit_all("baseline");
    scratch.remove("gone.png");
    scratch.write("new.png", png(120, 260, 2));
    scratch.git(&["add", "-N", "new.png"]);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 60);

    let view = pane.settled(&mut frame);
    let kinds: Vec<Kind> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Sized { key, .. } => Some(key.shape.kind),
            _ => None,
        })
        .collect();
    assert!(
        kinds.contains(&Kind::Added) && kinds.contains(&Kind::Deleted),
        "{kinds:?}"
    );
    let drawn = pane.draw(&view);
    let theme = Theme::dark();
    let bars: Vec<_> = drawn
        .buffer()
        .content()
        .iter()
        .filter_map(|cell| {
            (cell.bg == theme.added_bar.bg.expect("a bar"))
                .then_some("added")
                .or((cell.bg == theme.removed_bar.bg.expect("a bar")).then_some("removed"))
        })
        .collect();
    assert!(
        bars.contains(&"added") && bars.contains(&"removed"),
        "the bars were not drawn"
    );
}

/// An image drawn last in a long diff can be scrolled to whole.
#[test]
fn picture_at_end_reachable() {
    let scratch = Scratch::new("pictures-end");
    scratch.write("a.txt", support::numbered_lines(80));
    scratch.write("z.png", png(120, 260, 1));
    scratch.commit_all("baseline");
    scratch.write("a.txt", support::numbered_lines(160));
    scratch.write("z.png", png(120, 260, 2));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    for _ in 0..40 {
        pane.scroll(&mut frame, 10);
        pane.view(&mut frame);
    }

    let view = pane.view(&mut frame);
    let rows = picture_rows(&view);
    let held = view
        .rows
        .iter()
        .find_map(|row| match row {
            Row::Picture { key, .. } => Some(key.shape.rows),
            _ => None,
        })
        .expect("the picture is on screen at the end");
    assert_eq!(
        rows.last().map(|(part, _)| *part),
        Some(held - 1),
        "the picture's last row cannot be reached"
    );
}

/// A picture partly above the window draws its visible rows, as they are in
/// the whole picture.
#[test]
fn picture_partly_off() {
    let scratch = Scratch::new("pictures-partly");
    scratch.write("a.png", png(120, 260, 1));
    scratch.write("z.txt", support::numbered_lines(10));
    scratch.commit_all("baseline");
    scratch.write("a.png", png(120, 260, 2));
    scratch.write("z.txt", support::numbered_lines(13));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    let whole = pane.settled(&mut frame);
    let whole_drawn = pane.draw(&whole);
    let first_at = whole
        .rows
        .iter()
        .position(|row| matches!(row, Row::Picture { part: 0, .. }))
        .expect("the picture is on screen");

    for _ in 0..40 {
        pane.scroll(&mut frame, 10);
        pane.view(&mut frame);
    }
    let scrolled = pane.settled(&mut frame);
    let (at, part) = scrolled
        .rows
        .iter()
        .enumerate()
        .find_map(|(at, row)| match row {
            Row::Picture { part, .. } => Some((at, *part)),
            _ => None,
        })
        .expect("part of the picture is still on screen");
    assert!(part > 0, "the window did not cut into the picture");
    let scrolled_drawn = pane.draw(&scrolled);

    // The body starts at the same row on both screens, so a row's offset in the
    // view is its offset from the body's top.
    let top = |drawn: &TestBackend, view: &View| -> u16 {
        let body = (0..drawn.buffer().area.height)
            .find(|y| {
                (0..drawn.buffer().area.width)
                    .any(|x| matches!(drawn.buffer()[(x, *y)].symbol(), "▀" | "▄"))
            })
            .expect("a drawn picture");
        let first = view
            .rows
            .iter()
            .position(|row| matches!(row, Row::Picture { .. }))
            .expect("a picture row");
        body - first as u16
    };
    let (whole_top, scrolled_top) = (top(&whole_drawn, &whole), top(&scrolled_drawn, &scrolled));
    for x in 0..pane.width {
        let want = &whole_drawn.buffer()[(x, whole_top + (first_at as u16) + part)];
        let got = &scrolled_drawn.buffer()[(x, scrolled_top + at as u16)];
        if matches!(want.symbol(), "▀" | "▄") {
            assert_eq!(
                (got.symbol(), got.fg, got.bg),
                (want.symbol(), want.fg, want.bg),
                "column {x} of the picture's row {part} is not what the whole picture draws there"
            );
        }
    }
}

/// Only the pictures on screen are kept.
#[test]
fn kept_equals_on_screen() {
    let scratch = Scratch::new("pictures-kept");
    scratch.write("a.png", png(120, 260, 1));
    scratch.write("m.txt", support::numbered_lines(120));
    scratch.write("z.png", png(120, 260, 3));
    scratch.commit_all("baseline");
    scratch.write("a.png", png(120, 260, 2));
    scratch.write("m.txt", support::numbered_lines(200));
    scratch.write("z.png", png(120, 260, 4));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    pane.settled(&mut frame);
    assert_eq!(pane.pictures.kept(), 1, "a picture off screen was kept");
    for _ in 0..40 {
        pane.scroll(&mut frame, 10);
        pane.view(&mut frame);
    }
    let view = pane.settled(&mut frame);
    let shown: Vec<&str> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Sized { key, .. } => Some(key.path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        shown,
        ["z.png"],
        "the fixture did not scroll to the other image"
    );
    assert_eq!(
        pane.pictures.kept(),
        1,
        "the picture scrolled away was kept"
    );
}

/// A picture asked for at one size and then another is kept at the second.
#[test]
fn resize_mid_decode() {
    let scratch = changed(
        "pictures-resize",
        "screen.png",
        &png(120, 260, 1),
        &png(120, 260, 2),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    pane.view(&mut frame);
    pane.width = 140;
    pane.height = 40;

    let view = pane.settled(&mut frame);
    assert_eq!(
        pane.pictures.kept(),
        1,
        "the picture for the old size was kept"
    );
    assert!(picture_rows(&view).iter().all(|(_, built)| *built));
}

/// A PNG whose header claims 30000 by 30000: the limits refuse its decode, and
/// the line says so.
#[test]
fn decompression_bomb_refused() {
    let bomb = claimed_png(30_000, 30_000);
    let scratch = changed("pictures-bomb", "bomb.png", &png(120, 260, 1), &bomb);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);

    let view = pane.settled(&mut frame);
    assert!(
        sized(&view).is_some_and(|(line, found, _, _)| line.contains("30000×30000")
            && found.as_deref() == Some("could not be decoded")),
        "{:?}",
        sized(&view)
    );
    assert!(
        picture_rows(&view).is_empty(),
        "rows of a refused picture are still held"
    );
}

/// Sixteen-bit, grey with alpha, and fully transparent pictures all build.
#[test]
fn unusual_pixels_build() {
    let deep = image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        80,
        80,
        image::Rgba([60_000u16, 2_000, 30_000, 65_535]),
    ));
    let grey = image::DynamicImage::ImageLumaA8(image::ImageBuffer::from_pixel(
        80,
        80,
        image::LumaA([120u8, 200]),
    ));
    let clear =
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(80, 80, Rgba([255, 0, 0, 0])));
    for (name, image) in [("deep", deep), ("grey", grey), ("clear", clear)] {
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode");
        let scratch = changed(
            &format!("pictures-{name}"),
            "x.png",
            &png(80, 80, 9),
            &bytes.into_inner(),
        );
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut pane = Pane::new(100, 30);
        let view = pane.settled(&mut frame);
        assert!(
            sized(&view).is_some_and(|(_, _, _, panels)| panels > 0),
            "the {name} picture did not build"
        );
    }
}

/// The cells of a tiny known pair, colours and all.
#[test]
fn tiny_pair_cells() {
    let old = RgbaImage::from_fn(16, 32, |x, y| {
        Rgba([(x * 16) as u8, (y * 8) as u8, 128, 255])
    });
    let mut new = old.clone();
    new.put_pixel(4, 4, Rgba([0, 0, 0, 255]));
    let scratch = changed(
        "pictures-tiny",
        "tiny.png",
        &encode(&old, ImageFormat::Png),
        &encode(&new, ImageFormat::Png),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    let view = pane.settled(&mut frame);
    let drawn = pane.draw(&view);
    let buffer = drawn.buffer();
    let cells: Vec<String> = (0..buffer.area.height)
        .filter_map(|y| {
            let row: Vec<String> = (0..buffer.area.width)
                .filter_map(|x| {
                    let cell = &buffer[(x, y)];
                    matches!(cell.symbol(), "▀" | "▄").then(|| {
                        let hex = |c: ratatui::style::Color| match c {
                            ratatui::style::Color::Rgb(r, g, b) => format!("{r:02x}{g:02x}{b:02x}"),
                            other => format!("{other:?}"),
                        };
                        format!("{}{}/{}", cell.symbol(), hex(cell.fg), hex(cell.bg))
                    })
                })
                .collect();
            (!row.is_empty()).then(|| row.join(" "))
        })
        .collect();
    insta::assert_snapshot!(cells.join("\n"));
}

/// Moving to an image lands on its heading.
#[test]
fn landing_on_heading() {
    let scratch = Scratch::new("pictures-landing");
    scratch.write("a.txt", support::numbered_lines(80));
    scratch.write("b.png", png(120, 260, 1));
    scratch.commit_all("baseline");
    scratch.write("a.txt", support::numbered_lines(160));
    scratch.write("b.png", png(120, 260, 2));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    pane.view(&mut frame);
    let height = pane.body(&frame).diff;
    pane.app
        .apply(Action::File(1), &mut frame, height)
        .expect("next file");

    let view = pane.view(&mut frame);
    assert_eq!(
        (view.top.file, view.top.row),
        (1, 0),
        "moving to the image did not land on its heading"
    );
    assert!(
        matches!(view.rows.first(), Some(Row::File(entry)) if entry.path == "b.png"),
        "the image's heading is not the first row"
    );
}

/// A picture scrolled off screen before the worker reaches it is not built.
#[test]
fn worker_skips_hidden() {
    let scratch = Scratch::new("pictures-skip");
    let names: Vec<String> = (0..6).map(|n| format!("s{n}.png")).collect();
    for (n, name) in names.iter().enumerate() {
        scratch.write(name, png(120, 260, n as u8));
    }
    scratch.commit_all("baseline");
    for (n, name) in names.iter().enumerate() {
        scratch.write(name, png(120, 260, 100 + n as u8));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    // Tall enough for every picture to be queued at once.
    let mut pane = Pane::new(100, 400);
    // The worker waits after its first picture until the screen has been cleared.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    pane.pictures = Pictures::new(
        INKS,
        Picker::halfblocks(),
        Box::new(move || {
            let _ = held.lock().map(|held| held.recv());
        }),
    );
    let decoded = pane.pictures.decodes();
    let queued = pane.view(&mut frame);
    let shown = queued
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Sized { .. }))
        .count();
    assert_eq!(
        shown,
        names.len(),
        "the fixture did not put every picture on screen"
    );
    let started = std::time::Instant::now();
    while pane.pictures.decodes() - decoded < 2 {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the first picture was never built"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut nothing: Vec<Row> = Vec::new();
    pane.pictures.attach(&mut nothing, &frame);
    drop(release);
    assert!(
        pane.pictures.settle(Duration::from_secs(60)),
        "the worker never finished"
    );

    assert_eq!(
        pane.pictures.decodes() - decoded,
        2,
        "pictures nobody could see were decoded"
    );
    assert_eq!(pane.pictures.kept(), 0, "a picture off screen was kept");
}

/// The Kitty placeholder every cell of a Kitty picture carries.
const PLACEHOLDER: char = '\u{10EEEE}';

/// A settled pane drawing a modified screen image in `protocol`.
fn protocol_pane(name: &str, protocol: ProtocolType) -> (Scratch, Pane) {
    let scratch = changed(
        name,
        "screen.png",
        &encode(&screen_like(120, 260, [40, 90, 200]), ImageFormat::Png),
        &encode(&screen_like(120, 260, [200, 90, 40]), ImageFormat::Png),
    );
    (scratch, Pane::drawn_by(100, 30, protocol))
}

/// Cells whose symbol contains `needle`.
fn cells_with(backend: &TestBackend, needle: &str) -> usize {
    backend
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.symbol().contains(needle))
        .count()
}

#[test]
fn kitty_placement() {
    let (scratch, mut pane) = protocol_pane("pictures-kitty", ProtocolType::Kitty);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = pane.settled(&mut frame);
    let (_, _, kind, panels) = sized(&view).expect("a size line");
    assert_eq!((kind, panels), (Kind::Three, 3));
    let first = pane.draw(&view);
    assert_eq!(
        cells_with(&first, "\x1b_G"),
        3,
        "each panel's image is not sent once, in one cell"
    );
    let placed = cells_with(&first, &PLACEHOLDER.to_string());
    let slots: usize = sized_slots(&view).iter().map(|(w, r)| w * r).sum();
    assert!(
        placed >= slots * 9 / 10 && placed <= slots,
        "{placed} placeholders for {slots} cells of panels"
    );
    assert_eq!(
        cells_with(&first, "\u{2580}"),
        0,
        "a Kitty pane drew half-blocks"
    );
    let again = pane.draw(&view);
    assert_eq!(
        cells_with(&again, "\x1b_G"),
        0,
        "an image already sent was sent again"
    );
    assert_eq!(cells_with(&again, &PLACEHOLDER.to_string()), placed);
}

#[test]
fn scrolled_kitty_placement() {
    let scratch = Scratch::new("pictures-kitty-scrolled");
    scratch.write("a.png", png(120, 260, 1));
    scratch.write("z.txt", support::numbered_lines(10));
    scratch.commit_all("baseline");
    scratch.write("a.png", png(120, 260, 2));
    scratch.write("z.txt", support::numbered_lines(13));
    let mut pane = Pane::drawn_by(100, 30, ProtocolType::Kitty);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let whole = pane.settled(&mut frame);
    let rows = picture_rows(&whole).len();
    pane.scroll(&mut frame, 4);
    let view = pane.settled(&mut frame);
    let shown = picture_rows(&view);
    assert!(
        shown.first().is_some_and(|(part, _)| *part > 0) && shown.len() < rows,
        "the picture is not cut by the top of the pane: {shown:?}"
    );
    let drawn = pane.draw(&view);
    assert_eq!(
        cells_with(&drawn, "\x1b_G"),
        3,
        "a picture cut by the top was not sent"
    );
    let placed_rows = drawn
        .buffer()
        .content()
        .chunks(usize::from(pane.width))
        .filter(|row| row.iter().any(|cell| cell.symbol().contains(PLACEHOLDER)))
        .count();
    assert_eq!(
        placed_rows,
        shown.len(),
        "placeholders on {placed_rows} rows"
    );
}

#[test]
fn sixel_placement() {
    let (scratch, mut pane) = protocol_pane("pictures-sixel", ProtocolType::Sixel);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = pane.settled(&mut frame);
    let drawn = pane.draw(&view);
    assert_eq!(
        cells_with(&drawn, "\x1bP"),
        3,
        "each panel is not one Sixel sequence in its first cell"
    );
    assert_eq!(
        cells_with(&drawn, "\u{2580}"),
        0,
        "a Sixel pane drew half-blocks"
    );
}

#[test]
fn iterm2_placement() {
    let (scratch, mut pane) = protocol_pane("pictures-iterm2", ProtocolType::Iterm2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = pane.settled(&mut frame);
    let drawn = pane.draw(&view);
    let rows: usize = sized_slots(&view).iter().map(|(_, r)| r).sum();
    let images = cells_with(&drawn, "1337;File=");
    assert!(
        images > 0 && images <= rows,
        "{images} iTerm2 images for {rows} panel rows"
    );
}

/// Each panel's slot as `(width, rows)`.
fn sized_slots(view: &View) -> Vec<(usize, usize)> {
    view.rows
        .iter()
        .find_map(|row| match row {
            Row::Sized { key, .. } => Some(
                key.shape
                    .slots
                    .iter()
                    .map(|slot| (usize::from(slot.width), usize::from(slot.rows)))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// The id a Kitty transmit names, from its `i=` key.
fn kitty_ids(backend: &TestBackend) -> Vec<u32> {
    backend
        .buffer()
        .content()
        .iter()
        .filter_map(|cell| {
            let at = cell.symbol().find("a=T")?;
            let head = &cell.symbol()[..at];
            head.rsplit("i=").next()?.split(',').next()?.parse().ok()
        })
        .collect()
}

#[test]
fn kitty_deleted_on_leave() {
    let (scratch, mut pane) = protocol_pane("pictures-kitty-leave", ProtocolType::Kitty);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = pane.settled(&mut frame);
    let sent = kitty_ids(&pane.draw(&view));
    assert_eq!(sent.len(), 3, "three images were not sent: {sent:?}");
    assert_eq!(
        pane.pictures.retired(),
        "",
        "a delete was owed while on screen"
    );

    // The image is reverted, so its picture leaves the pane.
    scratch.checkout("screen.png");
    frame.advance().expect("advance");
    pane.view(&mut frame);
    let deletes = pane.pictures.retired();
    for id in &sent {
        assert!(
            deletes.contains(&format!("a=d,d=I,i={id},q=2")),
            "image {id} was not deleted: {deletes:?}"
        );
    }
    assert_eq!(pane.pictures.retired(), "", "a delete was owed twice");
}

#[test]
fn protocol_hidden_under_sheet() {
    for (protocol, sequence) in [
        (ProtocolType::Kitty, "\x1b_G"),
        (ProtocolType::Sixel, "\x1bP"),
    ] {
        let (scratch, mut pane) = protocol_pane(&format!("pictures-sheet-{protocol:?}"), protocol);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let view = pane.settled(&mut frame);
        let mut chrome = pane.chrome();
        chrome.sheet = Some(0);
        let covered = pane.draw_with(&view, &chrome);
        assert_eq!(
            cells_with(&covered, sequence),
            0,
            "a {protocol:?} image was drawn under the sheet, which cannot hide it"
        );
        let open = pane.draw(&view);
        assert_eq!(
            cells_with(&open, sequence),
            3,
            "the {protocol:?} images were not drawn once the sheet closed"
        );
    }
}

#[test]
fn built_after_pick() {
    let scratch = changed(
        "pictures-unpicked",
        "screen.png",
        &png(120, 260, 1),
        &png(120, 260, 2),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pane = Pane::new(100, 30);
    pane.pictures = Pictures::unpicked(INKS, Box::new(|| {}));
    let first = pane.view(&mut frame);
    assert!(
        !picture_rows(&first).is_empty(),
        "no rows were held for the pictures"
    );
    assert!(
        !pane.pictures.settle(Duration::from_millis(300)),
        "a picture was built before the picker was given"
    );
    assert_eq!(
        pane.pictures.kept(),
        0,
        "a picture was kept before the picker was given"
    );
    pane.pictures.pick(Picker::halfblocks());
    let view = pane.settled(&mut frame);
    assert!(
        picture_rows(&view).iter().all(|(_, built)| *built),
        "the held picture was not built once the picker was given"
    );
}

#[test]
fn kitty_deleted_on_quit() {
    let (scratch, mut pane) = protocol_pane("pictures-kitty-quit", ProtocolType::Kitty);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = pane.settled(&mut frame);
    let sent = kitty_ids(&pane.draw(&view));
    assert_eq!(sent.len(), 3, "three images were not sent: {sent:?}");
    pane.pictures.retire_all();
    let deletes = pane.pictures.retired();
    for id in &sent {
        assert!(
            deletes.contains(&format!("a=d,d=I,i={id},q=2")),
            "image {id} was left in the terminal on quit: {deletes:?}"
        );
    }
}
