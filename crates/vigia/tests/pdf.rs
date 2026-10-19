//! A changed PDF draws pictures of its first changed page. `SPEC.md` §11.1.
#![cfg(feature = "pdf")]

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use support::{
    Scratch, icon_ops, icon_pdf, label_pdf, open_locked_pdf, pdf, pdf_doc, pdf_with, zlib_zeros,
};
use vigia::pictures::{Inks, Kind, Pictures, Verdict};
use vigia::{App, Pointing, Row, View, body_layout};
use vigia_core::{Frame, Highlighter, History, Standing};

const INKS: Inks = Inks {
    canvas: (0x0d, 0x11, 0x17),
    changed: (0xf8, 0x51, 0x49),
};

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

/// What the size line came to once the pictures were built.
#[derive(Debug)]
struct Drawn {
    line: String,
    kind: Kind,
    panels: usize,
    verdict: Option<Verdict>,
    /// What the line says after the size, once built.
    said: Option<String>,
    /// Picture rows before the build, and after.
    rows: (usize, usize),
    /// The bar the built picture draws in place of its shape's.
    bar: Option<vigia::pictures::Bar>,
    /// The size line before the build, and whether the worker sized a side.
    first: Option<String>,
    learned: bool,
    decodes: u64,
}

fn picture_rows(view: &View) -> usize {
    view.rows
        .iter()
        .filter(|row| matches!(row, Row::Picture { .. }))
        .count()
}

/// `old` committed as `doc.pdf` and rewritten as `new` (or added, with no
/// `old`), drawn once every picture is built.
fn drawn(name: &str, old: Option<&[u8]>, new: &[u8]) -> Drawn {
    let scratch = Scratch::new(name);
    if let Some(old) = old {
        scratch.write("doc.pdf", old);
    } else {
        scratch.write("keep.txt", "x\n");
    }
    scratch.commit_all("initial");
    scratch.write("doc.pdf", new);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    app.draw_pictures(true);
    let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
    let first = view_of(&mut app, &mut pictures, &mut frame);
    let before = picture_rows(&first);
    let first_line = sized_line(&first);
    // As the shell does: a size the worker found is given to the frame, and
    // the pictures it then reserves are built.
    let mut learned = false;
    let view = loop {
        assert!(
            pictures.settle(Duration::from_secs(120)),
            "the pictures were not built"
        );
        let view = view_of(&mut app, &mut pictures, &mut frame);
        let sizes = pictures.found_sizes();
        if sizes.is_empty() {
            break view;
        }
        learned = true;
        for (stamp, width, height) in sizes {
            frame.learn_size(stamp, width, height);
        }
        view_of(&mut app, &mut pictures, &mut frame);
    };
    let after = picture_rows(&view);
    let decodes = pictures.decodes();
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
                said: ready.as_ref().and_then(|ready| ready.0.suffix()),
                bar: ready.as_ref().and_then(|ready| ready.0.bar),
                rows: (before, after),
                first: first_line.clone(),
                learned,
                decodes,
            }),
            _ => None,
        })
        .expect("a size line")
}

fn sized_line(view: &View) -> Option<String> {
    view.rows.iter().find_map(|row| match row {
        Row::Sized { line, .. } => Some(line.clone()),
        _ => None,
    })
}

fn changed(verdict: Option<Verdict>) -> bool {
    matches!(verdict, Some(Verdict::Changed { pixels, .. }) if pixels > 0)
}

/// Twelve pages, each its own colour, with `page` (from 1) drawn from
/// `seed` instead.
fn twelve(page: usize, seed: u8) -> Vec<u8> {
    let ops: Vec<Vec<u8>> = (1..=12)
        .map(|n| icon_ops(if n == page { seed } else { n as u8 * 10 }))
        .collect();
    let pages: Vec<(u32, u32, &[u8])> = ops.iter().map(|ops| (24, 24, ops.as_slice())).collect();
    pdf(&pages)
}

#[test]
fn pdf_three_panels() {
    let drawn = drawn("pdf-three", Some(&icon_pdf(1)), &icon_pdf(200));
    assert_eq!(drawn.line, "PDF 24×24 pt · 505 B");
    assert_eq!(drawn.kind, Kind::Three);
    assert_eq!(drawn.panels, 3);
}

#[test]
fn pdf_added_one_panel() {
    let drawn = drawn("pdf-added", None, &icon_pdf(1));
    assert_eq!(drawn.kind, Kind::Added);
    assert_eq!(drawn.panels, 1);
}

#[test]
fn pdf_shape_lights_mask() {
    let drawn = drawn("pdf-shape", Some(&icon_pdf(1)), &icon_pdf(200));
    assert!(changed(drawn.verdict), "{drawn:?}");
}

