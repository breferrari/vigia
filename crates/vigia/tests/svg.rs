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
        pictures.settle(support::ceiling(Duration::from_secs(60))),
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
fn svg_prefixed_text_lights_mask() {
    let mut fonts = resvg::usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    if fonts.is_empty() {
        eprintln!("note: this system has no fonts, so the text test proves nothing here");
        return;
    }
    let label = |text: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:s="http://www.w3.org/2000/svg" width="24" height="24"><s:text x="1" y="16" font-size="12">{text}</s:text></svg>"#
        )
    };
    let drawn = drawn("svg-prefixed-text", Some(&label("Save")), &label("Load"));
    assert!(
        changed(drawn.verdict),
        "a change to a prefixed label alone lit nothing: {:?}",
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

/// `bytes` as base64, for a `data:` URI.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, &byte)| {
            word | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            out.push(if at <= chunk.len() {
                char::from(ALPHABET[(word >> (18 - 6 * at)) as usize & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// A PNG of `width` by `height` noise, which does not compress.
fn noise_png(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 0x9e37_79b9_u32;
    let image = image::RgbaImage::from_fn(width, height, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        image::Rgba(seed.to_le_bytes())
    });
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode");
    png
}

/// `leaf` copied `fan` times at each of `levels` levels by `<use>`.
fn fanned(leaf: &str, fan: usize, levels: usize) -> String {
    let mut body = format!("<defs>{leaf}");
    for level in 1..=levels {
        body.push_str(&format!(r#"<g id="l{level}">"#));
        for _ in 0..fan {
            body.push_str(&format!(r##"<use xlink:href="#l{}"/>"##, level - 1));
        }
        body.push_str("</g>");
    }
    body.push_str(&format!(r##"</defs><use xlink:href="#l{levels}"/>"##));
    body
}

/// The verdict on `body` drawn over a plain square.
fn verdict_of(name: &str, body: &str) -> Option<Verdict> {
    drawn(
        name,
        Some(&svg(r#"<rect width="2" height="2"/>"#)),
        &svg(body),
    )
    .verdict
}

#[test]
fn svg_image_copies_bounded() {
    let png = base64(&noise_png(32, 32));
    let leaf = format!(
        r#"<image id="l0" width="1" height="1" xlink:href="data:image/png;base64,{png}"/>"#
    );
    assert_eq!(
        verdict_of("svg-image-copies", &fanned(&leaf, 10, 4)),
        Some(Verdict::Undecoded),
        "ten thousand copies of an embedded image were drawn"
    );
    let one = format!(r#"<image width="8" height="8" xlink:href="data:image/png;base64,{png}"/>"#);
    assert!(
        changed(verdict_of("svg-image-one", &one)),
        "one embedded image was refused"
    );
}

#[test]
fn svg_image_size_bounded() {
    let png = base64(&noise_png(16_385, 1));
    let body = format!(r#"<image width="8" height="8" xlink:href="data:image/png;base64,{png}"/>"#);
    assert_eq!(
        verdict_of("svg-image-wide", &body),
        Some(Verdict::Undecoded),
        "an embedded image wider than any picture was drawn"
    );
}

#[test]
fn svg_nested_svg_skipped() {
    let inner = base64(svg(&fanned(r#"<rect id="l0" width="1" height="1"/>"#, 10, 5)).as_bytes());
    let body = format!(
        r#"<image width="8" height="8" xlink:href="data:image/svg+xml;base64,{inner}"/><rect width="4" height="4"/>"#
    );
    let drawn = drawn(
        "svg-nested",
        Some(&svg(r#"<rect width="2" height="2"/>"#)),
        &svg(&body),
    );
    assert!(
        changed(drawn.verdict),
        "the document around a nested SVG was not drawn"
    );
    assert!(
        drawn.took < Duration::from_secs(5),
        "a nested SVG of a hundred thousand copies was drawn: {:?}",
        drawn.took
    );
}

#[test]
fn svg_unknown_image_skipped() {
    let body = r#"<image width="8" height="8" xlink:href="data:image/avif;base64,AAAAHGZ0eXBhdmlm"/><rect width="4" height="4"/>"#;
    assert!(
        changed(verdict_of("svg-unknown-image", body)),
        "an image usvg would leave out refused the whole document"
    );
}

/// How many pixels each side was rendered with, when the two compared.
fn rendered(verdict: Option<Verdict>) -> Option<u64> {
    match verdict {
        Some(Verdict::Changed { total, .. }) => Some(total),
        _ => None,
    }
}

const BLUR: &str = r#"<filter id="blur"><feGaussianBlur stdDeviation="1"/></filter>"#;

#[test]
fn svg_filters_scaled() {
    let leaf = format!(r#"{BLUR}<g id="l0" filter="url(#blur)"><rect width="2" height="2"/></g>"#);
    let total = rendered(verdict_of("svg-filters", &fanned(&leaf, 10, 2)));
    // A hundred blurs at 16 cells each: sqrt(2^26 / 1600) is 204 a side.
    assert_eq!(
        total,
        Some(204 * 204),
        "a hundred blurs were not rendered smaller"
    );
    let one = format!(r#"{BLUR}<g filter="url(#blur)"><rect width="8" height="8"/></g>"#);
    assert_eq!(
        rendered(verdict_of("svg-filter-one", &one)),
        Some(1024 * 1024),
        "one blur was rendered smaller"
    );
}

#[test]
fn svg_filters_refused() {
    let leaf = format!(r#"{BLUR}<g id="l0" filter="url(#blur)"><rect width="2" height="2"/></g>"#);
    assert_eq!(
        verdict_of("svg-filters-many", &fanned(&leaf, 10, 4)),
        Some(Verdict::Undecoded),
        "ten thousand filtered copies were drawn"
    );
}

#[test]
fn svg_filter_side_compared() {
    // Only the new side is filtered, and both are rendered at its size.
    let leaf = format!(r#"{BLUR}<g id="l0" filter="url(#blur)"><rect width="2" height="2"/></g>"#);
    let drawn = drawn(
        "svg-filter-side",
        Some(&svg(r#"<rect width="2" height="2"/>"#)),
        &svg(&fanned(&leaf, 10, 2)),
    );
    assert!(changed(drawn.verdict), "the two sides were not compared");
}

#[test]
fn svg_convolve_bounded() {
    let convolve = |order: usize| {
        let kernel = vec!["1"; order * order].join(" ");
        format!(
            r#"<filter id="big"><feConvolveMatrix order="{order}" kernelMatrix="{kernel}"/></filter><rect width="8" height="8" filter="url(#big)"/>"#
        )
    };
    assert_eq!(
        rendered(verdict_of("svg-convolve", &convolve(32))),
        Some(256 * 256),
        "a 32x32 convolution was not rendered at 256 a side"
    );
    assert_eq!(
        verdict_of("svg-convolve-huge", &convolve(129)),
        Some(Verdict::Undecoded),
        "a 129x129 convolution was drawn"
    );
}

#[test]
fn svg_masked_filters_refused() {
    // The filtered copies are reached only through the mask.
    let leaf = format!(
        r#"{BLUR}<g id="l0" filter="url(#blur)"><rect width="2" height="2" fill="white"/></g>"#
    );
    let body = format!(
        r##"{}<mask id="m"><use xlink:href="#l4"/></mask><rect width="8" height="8" mask="url(#m)"/>"##,
        fanned(&leaf, 10, 4).replace(r##"<use xlink:href="#l4"/>"##, "")
    );
    assert_eq!(
        verdict_of("svg-masked-filters", &body),
        Some(Verdict::Undecoded),
        "ten thousand filtered copies inside a mask were drawn"
    );
}

#[test]
fn svg_morphology_scaled() {
    // A 4-unit radius on a 24-unit document spans a third of the render at 1024.
    let body = r#"<filter id="m"><feMorphology operator="dilate" radius="4"/></filter><rect width="4" height="4" filter="url(#m)"/>"#;
    let total = rendered(verdict_of("svg-morphology", body));
    assert!(
        total.is_some_and(|total| total < 256 * 256),
        "a morphology window as wide as the picture was rendered at {total:?} pixels"
    );
}

#[test]
fn svg_turbulence_refused() {
    let body = r#"<filter id="t"><feTurbulence baseFrequency="0.5" numOctaves="100000000"/></filter><rect width="8" height="8" filter="url(#t)"/>"#;
    assert_eq!(
        verdict_of("svg-turbulence", body),
        Some(Verdict::Undecoded),
        "a hundred million octaves of turbulence were drawn"
    );
}
