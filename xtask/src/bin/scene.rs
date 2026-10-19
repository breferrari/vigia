//! The scene bed: a throwaway repository holding every picture state beside
//! ordinary code edits, for a reader to run `vigia` on. `SPEC.md` §7.
//!
//! `cargo run -p xtask --bin scene -- static` writes every change at once.
//! `cargo run -p xtask --bin scene -- replay` writes them in bursts, the way
//! an agent does.

#[path = "../../../crates/vigia-core/tests/support/assets.rs"]
#[allow(dead_code)]
mod assets;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use image::ImageFormat;

const USAGE: &str = "usage: cargo run -p xtask --bin scene -- static|replay";

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    let replay = match mode.as_deref() {
        Some("static") => false,
        Some("replay") => true,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("scene: no `git` on PATH, and the bed is a git repository");
        return ExitCode::FAILURE;
    }
    let dir = fresh_dir();
    let scene = Scene::new(&dir);
    scene.baseline();
    println!("vigia {}", dir.display());
    if replay {
        println!();
        scene.replay(1.0, |line| println!("{line}"));
    } else {
        scene.changes();
        println!();
        for (path, shows) in STATES {
            println!("{path:<28} {shows}");
        }
    }
    ExitCode::SUCCESS
}

/// A directory under the system temp dir that did not exist before.
fn fresh_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let base = std::env::temp_dir();
    for n in 0.. {
        let dir = base.join(format!("vigia-scene-{}-{stamp}-{n}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    unreachable!("the counter does not end")
}

/// Every state `static` writes, by path, and what the pane shows for it.
const STATES: &[(&str, &str)] = &[
    ("src/added.rs", "added source"),
    ("src/deleted.rs", "deleted source"),
    ("src/small.rs", "a one-line hunk"),
    ("src/medium.rs", "an eight-line hunk"),
    ("src/large.rs", "a forty-line hunk"),
    ("png/added.png", "added: one panel"),
    ("png/deleted.png", "deleted: one panel"),
    ("png/modified.png", "modified, same size: three panels"),
    ("png/resized.png", "resized: three panels, sizes differ"),
    (
        "png/after.png",
        "staged as a rename of png/before.png (press a), then changed",
    ),
    ("jpeg/", "the same five states as png/"),
    ("gif/", "the same five states as png/"),
    ("webp/", "the same five states as png/"),
    ("png/one-pixel.png", "1 px changed"),
    ("png/profile.png", "colour profile changed"),
    ("gif/animated.gif", "first frames compared"),
    (
        "broken/truncated.png",
        "cut in its pixels: could not be decoded",
    ),
    ("broken/truncated-header.png", "cut in its header: binary"),
    ("broken/corrupt.png", "pixels garbled: could not be decoded"),
    (
        "broken/too-large.png",
        "past the decode limit: could not be decoded",
    ),
    ("lfs/pointer.png", "an LFS pointer, drawn from its object"),
    ("bin/blob.dat", "binary"),
    ("svg/icon.svg", "SVG, shapes changed"),
    ("svg/label.svg", "SVG, only its text changed"),
    (
        "svg/costly.svg",
        "SVG refused for cost: could not be decoded",
    ),
];

/// The raster formats every image state is written in, with their extension.
const FORMATS: [(ImageFormat, &str, &str); 4] = [
    (ImageFormat::Png, "png", "png"),
    (ImageFormat::Jpeg, "jpeg", "jpg"),
    (ImageFormat::Gif, "gif", "gif"),
    (ImageFormat::WebP, "webp", "webp"),
];

/// An image `width` by `height` in `format`, with the pixel at `changed`
/// recoloured. One pixel near the end keeps most of a file's bytes, so a
/// rename with this change is still matched as a rename.
fn image_with(
    format: ImageFormat,
    width: u32,
    height: u32,
    seed: u8,
    changed: Option<(u32, u32)>,
) -> Vec<u8> {
    let mut image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([seed, (x * 3 % 256) as u8, (y * 5 % 256) as u8, 255])
    });
    if let Some((x, y)) = changed {
        image.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
    }
    assets::encode_image(&image, format)
}