#[test]
fn pdf_render_on_worker() {
    let before = vigia::pictures::decodes_off_worker();
    let drawn = drawn("pdf-worker", Some(&icon_pdf(1)), &icon_pdf(200));
    assert_eq!(
        vigia::pictures::decodes_off_worker(),
        before,
        "a PDF was read off the worker"
    );
    assert_eq!(drawn.decodes, 2, "both sides were not read on the worker");
}

#[test]
fn pdf_text_lights_mask() {
    let drawn = drawn("pdf-text", Some(&label_pdf("Save")), &label_pdf("Load"));
    assert!(
        changed(drawn.verdict),
        "a change to a label alone lit nothing: {drawn:?}"
    );
}

#[test]
fn pdf_empty_password_drawn() {
    let drawn = drawn("pdf-open-locked", Some(&icon_pdf(1)), &open_locked_pdf(200));
    assert!(
        changed(drawn.verdict),
        "a PDF that opens with no password did not draw its content: {drawn:?}"
    );
}

#[test]
fn pdf_change_page_three() {
    let drawn = drawn("pdf-page-three", Some(&twelve(3, 30)), &twelve(3, 250));
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("page 3 of 12 · ")),
        "{drawn:?}"
    );
    assert!(
        changed(drawn.verdict),
        "page 3's change lit nothing: {drawn:?}"
    );
}

#[test]
fn pdf_page_count_named() {
    let ops = icon_ops(1);
    let pages = |count: usize| pdf(&vec![(24, 24, ops.as_slice()); count]);
    let drawn = drawn("pdf-pages", Some(&pages(12)), &pages(13));
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.contains("12 → 13 pages")),
        "{drawn:?}"
    );
}

#[test]
fn pdf_metadata_only() {
    let page: &[(u32, u32, &[u8])] = &[(24, 24, &icon_ops(1))];
    let drawn = drawn(
        "pdf-metadata",
        Some(&pdf(page)),
        &pdf_with(page, "/Info << /Title (new) >> "),
    );
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("no page changed")),
        "{drawn:?}"
    );
}

/// `doc` with the last `from` replaced by `to`. Its offsets go stale, which
/// the reader repairs.
fn swapped(doc: &[u8], from: &str, to: &str) -> Vec<u8> {
    let at = doc
        .windows(from.len())
        .rposition(|window| window == from.as_bytes())
        .expect("the text to replace");
    [&doc[..at], to.as_bytes(), &doc[at + from.len()..]].concat()
}

#[test]
fn pdf_first_page_unnamed() {
    let drawn = drawn("pdf-first-page", Some(&twelve(1, 250)), &twelve(1, 30));
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| !said.contains("page")),
        "page 1 was named: {drawn:?}"
    );
}

#[test]
fn pdf_box_change_found() {
    let ops = icon_ops(1);
    let old = pdf(&[(24, 24, &ops), (24, 24, &ops)]);
    let new = swapped(
        &old,
        "/MediaBox [0 0 24 24]",
        "/MediaBox [0 0 24 24] /Rotate 90",
    );
    let drawn = drawn("pdf-box", Some(&old), &new);
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("page 2 of 2 · ")),
        "{drawn:?}"
    );
}

#[test]
fn pdf_inherited_font_found() {
    // The page tree holds the font, and the page holds no resources.
    let doc = |font: &str| {
        let label = label_pdf("Ag");
        let label = swapped(
            &label,
            &format!("/Resources << {} >> ", support::HELVETICA),
            "",
        );
        swapped(
            &label,
            "/Type /Pages ",
            &format!(
                "/Type /Pages /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /{font} >> >> >> "
            ),
        )
    };
    let drawn = drawn("pdf-inherited", Some(&doc("Helvetica")), &doc("Courier"));
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| !said.contains("no page changed")),
        "the font change went unseen: {drawn:?}"
    );
}

#[test]
fn pdf_other_aspect_rows() {
    // A portrait first page, and a landscape third page that changed.
    let ops = icon_ops(1);
    let doc = |seed: u8| {
        let third = icon_ops(seed);
        pdf(&[
            (24, 48, ops.as_slice()),
            (24, 48, ops.as_slice()),
            (48, 24, third.as_slice()),
        ])
    };
    let drawn = drawn("pdf-aspect", Some(&doc(30)), &doc(250));
    assert!(drawn.rows.0 > 0, "no rows were reserved: {drawn:?}");
    assert_eq!(
        drawn.rows.0, drawn.rows.1,
        "the rows moved when the picture arrived"
    );
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("page 3 of 3")),
        "{drawn:?}"
    );
    // Fitted into the first page's shape, 1024 tall by 512 wide.
    assert!(
        matches!(drawn.verdict, Some(Verdict::Changed { total, .. }) if total == 512 * 1024),
        "the changed page was not fitted into the first page's shape: {drawn:?}"
    );
}

