//! A changed PDF draws pictures of its first changed page. `SPEC.md` §11.1.
#![cfg(feature = "pdf")]

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use support::{
    Scratch, ccitt_white, icon_ops, icon_pdf, label_pdf, lzw_repeats, lzw_zeros, open_locked_pdf,
    pdf, pdf_doc, pdf_with, run_length_zeros, zlib_filled, zlib_zeros, zlib_zeros_lenient,
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
            pictures.settle(support::ceiling(Duration::from_secs(120))),
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

/// `doc` with every `from` replaced by `to`.
fn swapped_all(doc: &[u8], from: &str, to: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(at) = rest.windows(from.len()).position(|w| w == from.as_bytes()) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(to.as_bytes());
        rest = &rest[at + from.len()..];
    }
    out.extend_from_slice(rest);
    out
}

#[test]
fn pdf_shared_resource_found() {
    // Both images sit in the page tree's resources; page 2 alone draws B.
    let doc = |grey: u8| {
        let image = |value: u8| {
            let mut image = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n".to_vec();
            image.push(value);
            image.extend_from_slice(b"\nendstream");
            image
        };
        let (a, b) = (image(64), image(grey));
        let doc = pdf_doc(
            &[
                (24, 24, b"q 24 0 0 24 0 0 cm /A Do Q"),
                (24, 24, b"q 24 0 0 24 0 0 cm /B Do Q"),
            ],
            "",
            1,
            &[&a, &b],
            "",
        );
        let doc = swapped_all(&doc, "/Resources <<  >> ", "");
        swapped(
            &doc,
            "/Type /Pages ",
            "/Type /Pages /Resources << /XObject << /A 7 0 R /B 8 0 R >> >> ",
        )
    };
    let drawn = drawn("pdf-shared-resource", Some(&doc(32)), &doc(224));
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("page 2 of 2 · ")),
        "{drawn:?}"
    );
}

