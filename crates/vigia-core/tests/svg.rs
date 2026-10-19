//! What a changed SVG reads as on the frame path: its size from the root tag,
//! or its text diff. `SPEC.md` §11.1.

mod support;

use support::{Scratch, changes_sorted};
use vigia_core::{FileDiff, Format};

fn diff_of(scratch: &Scratch, rela: &str) -> FileDiff {
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == rela)
        .unwrap_or_else(|| panic!("{rela} is not listed"));
    worktree.diff(change).expect("diff")
}

fn icon(root: &str, fill: &str) -> String {
    format!(r#"{root}<circle cx="12" cy="12" r="10" fill="{fill}"/></svg>"#)
}

/// One SVG changed from `old` to `new`, as its diff.
fn changed(name: &str, old: &str, new: &str) -> FileDiff {
    let scratch = Scratch::new(name);
    scratch.write("icon.svg", old);
    scratch.commit_all("initial");
    scratch.write("icon.svg", new);
    diff_of(&scratch, "icon.svg")
}

/// A side's format and size.
type Size = Option<(Format, u32, u32)>;

/// Each side's format and size.
fn sizes(diff: &FileDiff) -> Option<(Size, Size)> {
    let picture = diff.picture?;
    let size = |side: Option<vigia_core::ImageSide>| side.map(|s| (s.format, s.width, s.height));
    Some((size(picture.before), size(picture.after)))
}

#[test]
fn svg_size_line() {
    for (root, want) in [
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#,
            (24, 24),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="48px" height="16px">"#,
            (48, 16),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 20">"#,
            (32, 20),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 10 30">"#,
            (10, 30),
        ),
        (
            r#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 7.5 3'>"#,
            (8, 3),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="48" viewBox="0 0 24 12">"#,
            (48, 24),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" height="10" viewBox="0 0 30 15">"#,
            (20, 10),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="50%" viewBox="0 0 10 30">"#,
            (5, 15),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="50%" viewBox="0 0 10 30">"#,
            (40, 15),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297">"#,
            (794, 1123),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1in" height="2cm">"#,
            (96, 76),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" font-size="10" width="2em" height="1ex">"#,
            (20, 5),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="auto" height="10" viewBox="0 0 30 15">"#,
            (20, 10),
        ),
    ] {
        let diff = changed("svg-size", &icon(root, "red"), &icon(root, "blue"));
        let side = Some((Format::Svg, want.0, want.1));
        assert_eq!(sizes(&diff), Some((side, side)), "{root}");
        assert!(
            diff.binary && diff.hunks.is_empty(),
            "a drawn SVG kept its text: {root}"
        );
    }
}

#[test]
fn svg_prolog_skipped() {
    let root = concat!(
        "\u{feff}<?xml version=\"1.0\"?>\r\n",
        "<!-- an icon -->\r\n",
        "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x\" [ <!ENTITY a \"b>c\"> ]>\r\n",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"\r\n width=\"24\" height=\"24\">"
    );
    let diff = changed("svg-prolog", &icon(root, "red"), &icon(root, "blue"));
    let side = Some((Format::Svg, 24, 24));
    assert_eq!(
        sizes(&diff),
        Some((side, side)),
        "a prolog, comment or doctype hid the root"
    );
}

#[test]
fn svg_unparsable_keeps_text() {
    for root in [
        "<svg width=\"24\">",
        "<html><svg width=\"24\" height=\"24\">",
        "<svg width=\"0\" height=\"24\">",
        "<svg width=\"24\" height=\"24\"",
        "<svgx width=\"24\" height=\"24\">",
        "<svg viewBox=\"0 0 -24 24\">",
    ] {
        let diff = changed("svg-unparsable", &icon(root, "red"), &icon(root, "blue"));
        assert!(
            diff.picture.is_none() && !diff.binary && !diff.hunks.is_empty(),
            "an SVG with no size from its root did not keep its text diff: {root}"
        );
    }
}

#[test]
fn svg_attribute_keeps_text() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let scratch = Scratch::new("svg-attribute");
    scratch.write(".gitattributes", "*.svg diff\n");
    scratch.write("icon.svg", icon(root, "red"));
    scratch.commit_all("initial");
    scratch.write("icon.svg", icon(root, "blue"));
    let diff = diff_of(&scratch, "icon.svg");
    assert!(
        diff.picture.is_none() && !diff.hunks.is_empty(),
        "a diff attribute did not keep the text diff"
    );
}