/// A page whose one content stream, `ops` bytes of squares, is listed
/// `repeat` times, so a short file holds a long page.
fn heavy(repeat: usize, seed: u8) -> Vec<u8> {
    let mut ops = icon_ops(seed);
    ops.push(b'\n');
    while ops.len() < 10_000 {
        ops.extend_from_slice(b"0.5 0.5 0.5 rg 1 1 3 3 re f\n");
    }
    pdf_doc(&[(24, 24, &ops)], "", repeat, &[], "")
}

#[test]
fn pdf_stream_scaled() {
    // About 3 MB of content, past the 1.4 MiB drawn at 1024 a side.
    let drawn = drawn("pdf-scaled", Some(&icon_pdf(1)), &heavy(300, 200));
    let total = match drawn.verdict {
        Some(Verdict::Changed { total, .. }) => total,
        other => panic!("a heavy page did not compare: {other:?}"),
    };
    assert!(total < 1024 * 1024, "drawn at {total} pixels");
}

#[test]
fn pdf_stream_refused() {
    // About 24 MB of content, past what the floor allows.
    let drawn = drawn("pdf-refused", Some(&icon_pdf(1)), &heavy(2_400, 200));
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
}

#[test]
fn pdf_image_refused() {
    // An image that claims more pixels than any image may decode.
    let resources = "/XObject << /Im1 5 0 R >>";
    let image: &[u8] = b"<< /Type /XObject /Subtype /Image /Width 20000 /Height 20000 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 1 >>\nstream\n\x00\nendstream";
    let page = pdf_doc(
        &[(24, 24, b"q 24 0 0 24 0 0 cm /Im1 Do Q")],
        resources,
        1,
        &[image],
        "",
    );
    let drawn = drawn("pdf-image-bomb", Some(&icon_pdf(1)), &page);
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
}

/// A page drawing `images`, each a `width` by `height` image whose Flate data
/// decodes to `zeros` bytes.
fn imaged(images: usize, (width, height): (u32, u32), zeros: u64) -> Vec<u8> {
    let data = zlib_zeros(zeros);
    let objects: Vec<Vec<u8>> = (0..images)
        .map(|_| {
            let mut object = format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                data.len()
            )
            .into_bytes();
            object.extend_from_slice(&data);
            object.extend_from_slice(b"\nendstream");
            object
        })
        .collect();
    let names: Vec<String> = (0..images)
        .map(|n| format!("/Im{n} {} 0 R", 5 + n))
        .collect();
    let draws: String = (0..images)
        .map(|n| format!("q 24 0 0 24 0 0 cm /Im{n} Do Q "))
        .collect();
    let extras: Vec<&[u8]> = objects.iter().map(Vec::as_slice).collect();
    pdf_doc(
        &[(24, 24, draws.as_bytes())],
        &format!("/XObject << {} >>", names.join(" ")),
        1,
        &extras,
        "",
    )
}

/// This process's resident memory, in MiB, where the platform says.
fn resident() -> Option<u64> {
    vigia::memory::resident().map(|bytes| bytes >> 20)
}