/// `pages` pages, each naming `each` graphics states of its own, the last
/// page in a colour from `seed`.
fn many_objects(pages: usize, each: usize, seed: u8) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    let per_page = 2 + each;
    let kids: Vec<String> = (0..pages)
        .map(|page| format!("{} 0 R", 3 + page * per_page))
        .collect();
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(
        &mut out,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        )
        .as_bytes(),
    );
    for page in 0..pages {
        let first = 3 + page * per_page;
        let states: String = (0..each)
            .map(|n| format!("/G{n} {} 0 R ", first + 2 + n))
            .collect();
        object(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 24 24] /Resources << /ExtGState << {states}>> >> /Contents {} 0 R >>",
                first + 1
            )
            .as_bytes(),
        );
        let mut content: String = (0..each).map(|n| format!("/G{n} gs ")).collect();
        let colour = if page + 1 == pages { seed } else { 1 };
        content.push_str(&String::from_utf8(icon_ops(colour)).expect("ASCII"));
        object(
            &mut out,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
            .as_bytes(),
        );
        for _ in 0..each {
            object(&mut out, b"<< /Type /ExtGState /CA 1 >>");
        }
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn pdf_objects_per_page() {
    // 105,000 objects before the last page, which changed: the bound is a
    // page's, not the document's.
    let drawn = drawn(
        "pdf-objects",
        Some(&many_objects(700, 150, 30)),
        &many_objects(700, 150, 250),
    );
    assert!(
        drawn
            .said
            .as_deref()
            .is_some_and(|said| said.starts_with("page 700 of 700 · ")),
        "{drawn:?}"
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

/// A page drawing one image whose dictionary is `dict` and whose data is
/// `data`.
fn image_page(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image {dict} /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    image.extend_from_slice(data);
    image.extend_from_slice(b"\nendstream");
    pdf_doc(
        &[(24, 24, b"q 24 0 0 24 0 0 cm /Im0 Do Q")],
        "/XObject << /Im0 5 0 R >>",
        1,
        &[&image],
        "",
    )
}

/// The most a bomb may add to peak memory, in MiB: the 256 MiB limit, half
/// of it again for the last growth, which copies where the allocator cannot
/// grow in place (every decoder grows in powers of two, never past what is
/// left, so it copies from at most 128), and 64 for the test's own fixtures.
const HELD: u64 = 256 + 128 + 64;

/// `bomb` reads "could not be decoded", within the worker's limit and what
/// holding it costs.
fn bounded(name: &str, bomb: &[u8]) {
    bounded_within(name, bomb, HELD);
}

/// [`bounded`], its peak no more than `mib` above where it started.
fn bounded_within(name: &str, bomb: &[u8], mib: u64) {
    let before = support::peak_resident();
    let drawn = drawn(name, Some(&icon_pdf(1)), bomb);
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        assert!(
            after < before + mib,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

/// A grey 8 by 8 image, which every bomb below claims to be.
const GREY: &str = "/Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8";

#[test]
fn pdf_lenient_flate_bomb() {
    let data = zlib_zeros_lenient(1 << 30);
    bounded(
        "pdf-lenient-flate",
        &image_page(&format!("{GREY} /Filter /FlateDecode"), &data),
    );
}

#[test]
fn pdf_lzw_bomb() {
    let data = lzw_zeros(1 << 30);
    bounded(
        "pdf-lzw",
        &image_page(&format!("{GREY} /Filter /LZWDecode"), &data),
    );
}

#[test]
fn pdf_lzw_repeat_bomb() {
    let data = lzw_repeats(1 << 30);
    bounded(
        "pdf-lzw-repeat",
        &image_page(&format!("{GREY} /Filter /LZWDecode"), &data),
    );
}

#[test]
fn pdf_run_length_bomb() {
    let data = run_length_zeros(768 << 20);
    bounded(
        "pdf-run-length",
        &image_page(&format!("{GREY} /Filter /RunLengthDecode"), &data),
    );
}

#[test]
fn pdf_ccitt_bomb() {
    // A row of a million white pixels costs one bit.
    let data = ccitt_white(1024);
    bounded(
        "pdf-ccitt",
        &image_page(
            "/Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 1000000 /Rows 1024 >>",
            &data,
        ),
    );
}

#[test]
fn pdf_ascii85_bomb() {
    // 200 MiB of `z` inflates within the limit, and each `z` is four zeros.
    let data = zlib_filled(b'z', 200 << 20);
    bounded(
        "pdf-ascii85",
        &image_page(
            &format!("{GREY} /Filter [/FlateDecode /ASCII85Decode]"),
            &data,
        ),
    );
}

#[test]
fn pdf_predictor_in_place() {
    // 250 MiB of eight-bit rows, within the limit: the predictor undoes them
    // in place, so the picture draws without a second buffer.
    let before = support::peak_resident();
    let drawn = drawn(
        "pdf-predictor-in-place",
        Some(&icon_pdf(1)),
        &image_page(
            &format!(
                "{GREY} /Filter /FlateDecode /DecodeParms << /Predictor 12 /Colors 1 /BitsPerComponent 8 /Columns 1024 >>"
            ),
            &zlib_zeros(250 << 20),
        ),
    );
    assert!(
        matches!(drawn.verdict, Some(Verdict::Changed { .. })),
        "{drawn:?}"
    );
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        assert!(
            after < before + HELD,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_predictor_two_buffers() {
    // The same as sixteen-bit TIFF rows, which the predictor writes to a
    // second buffer: the two together pass the limit.
    bounded(
        "pdf-predictor-two",
        &image_page(
            "/Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 16 /Filter /FlateDecode /DecodeParms << /Predictor 2 /Colors 1 /BitsPerComponent 16 /Columns 1024 >>",
            &zlib_zeros(250 << 20),
        ),
    );
}

#[test]
fn pdf_predictor_row() {
    // A row 128 TiB long, over 64 bytes of data: the pane draws on.
    let drawn = drawn(
        "pdf-predictor",
        Some(&icon_pdf(1)),
        &image_page(
            &format!(
                "{GREY} /Filter /FlateDecode /DecodeParms << /Predictor 2 /BitsPerComponent 16 /Colors 1 /Columns 70368744177664 >>"
            ),
            &zlib_zeros(64),
        ),
    );
    assert!(drawn.verdict.is_some(), "{drawn:?}");
}

/// An 8 by 8 JPEG whose header claims `side` by `side`.
fn jpeg_claiming(side: u16) -> Vec<u8> {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode(&[128; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
        .expect("a JPEG");
    let frame = jpeg
        .windows(2)
        .position(|marker| marker == [0xff, 0xc0])
        .expect("a baseline frame");
    jpeg[frame + 5..frame + 9].copy_from_slice(&[side.to_be_bytes(), side.to_be_bytes()].concat());
    jpeg
}

/// A JPEG 2000 codestream of one `side` by `side` tile with no data, in
/// `components` 8-bit components.
fn jpx_claiming(side: u32, components: u16) -> Vec<u8> {
    let side = side.to_be_bytes();
    let zero = 0u32.to_be_bytes();
    let length = (38 + 3 * components).to_be_bytes();
    let each = [0x07, 0x01, 0x01].repeat(usize::from(components));
    [
        &[0xff, 0x4f][..],
        // The image and tile size, and each component's depth.
        &[0xff, 0x51],
        &length,
        &[0x00, 0x00],
        &side,
        &side,
        &zero,
        &zero,
        &side,
        &side,
        &zero,
        &zero,
        &components.to_be_bytes(),
        &each,
        // One layer, no decomposition, 64 by 64 code blocks, reversible.
        &[
            0xff, 0x52, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x04, 0x04, 0x00, 0x01,
        ],
        &[0xff, 0x5c, 0x00, 0x04, 0x40, 0x40],
        // One tile part, an empty packet for each component.
        &[0xff, 0x90, 0x00, 0x0a, 0x00, 0x00],
        &(14 + u32::from(components)).to_be_bytes(),
        &[0x00, 0x01, 0xff, 0x93],
        &vec![0; usize::from(components)],
        &[0xff, 0xd9],
    ]
    .concat()
}

#[test]
fn pdf_jpx_bomb() {
    // Refused from its header, before a sample is decoded.
    bounded_within(
        "pdf-jpx",
        &image_page(
            "/Width 8 /Height 8 /Filter /JPXDecode",
            &jpx_claiming(11_000, 1),
        ),
        128,
    );
}

#[test]
fn pdf_jpx_components() {
    // 3000 by 3000 in four components: small as pixels, not as samples.
    bounded_within(
        "pdf-jpx-components",
        &image_page(
            "/Width 8 /Height 8 /Filter /JPXDecode",
            &jpx_claiming(3_000, 4),
        ),
        128,
    );
}

#[test]
fn pdf_jpeg_working_set() {
    // Its pixels fit the limit, and its samples with their coefficients do
    // not.
    let drawn = drawn(
        "pdf-jpeg-working-set",
        Some(&icon_pdf(1)),
        &image_page(
            "/Width 7000 /Height 7000 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
            &jpeg_claiming(7_000),
        ),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
}

/// A JBIG2 page of `side` by `side` with nothing on it.
fn jbig2_claiming(side: u32) -> Vec<u8> {
    let side = side.to_be_bytes();
    [
        // Segment 0: page information, 19 bytes, on page 1.
        &[0, 0, 0, 0, 48, 0, 1, 0, 0, 0, 19][..],
        &side,
        &side,
        &[0; 8],
        &[0, 0, 0],
        // Segment 1: the end of the page.
        &[0, 0, 0, 1, 49, 0, 1, 0, 0, 0, 0],
    ]
    .concat()
}

#[test]
fn pdf_jbig2_bomb() {
    // Refused from its page size, before a pixel is decoded.
    bounded_within(
        "pdf-jbig2",
        &image_page(
            "/Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode",
            &jbig2_claiming(40_000),
        ),
        128,
    );
}

#[test]
fn pdf_jpeg_bomb() {
    // Refused from its header, before a pixel is decoded.
    bounded_within(
        "pdf-jpeg",
        &image_page(
            "/Width 11500 /Height 11500 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
            &jpeg_claiming(11_500),
        ),
        128,
    );
}

#[test]
fn pdf_flate_bomb_undecoded() {
    // About 7 MB of file that inflates to a gigabyte, as an image the frame
    // path skips and the worker must not decode.
    let before = support::peak_resident();
    let drawn = drawn(
        "pdf-flate-bomb",
        Some(&icon_pdf(1)),
        &imaged(1, (8_000, 8_000), 1 << 30),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        // The worker's limit, 256 MiB, and what holding it costs.
        assert!(
            after < before + HELD,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_small_image_bomb() {
    // An image that claims 8 by 8 pixels and inflates to a gigabyte: only the
    // inflate itself can stop it.
    let before = support::peak_resident();
    let drawn = drawn(
        "pdf-small-image-bomb",
        Some(&icon_pdf(1)),
        &imaged(1, (8, 8), 1 << 30),
    );
    assert_eq!(drawn.verdict, Some(Verdict::Undecoded), "{drawn:?}");
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        assert!(
            after < before + HELD,
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
    let before = support::peak_resident();
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
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        assert!(
            after < before + 64,
            "memory went from {before} MiB to {after} MiB"
        );
    }
}

#[test]
fn pdf_content_bomb_undecoded() {
    // A page whose content inflates to a gigabyte.
    let before = support::peak_resident();
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
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        assert!(
            after < before + HELD,
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