/// Source text of `lines` numbered lines, with `tag` in each.
fn source(lines: usize, tag: &str) -> String {
    (1..=lines)
        .map(|n| format!("fn line_{n}() -> &'static str {{ \"{tag}\" }}\n"))
        .collect()
}

/// `base` with `count` lines from `from` on rewritten.
fn edited(lines: usize, from: usize, count: usize) -> String {
    (1..=lines)
        .map(|n| {
            if (from..from + count).contains(&n) {
                format!("fn line_{n}() -> &'static str {{ \"edited\" }}\n")
            } else {
                format!("fn line_{n}() -> &'static str {{ \"base\" }}\n")
            }
        })
        .collect()
}

/// The bed's repository, and how to write into it.
struct Scene {
    dir: PathBuf,
    /// Every file written or removed, for the test that holds them inside
    /// `dir`.
    touched: std::cell::RefCell<Vec<PathBuf>>,
}

impl Scene {
    /// A new repository at `dir`, configured so nothing of the reader's own
    /// git config changes what it shows.
    fn new(dir: &Path) -> Self {
        let scene = Self {
            dir: dir.to_path_buf(),
            touched: std::cell::RefCell::default(),
        };
        scene.git(&["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("core.autocrlf", "false"),
            ("core.safecrlf", "false"),
            ("core.fsmonitor", "false"),
            ("core.untrackedCache", "false"),
            ("commit.gpgsign", "false"),
            ("user.email", "scene@example.invalid"),
            ("user.name", "vigia scene"),
        ] {
            scene.git(&["config", key, value]);
        }
        scene
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `rela` under the scene's directory, refusing a path that would leave
    /// it.
    fn inside(&self, rela: &str) -> PathBuf {
        let rela = Path::new(rela);
        assert!(
            rela.components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "{} would leave the scene",
            rela.display()
        );
        let path = self.dir.join(rela);
        self.touched.borrow_mut().push(path.clone());
        path
    }

    fn write(&self, rela: &str, bytes: impl AsRef<[u8]>) {
        let path = self.inside(rela);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the scene's folders");
        }
        std::fs::write(&path, bytes).expect("write a scene file");
    }

    fn remove(&self, rela: &str) {
        std::fs::remove_file(self.inside(rela)).expect("remove a scene file");
    }

    /// Every file the changes start from, committed.
    fn baseline(&self) {
        self.write("src/deleted.rs", source(10, "base"));
        for (name, lines) in [("small", 20), ("medium", 40), ("large", 80)] {
            self.write(&format!("src/{name}.rs"), edited(lines, 0, 0));
        }
        self.write("src/feature.rs", "");
        for (format, folder, ext) in FORMATS {
            let file = |name: &str| format!("{folder}/{name}.{ext}");
            self.write(&file("deleted"), image_with(format, 96, 64, 10, None));
            self.write(&file("modified"), image_with(format, 96, 64, 20, None));
            self.write(&file("resized"), image_with(format, 48, 32, 30, None));
            self.write(&file("before"), image_with(format, 96, 64, 40, None));
        }
        self.write(
            "png/one-pixel.png",
            image_with(ImageFormat::Png, 96, 64, 50, None),
        );
        self.write(
            "png/profile.png",
            assets::profiled_png(96, 64, 60, b"profile one"),
        );
        self.write("gif/animated.gif", animated(70));
        for name in ["truncated", "truncated-header", "corrupt", "too-large"] {
            self.write(&format!("broken/{name}.png"), assets::png(48, 32, 80));
        }
        self.write("bin/blob.dat", blob(1));
        self.write("svg/icon.svg", assets::icon_svg(1));
        self.write("svg/label.svg", assets::labelled_svg("Save"));
        self.write("svg/costly.svg", assets::icon_svg(3));
        // Committed as text before the attribute exists, so no filter runs.
        let (oid, pointer) = assets::lfs_pointer(&lfs_old(), 0xab);
        self.write("lfs/pointer.png", pointer);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "baseline"]);
        self.write(
            ".gitattributes",
            "lfs/*.png filter=lfs diff=lfs merge=lfs -text\n",
        );
        self.git(&["add", ".gitattributes"]);
        self.git(&["commit", "-q", "-m", "lfs attribute"]);
        self.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
        self.write(
            &format!(".git/lfs/objects/{}/{}/{oid}", &oid[0..2], &oid[2..4]),
            lfs_old(),
        );
        // The index is refreshed after the clock has passed the commit, so no
        // file reads as racily clean and status never asks the LFS filter
        // (#659, whose fix removes this).
        std::thread::sleep(Duration::from_millis(1100));
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(["update-index", "-q", "--refresh"])
            .output();
    }

    /// Every change of `static`, written at once.
    fn changes(&self) {
        self.write("src/added.rs", source(12, "new"));
        self.remove("src/deleted.rs");
        self.write("src/small.rs", edited(20, 10, 1));
        self.write("src/medium.rs", edited(40, 12, 8));
        self.write("src/large.rs", edited(80, 20, 40));
        for (format, folder, ext) in FORMATS {
            let file = |name: &str| format!("{folder}/{name}.{ext}");
            self.write(&file("added"), image_with(format, 96, 64, 15, None));
            self.remove(&file("deleted"));
            self.write(&file("modified"), image_with(format, 96, 64, 25, None));
            self.write(&file("resized"), image_with(format, 64, 40, 30, None));
            // Staged as a rename, then changed: the staged run shows the
            // rename and the unstaged run the change.
            self.remove(&file("before"));
            self.write(&file("after"), image_with(format, 96, 64, 40, None));
            self.git(&["add", "--", &file("before"), &file("after")]);
            self.write(&file("after"), image_with(format, 96, 64, 41, None));
        }
        self.write(
            "png/one-pixel.png",
            image_with(ImageFormat::Png, 96, 64, 50, Some((40, 30))),
        );
        self.write(
            "png/profile.png",
            assets::profiled_png(96, 64, 60, b"profile two"),
        );
        self.write("gif/animated.gif", animated(90));
        let whole = assets::png(48, 32, 81);
        self.write("broken/truncated.png", &whole[..whole.len() / 2]);
        self.write("broken/truncated-header.png", &whole[..20]);
        self.write("broken/corrupt.png", assets::corrupt_png(48, 32, 81));
        self.write("broken/too-large.png", assets::claimed_png(20_000, 20_000));
        self.write(
            "lfs/pointer.png",
            image_with(ImageFormat::Png, 96, 64, 0xcd, None),
        );
        self.write("bin/blob.dat", blob(2));
        self.write("svg/icon.svg", assets::icon_svg(2));
        self.write("svg/label.svg", assets::labelled_svg("Load"));
        self.write("svg/costly.svg", assets::costly_svg());
    }

    /// The bursts of `replay`, each pause scaled by `pace`, with one line said
    /// per burst.
    fn replay(&self, pace: f64, mut say: impl FnMut(&str)) {
        let pause = |seconds: f64| std::thread::sleep(Duration::from_secs_f64(seconds * pace));
        say("an agent writes src/feature.rs, line by line");
        for lines in 1..=12 {
            self.write("src/feature.rs", source(lines, "feature"));
            pause(0.15);
        }
        pause(3.0);
        say("png/modified.png rewritten");
        self.write(
            "png/modified.png",
            image_with(ImageFormat::Png, 96, 64, 25, None),
        );
        pause(3.0);
        say("svg/new.svg added");
        self.write("svg/new.svg", assets::icon_svg(4));
        pause(3.0);
        say("png/before.png renamed to png/after.png, and changed");
        self.remove("png/before.png");
        self.write(
            "png/after.png",
            image_with(ImageFormat::Png, 96, 64, 40, Some((90, 63))),
        );
        pause(3.0);
        say("jpeg/deleted.jpg and src/deleted.rs deleted");
        self.remove("jpeg/deleted.jpg");
        self.remove("src/deleted.rs");
        pause(3.0);
        say("broken/flaky.png written corrupt, then fixed");
        self.write("broken/flaky.png", assets::corrupt_png(48, 32, 83));
        pause(2.0);
        self.write("broken/flaky.png", assets::png(48, 32, 83));
        pause(3.0);
        say("gif/animated.gif changed");
        self.write("gif/animated.gif", animated(90));
        say("done");
    }
}