#[test]
fn pdf_flate_bomb_undecoded() {
    // About 7 MB of file that inflates to a gigabyte, as an image the frame
    // path skips and the worker must not decode.
    let before = resident();
    let drawn = drawn(
        "pdf-flate-bomb",
        Some(&icon_pdf(1)),
        &imaged(1, (8_000, 8_000), 1 << 30),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, resident()) {
        // The worker's limit, 256 MiB, and what holding it costs.
        assert!(
            after < before + 384,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_small_image_bomb() {
    // An image that claims 8 by 8 pixels and inflates to a gigabyte: only the
    // inflate itself can stop it.
    let before = resident();
    let drawn = drawn(
        "pdf-small-image-bomb",
        Some(&icon_pdf(1)),
        &imaged(1, (8, 8), 1 << 30),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, resident()) {
        assert!(
            after < before + 384,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_many_streams_bounded() {
    // Twenty images of 20 MB each: each fits, and together they do not.
    let drawn = drawn(
        "pdf-many-streams",
        Some(&icon_pdf(1)),
        &imaged(20, (2_000, 2_500), 20_000_000),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
}

#[test]
fn pdf_large_sized_on_worker() {
    // Past what the frame path reads itself: sized on the worker.
    let mut ops = icon_ops(200);
    ops.push(b'\n');
    while ops.len() < 4 << 20 {
        ops.extend_from_slice(b"0.5 0.5 0.5 rg 1 1 3 3 re f\n");
    }
    let large = pdf(&[(24, 24, &ops)]);
    let drawn = drawn("pdf-large", Some(&icon_pdf(1)), &large);
    assert_eq!(
        drawn.first.as_deref(),
        Some("PDF · 505 B → 4.0 MiB"),
        "the line did not wait for the worker's size: {drawn:?}"
    );
    assert_eq!(
        drawn.rows.0, 0,
        "rows were reserved before the size was known"
    );
    assert!(drawn.learned, "the worker did not size the large side");
    assert!(drawn.line.starts_with("PDF 24×24 pt"), "{drawn:?}");
    assert!(drawn.rows.1 > 0, "no rows once the size arrived: {drawn:?}");
    assert!(changed(drawn.verdict), "{drawn:?}");
}

#[test]
fn pdf_unread_bomb_drawn() {
    // Past what the frame path reads, an object that would inflate to a
    // gigabyte and that nothing refers to: nothing decodes it.
    let before = resident();
    let mut object = format!(
        "<< /Type /ObjStm /N 1 /First 0 /Filter /FlateDecode /Length {} >>\nstream\n",
        zlib_zeros(1 << 30).len()
    )
    .into_bytes();
    object.extend_from_slice(&zlib_zeros(1 << 30));
    object.extend_from_slice(b"\nendstream");
    let doc = pdf_doc(&[(24, 24, &icon_ops(200))], "", 1, &[&object], "");
    assert!(
        doc.len() > 1 << 20,
        "the file is small enough for the frame path"
    );
    let drawn = drawn("pdf-unread-bomb", Some(&icon_pdf(1)), &doc);
    assert!(changed(drawn.verdict), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, resident()) {
        assert!(
            after < before + 64,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_content_bomb_undecoded() {
    // A page whose content inflates to a gigabyte.
    let before = resident();
    let zeros = zlib_zeros(1 << 30);
    let mut content = format!(
        "<< /Filter /FlateDecode /Length {} >>\nstream\n",
        zeros.len()
    )
    .into_bytes();
    content.extend_from_slice(&zeros);
    content.extend_from_slice(b"\nendstream");
    let doc = pdf_doc(&[(24, 24, b"")], "", 1, &[&content], "").replace_contents("4 0 R", "5 0 R");
    let drawn = drawn("pdf-content-bomb", Some(&icon_pdf(1)), &doc);
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, resident()) {
        assert!(
            after < before + 384,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

/// Byte replacement in a generated PDF.
trait Replace {
    fn replace_contents(self, from: &str, to: &str) -> Vec<u8>;
}

impl Replace for Vec<u8> {
    fn replace_contents(self, from: &str, to: &str) -> Vec<u8> {
        let at = self
            .windows(from.len())
            .position(|window| window == from.as_bytes())
            .expect("the text to replace");
        [&self[..at], to.as_bytes(), &self[at + from.len()..]].concat()
    }
}

#[test]
fn pdf_page_appended() {
    let ops = icon_ops(1);
    let mut pages = vec![(24, 24, ops.as_slice()); 12];
    let old = pdf(&pages);
    let added = icon_ops(250);
    pages.push((24, 24, added.as_slice()));
    let drawn = drawn("pdf-appended", Some(&old), &pdf(&pages));
    assert_eq!(
        drawn.said.as_deref(),
        Some("page 13 of 13 added · 12 → 13 pages"),
        "{drawn:?}"
    );
    assert_eq!(
        drawn.panels, 1,
        "the added page is not drawn alone: {drawn:?}"
    );
    assert_eq!(drawn.bar, Some(vigia::pictures::Bar::Added), "{drawn:?}");
}

#[test]
fn pdf_page_removed() {
    let ops = icon_ops(1);
    let gone = icon_ops(250);
    let mut pages = vec![(24, 24, ops.as_slice()); 12];
    let new = pdf(&pages);
    pages.push((24, 24, gone.as_slice()));
    let drawn = drawn("pdf-removed", Some(&pdf(&pages)), &new);
    assert_eq!(
        drawn.said.as_deref(),
        Some("page 13 of 13 removed · 13 → 12 pages"),
        "{drawn:?}"
    );
    assert_eq!(
        drawn.panels, 1,
        "the removed page is not drawn alone: {drawn:?}"
    );
    assert_eq!(drawn.bar, Some(vigia::pictures::Bar::Removed), "{drawn:?}");
}
