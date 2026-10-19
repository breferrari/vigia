//! A changed SVG draws pictures in place of its text diff. `SPEC.md` §11.1.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use support::Scratch;
use vigia::pictures::{self, Inks, Kind, Pictures, Verdict};
use vigia::{App, Pointing, Row, View, body_layout};
use vigia_core::{Frame, Highlighter, History, Standing};

const INKS: Inks = Inks {
    canvas: (0x0d, 0x11, 0x17),
    changed: (0xf8, 0x51, 0x49),
};

const ROOT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="24" height="24">"#;

/// A view of `frame` on a 100x30 pane, with its pictures attached.
fn view_of(app: &mut App, pictures: &mut Pictures, frame: &mut Frame) -> View {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome(
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
    let body = body_layout(
        Rect::new(0, 0, 100, 30),
        &chrome,
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    );
    let mut view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    pictures.attach(&mut view.rows, frame);
    view
}

/// What the size line came to: its text, the kind of pictures, how many
/// panels were built, and what the comparison found.
struct Drawn {
    line: String,
    kind: Kind,
    panels: usize,
    verdict: Option<Verdict>,
    took: Duration,
}

/// `old` committed as `icon.svg` and rewritten as `new` (or added, with no
/// `old`), drawn once every picture is built.
fn drawn(name: &str, old: Option<&str>, new: &str) -> Drawn {
    let scratch = Scratch::new(name);
    if let Some(old) = old {
        scratch.write("icon.svg", old);
    } else {
        scratch.write("keep.txt", "x\n");
    }
    scratch.commit_all("initial");
    scratch.write("icon.svg", new);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    app.draw_pictures(true);
    let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
    view_of(&mut app, &mut pictures, &mut frame);
    let began = Instant::now();
    assert!(
        pictures.settle(Duration::from_secs(60)),
        "the pictures were not built"
    );
    let took = began.elapsed();
    let view = view_of(&mut app, &mut pictures, &mut frame);
    view.rows
        .iter()
        .find_map(|row| match row {
            Row::Sized {
                line, key, ready, ..
            } => Some(Drawn {
                line: line.clone(),
                kind: key.shape.kind,
                panels: ready.as_ref().map_or(0, |ready| ready.0.panels.len()),
                verdict: ready.as_ref().map(|ready| ready.0.verdict),
                took,
            }),
            _ => None,
        })
        .expect("a size line")
}

fn svg(body: &str) -> String {
    format!("{ROOT}{body}</svg>")
}

fn changed(verdict: Option<Verdict>) -> bool {
    matches!(verdict, Some(Verdict::Changed { pixels, .. }) if pixels > 0)
}

#[test]
fn svg_three_panels() {
    let drawn = drawn(
        "svg-three",
        Some(&svg(r#"<circle cx="12" cy="12" r="10" fill="red"/>"#)),
        &svg(r#"<circle cx="12" cy="12" r="10" fill="blue"/>"#),
    );
    assert!(
        drawn.line.starts_with("SVG 24×24"),
        "the line reads {:?}",
        drawn.line
    );
    assert_eq!((drawn.kind, drawn.panels), (Kind::Three, 3));
}

#[test]
fn svg_added_one_panel() {
    let drawn = drawn(
        "svg-added",
        None,
        &svg(r#"<rect width="24" height="24" fill="red"/>"#),
    );
    assert_eq!((drawn.kind, drawn.panels), (Kind::Added, 1));
}

#[test]
fn svg_shape_lights_mask() {
    let drawn = drawn(
        "svg-shape",
        Some(&svg(
            r#"<rect x="2" y="2" width="4" height="4" fill="red"/>"#,
        )),
        &svg(r#"<rect x="3" y="2" width="4" height="4" fill="red"/>"#),
    );
    assert!(
        changed(drawn.verdict),
        "a moved shape lit nothing: {:?}",
        drawn.verdict
    );
}

#[test]
fn svg_bad_render_undecoded() {
    // A root that gives a size, and a body usvg refuses.
    let drawn = drawn(
        "svg-bad",
        Some(&svg(r#"<rect width="2" height="2"/>"#)),
        &format!("{ROOT}<rect width=\"2\" height=</svg>"),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded));
    assert_eq!(drawn.panels, 0);
}

#[test]
fn svg_render_on_worker() {
    let before = pictures::decodes_off_worker();
    let drawn = drawn(
        "svg-worker",
        Some(&svg(r#"<rect width="2" height="2" fill="red"/>"#)),
        &svg(r#"<rect width="3" height="2" fill="red"/>"#),
    );
    assert_eq!(drawn.panels, 3);
    assert_eq!(
        pictures::decodes_off_worker(),
        before,
        "an SVG was rendered off the picture worker"
    );
}

#[test]
fn svg_text_change_lights_mask() {
    let mut fonts = resvg::usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    if fonts.is_empty() {
        eprintln!("note: this system has no fonts, so the text test proves nothing here");
        return;
    }
    // Plain ASCII, which any fallback face draws. Pixels are never compared:
    // fonts differ between machines.
    let label = |text: &str| {
        svg(&format!(
            r#"<text x="1" y="16" font-size="12">{text}</text>"#
        ))
    };
    let drawn = drawn("svg-text", Some(&label("Save")), &label("Load"));
    assert!(
        changed(drawn.verdict),
        "a change to a label alone lit nothing: {:?}",
        drawn.verdict
    );
}

#[test]
fn svg_external_href_not_read() {
    let scratch = Scratch::new("svg-href-target");
    let target = scratch.path_of("green.png");
    std::fs::write(
        &target,
        support::encode_image(
            &image::RgbaImage::from_pixel(24, 24, image::Rgba([0, 255, 0, 255])),
            image::ImageFormat::Png,
        ),
    )
    .expect("write the target");
    let refused = pictures::hrefs_refused();
    let pointing = svg(&format!(
        r#"<image width="24" height="24" xlink:href="{}"/><rect width="1" height="1"/>"#,
        target.display()
    ));
    let without = svg(r#"<rect width="1" height="1"/>"#);
    let drawn = drawn("svg-href", Some(&without), &pointing);
    assert_eq!(
        drawn.verdict,
        Some(Verdict::Unchanged),
        "a file an SVG pointed at was drawn"
    );
    assert!(
        pictures::hrefs_refused() > refused,
        "the reference was not refused by the resolver"
    );
}

#[test]
fn svg_use_bomb_bounded() {
    let mut bomb = String::from(r#"<defs><rect id="l0" width="1" height="1"/>"#);
    for level in 1..=6 {
        bomb.push_str(&format!(r#"<g id="l{level}">"#));
        for _ in 0..10 {
            bomb.push_str(&format!(r##"<use xlink:href="#l{}"/>"##, level - 1));
        }
        bomb.push_str("</g>");
    }
    bomb.push_str(r##"</defs><use xlink:href="#l6"/>"##);
    let drawn = drawn(
        "svg-bomb",
        Some(&svg(r#"<rect width="1" height="1"/>"#)),
        &svg(&bomb),
    );
    assert_eq!(
        drawn.verdict,
        Some(Verdict::Undecoded),
        "a million-use bomb was drawn"
    );
    assert!(
        drawn.took < Duration::from_secs(30),
        "the bomb held the worker for {:?}",
        drawn.took
    );
}