/// The LFS object the committed pointer names.
fn lfs_old() -> Vec<u8> {
    image_with(ImageFormat::Png, 96, 64, 0xab, None)
}

/// A two-frame GIF whose first frame comes from `seed`.
fn animated(seed: u8) -> Vec<u8> {
    let frame = |seed: u8| {
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([seed, (x * 5 % 256) as u8, (y * 7 % 256) as u8, 255])
        })
    };
    assets::animated_gif(&[frame(seed), frame(seed.wrapping_add(100))])
}

/// Bytes that are binary and no image: NULs, from `seed`.
fn blob(seed: u8) -> Vec<u8> {
    (0..512u32)
        .map(|n| {
            if n % 7 == 0 {
                0
            } else {
                (n as u8).wrapping_mul(seed)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vigia_core::{ChangeKind, Worktree};

    /// What the core makes of one changed path.
    #[derive(Debug)]
    struct Read {
        kind: ChangeKind,
        binary: bool,
        before: bool,
        after: bool,
        added: u32,
    }

    /// The static scene, built in a fresh directory.
    fn built() -> PathBuf {
        let dir = fresh_dir();
        let scene = Scene::new(&dir);
        scene.baseline();
        scene.changes();
        dir
    }

    fn read(dir: &Path) -> BTreeMap<String, Read> {
        let worktree = Worktree::discover(dir).expect("the scene is a repository");
        let mut frame = worktree.frame();
        frame.show_staged(true);
        frame.advance().expect("advance");
        (0..frame.files().len())
            .map(|index| {
                let (change, diff) = frame.diff(index).expect("diff");
                let picture = diff.picture;
                let staged = matches!(change.origin, vigia_core::Origin::Staged);
                (
                    format!("{}{}", if staged { "staged:" } else { "" }, change.path),
                    Read {
                        kind: change.kind.clone(),
                        binary: diff.binary,
                        before: picture.is_some_and(|picture| picture.before.is_some()),
                        after: picture.is_some_and(|picture| picture.after.is_some()),
                        added: diff.added,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn scene_static_states() {
        let dir = built();
        let found = read(&dir);
        let at = |path: &str| {
            found
                .get(path)
                .unwrap_or_else(|| panic!("{path} is not listed: {:?}", found.keys()))
        };
        assert_eq!(at("src/added.rs").kind, ChangeKind::Added);
        assert_eq!(at("src/deleted.rs").kind, ChangeKind::Removed);
        for (path, lines) in [
            ("src/small.rs", 1),
            ("src/medium.rs", 8),
            ("src/large.rs", 40),
        ] {
            assert_eq!(at(path).added, lines, "{path} has the wrong hunk");
        }
        for (_, folder, ext) in FORMATS {
            let file = |name: &str| format!("{folder}/{name}.{ext}");
            let sides = |name: &str| {
                let read = at(&file(name));
                (read.before, read.after)
            };
            assert_eq!(sides("added"), (false, true), "{}", file("added"));
            assert_eq!(sides("deleted"), (true, false), "{}", file("deleted"));
            assert_eq!(sides("modified"), (true, true), "{}", file("modified"));
            assert_eq!(sides("resized"), (true, true), "{}", file("resized"));
            assert_eq!(
                at(&format!("staged:{}", file("after"))).kind,
                ChangeKind::Renamed {
                    from: file("before")
                },
                "{} is not a staged rename",
                file("after")
            );
            assert_eq!(sides("after"), (true, true), "{}", file("after"));
        }
        for path in [
            "png/one-pixel.png",
            "png/profile.png",
            "gif/animated.gif",
            "broken/truncated.png",
            "broken/corrupt.png",
            "broken/too-large.png",
            "lfs/pointer.png",
            "svg/icon.svg",
            "svg/label.svg",
            "svg/costly.svg",
        ] {
            let read = at(path);
            assert!(
                read.before && read.after,
                "{path} has no size line: {read:?}"
            );
        }
        for path in ["broken/truncated-header.png", "bin/blob.dat"] {
            let read = at(path);
            assert!(
                read.binary && !read.before && !read.after,
                "{path} is not plain binary: {read:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the pane drew for one file, over every view that showed it.
    #[derive(Debug, Default)]
    struct Drawn {
        /// Its size line's text and what the built picture said after it.
        sized: Option<(String, Option<String>)>,
        picture_rows: usize,
        reasons: Vec<String>,
        hunks: usize,
    }

    /// The static scene drawn by `vigia`'s own view at `width` by `height`,
    /// scrolled from top to bottom, with every picture built while it is on
    /// screen. Each view is also painted into a buffer.
    fn drawn(dir: &Path, width: u16, height: u16) -> BTreeMap<String, Drawn> {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use vigia::pictures::{Inks, Pictures};
        use vigia::{Action, App, Glyphs, Pointing, Row, Theme, body_layout, render};
        use vigia_core::{Highlighter, History, Standing};

        let worktree = Worktree::discover(dir).expect("the scene is a repository");
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut app = App::new();
        app.draw_pictures(true);
        let mut pictures = Pictures::new(
            Inks {
                canvas: (0x0d, 0x11, 0x17),
                changed: (0xf8, 0x51, 0x49),
            },
            ratatui_image::picker::Picker::halfblocks(),
            Box::new(|| {}),
        );
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let area = Rect::new(0, 0, width, height);
        let chrome = |app: &App| {
            app.chrome(
                "scene",
                None,
                vigia::Stood {
                    standing: &Standing::Current,
                    now: 0,
                },
                Pointing::default(),
                Default::default(),
                "",
            )
        };
        let mut found: BTreeMap<String, Drawn> = BTreeMap::new();
        let files = frame.files().len();
        // One view per file, each starting at its heading.
        for _ in 0..files {
            let body = body_layout(
                area,
                &chrome(&app),
                frame.files().len(),
                vigia::list_rows_wanted(frame.files()),
            );
            let mut view = app
                .view(&mut frame, &mut highlighter, &history, body)
                .expect("view");
            pictures.attach(&mut view.rows, &frame);
            assert!(
                pictures.settle(Duration::from_secs(60)),
                "the pictures were not built"
            );
            let mut view = app
                .view(&mut frame, &mut highlighter, &history, body)
                .expect("view");
            pictures.attach(&mut view.rows, &frame);
            render(
                &mut Buffer::empty(area),
                area,
                &view,
                &Theme::dark(),
                Glyphs::default(),
                &chrome(&app),
            );
            let mut heading: Option<String> = None;
            for row in &view.rows {
                let path_of = |file: usize| frame.files()[file].path.clone();
                match row {
                    Row::File(entry) => heading = Some(entry.path.clone()),
                    Row::Sized {
                        line, file, ready, ..
                    } => {
                        let suffix = ready.as_ref().and_then(|ready| ready.0.suffix());
                        found.entry(path_of(*file)).or_default().sized =
                            Some((line.clone(), suffix));
                    }
                    Row::Picture { file, .. } => {
                        found.entry(path_of(*file)).or_default().picture_rows += 1
                    }
                    Row::Reason(reason) => {
                        if let Some(path) = &heading {
                            found
                                .entry(path.clone())
                                .or_default()
                                .reasons
                                .push(reason.clone());
                        }
                    }
                    Row::Hunk { .. } => {
                        if let Some(path) = &heading {
                            found.entry(path.clone()).or_default().hunks += 1;
                        }
                    }
                    _ => {}
                }
            }
            app.apply(Action::File(1), &mut frame, body.diff)
                .expect("next file");
        }
        found
    }

    #[test]
    fn scene_renders() {
        let dir = built();
        for (width, height) in [(120, 40), (80, 24)] {
            let found = drawn(&dir, width, height);
            let at = |path: &str| {
                found.get(path).unwrap_or_else(|| {
                    panic!(
                        "{path} was never drawn at {width}x{height}: {:?}",
                        found.keys()
                    )
                })
            };
            let said = |path: &str| {
                at(path)
                    .sized
                    .clone()
                    .unwrap_or_else(|| {
                        panic!(
                            "{path} drew no size line at {width}x{height}: {:?}",
                            at(path)
                        )
                    })
                    .1
                    .unwrap_or_default()
            };
            for (_, folder, ext) in FORMATS {
                for name in ["added", "deleted", "modified", "resized", "after"] {
                    let path = format!("{folder}/{name}.{ext}");
                    assert!(
                        at(&path).sized.is_some(),
                        "{path} drew no size line at {width}x{height}"
                    );
                    assert!(
                        at(&path).picture_rows > 0,
                        "{path} drew no picture at {width}x{height}"
                    );
                }
            }
            for path in [
                "png/one-pixel.png",
                "gif/animated.gif",
                "lfs/pointer.png",
                "svg/icon.svg",
                "svg/label.svg",
            ] {
                assert!(
                    said(path).contains("px changed"),
                    "{path} at {width}x{height}: {:?}",
                    at(path)
                );
                assert!(
                    at(path).picture_rows > 0,
                    "{path} drew no picture at {width}x{height}"
                );
            }
            assert!(
                said("png/one-pixel.png").starts_with("1 px changed"),
                "{:?}",
                at("png/one-pixel.png")
            );
            assert_eq!(said("png/profile.png"), "colour profile changed");
            for path in [
                "broken/truncated.png",
                "broken/corrupt.png",
                "broken/too-large.png",
                "svg/costly.svg",
            ] {
                assert_eq!(
                    said(path),
                    "could not be decoded",
                    "{path} at {width}x{height}"
                );
            }
            for path in ["broken/truncated-header.png", "bin/blob.dat"] {
                assert_eq!(at(path).reasons, ["binary"], "{path} at {width}x{height}");
            }
            for path in [
                "src/added.rs",
                "src/small.rs",
                "src/medium.rs",
                "src/large.rs",
            ] {
                assert!(
                    at(path).hunks > 0,
                    "{path} drew no hunk at {width}x{height}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_replay_ends() {
        let dir = fresh_dir();
        let scene = Scene::new(&dir);
        scene.baseline();
        let mut said = Vec::new();
        scene.replay(0.0, |line| said.push(line.to_owned()));
        assert_eq!(said.last().map(String::as_str), Some("done"), "{said:?}");
        let found = read(&dir);
        let at = |path: &str| {
            found
                .get(path)
                .unwrap_or_else(|| panic!("{path} is not listed: {:?}", found.keys()))
        };
        assert_eq!(
            at("src/feature.rs").added,
            12,
            "the agent's file is not whole"
        );
        assert_eq!(at("svg/new.svg").kind, ChangeKind::Added);
        assert_eq!(at("jpeg/deleted.jpg").kind, ChangeKind::Removed);
        assert_eq!(at("src/deleted.rs").kind, ChangeKind::Removed);
        let flaky = at("broken/flaky.png");
        assert!(flaky.after, "the fixed image does not size: {flaky:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_temp_only() {
        let dir = fresh_dir();
        assert!(
            dir.starts_with(std::env::temp_dir()),
            "{} is not under the temp dir",
            dir.display()
        );
        let scene = Scene::new(&dir);
        scene.baseline();
        scene.changes();
        let touched = scene.touched.borrow();
        assert!(
            touched.len() > 50,
            "only {} files were written",
            touched.len()
        );
        for path in touched.iter() {
            assert!(
                path.starts_with(&dir),
                "{} is outside the scene",
                path.display()
            );
        }
        for escape in [
            "../outside.txt",
            "/etc/outside.txt",
            "png/../../outside.txt",
        ] {
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scene.write(escape, "x");
            }));
            assert!(refused.is_err(), "{escape} was written");
        }
        drop(touched);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