#[test]
fn svgz_stays_binary() {
    let scratch = Scratch::new("svgz");
    scratch.write("icon.svgz", [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 1, 2, 0]);
    scratch.commit_all("initial");
    scratch.write("icon.svgz", [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 4, 5, 0]);
    let diff = diff_of(&scratch, "icon.svgz");
    assert!(diff.binary && diff.picture.is_none(), "an svgz was drawn");
}

#[test]
fn svg_root_tag_only() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let body: String = (0..60_000)
        .map(|i| format!(r#"<rect x="{}" y="1" width="1" height="1"/>"#, i % 24))
        .collect();
    let big = format!("{root}{body}</svg>");
    assert!(big.len() > 2 * 1024 * 1024, "the fixture is not large");
    let scratch = Scratch::new("svg-large");
    scratch.write("big.svg", &big);
    let diff = diff_of(&scratch, "big.svg");
    assert_eq!(sizes(&diff), Some((None, Some((Format::Svg, 24, 24)))));
    assert!(
        diff.bytes <= 64 * 1024,
        "the frame path read {} bytes of a large SVG for its size",
        diff.bytes
    );
}

#[test]
fn svg_counted_binary() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let diff = changed("svg-counted", &icon(root, "red"), &icon(root, "blue"));
    assert!(diff.binary, "a drawn SVG is not counted as binary");
    assert_eq!(
        (diff.added, diff.removed),
        (0, 0),
        "a drawn SVG's lines count toward the total"
    );
}

#[test]
fn svg_measured_binary() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let scratch = Scratch::new("svg-measured");
    scratch.write("icon.svg", icon(root, "red"));
    scratch.commit_all("initial");
    scratch.write("icon.svg", icon(root, "blue"));
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let span = worktree.measure(&changes[0]).expect("measure");
    assert!(
        span.binary && span.hunks == 0 && span.lines == 0,
        "the height walk measured a drawn SVG as text: {span:?}"
    );
}

#[test]
fn svg_utf16_keeps_text() {
    let utf16 = |fill: &str| -> Vec<u8> {
        let text = icon(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#,
            fill,
        );
        [0xff, 0xfe]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect()
    };
    let scratch = Scratch::new("svg-utf16");
    scratch.write("icon.svg", utf16("red"));
    scratch.commit_all("initial");
    scratch.write("icon.svg", utf16("blue"));
    let diff = diff_of(&scratch, "icon.svg");
    assert!(
        diff.picture.is_none(),
        "a UTF-16 SVG, which usvg cannot read, was drawn"
    );
}

#[test]
fn svg_rename_drawn() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let scratch = Scratch::new("svg-rename");
    scratch.write("old.svg", icon(root, "red"));
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("old.svg")).expect("remove");
    scratch.write("new.svg", icon(root, "red"));
    scratch.git(&["add", "-A"]);
    scratch.write("new.svg", icon(root, "blue"));
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert!(!changes.is_empty(), "the rename was not listed");
    for change in &changes {
        let diff = worktree.diff(change).expect("diff");
        assert!(
            diff.picture.is_some(),
            "{} of a renamed SVG was not drawn",
            change.path
        );
    }
}

#[test]
fn svg_no_pictures_text() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let scratch = Scratch::new("svg-no-pictures");
    scratch.write("icon.svg", icon(root, "red"));
    scratch.commit_all("initial");
    scratch.write("icon.svg", icon(root, "blue"));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    // Settled, so the drawn diff is cached and the switch has to drop it.
    support::settle(&mut frame);
    let drawn = |frame: &mut vigia_core::Frame| {
        let (_, diff) = frame.diff(0).expect("diff");
        (diff.binary, diff.picture.is_some(), diff.added)
    };
    assert_eq!(drawn(&mut frame), (true, true, 0), "a drawn SVG");
    frame.draw_svg(false);
    assert_eq!(
        drawn(&mut frame),
        (false, false, 1),
        "an SVG in a pane that draws no picture lost its text diff"
    );
    frame.draw_svg(true);
    assert_eq!(
        drawn(&mut frame),
        (true, true, 0),
        "the SVG was not drawn again"
    );
}

#[test]
fn svg_blob_head_only() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let late = format!("<!--{}-->{}", "x".repeat(70 * 1024), icon(root, "red"));
    let scratch = Scratch::new("svg-blob-head");
    scratch.write("icon.svg", &late);
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("icon.svg")).expect("remove");
    let diff = diff_of(&scratch, "icon.svg");
    assert!(
        diff.picture.is_none(),
        "a committed SVG was sized from past the head a worktree side reads"
    );
}
