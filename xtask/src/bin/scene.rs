//! The scene bed: a throwaway repository holding every picture state beside
//! ordinary code edits, for a reader to run `vigia` on. `SPEC.md` §7.
//!
//! `cargo run -p xtask --bin scene -- static` writes every change at once.
//! `cargo run -p xtask --bin scene -- replay` writes them in bursts, the way
//! an agent does.

// Shared with the tests, which use generators this binary does not.
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
    let dir = match fresh_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("scene: cannot make a directory under the temp dir: {error}");
            return ExitCode::FAILURE;
        }
    };
    let scene = Scene::new(&dir);
    scene.baseline();
    println!("vigia {}", dir.display());
    println!();
    if replay {
        println!("Start vigia there, then press Enter to begin.");
        let mut enter = String::new();
        if !matches!(std::io::stdin().read_line(&mut enter), Ok(read) if read > 0) {
            println!("(stdin is closed, so it begins now)");
        }
        scene.replay(1.0, |line| println!("{line}"));
    } else {
        scene.changes();
        for (path, shows) in states() {
            println!("{path:<28} {shows}");
        }
    }
    ExitCode::SUCCESS
}

/// A directory under the system temp dir that did not exist before.
fn fresh_dir() -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let base = std::env::temp_dir();
    for n in 0.. {
        let dir = base.join(format!("vigia-scene-{}-{stamp}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    unreachable!("the counter does not end")
}

/// A raster format the image states are written in, and where.
struct Raster {
    format: ImageFormat,
    folder: &'static str,
    ext: &'static str,
}

impl Raster {
    fn file(&self, name: &str) -> String {
        format!("{}/{name}.{}", self.folder, self.ext)
    }
}

const RASTERS: [Raster; 4] = [
    Raster {
        format: ImageFormat::Png,
        folder: "png",
        ext: "png",
    },
    Raster {
        format: ImageFormat::Jpeg,
        folder: "jpeg",
        ext: "jpg",
    },
    Raster {
        format: ImageFormat::Gif,
        folder: "gif",
        ext: "gif",
    },
    Raster {
        format: ImageFormat::WebP,
        folder: "webp",
        ext: "webp",
    },
];

/// The five states each raster format is written in, and what they show.
const RASTER_STATES: [(&str, &str); 5] = [
    ("added", "added: one panel"),
    ("deleted", "deleted: one panel"),
    ("modified", "modified, same size: three panels"),
    ("resized", "resized: old and new"),
    ("after", "a staged rename (press a), then changed"),
];

/// Every state `static` writes, by path, and what the pane shows for it.
fn states() -> Vec<(String, &'static str)> {
    let mut states: Vec<(String, &'static str)> = [
        ("src/added.rs", "added source"),
        ("src/deleted.rs", "deleted source"),
        ("src/small.rs", "a one-line hunk"),
        ("src/medium.rs", "an eight-line hunk"),
        ("src/large.rs", "a forty-line hunk"),
    ]
    .into_iter()
    .map(|(path, shows)| (path.to_owned(), shows))
    .collect();
    for raster in &RASTERS {
        for (name, shows) in RASTER_STATES {
            states.push((raster.file(name), shows));
        }
    }
    states.extend(
        [
            ("png/one-pixel.png", "1 px changed"),
            (
                "png/moved.png",
                "renamed and changed, unstaged: old and new",
            ),
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
                "claims 20000×20000: could not be decoded",
            ),
            ("lfs/pointer.png", "an LFS pointer, drawn from its object"),
            (
                "lfs/missing.png",
                "an LFS pointer whose object is missing: binary",
            ),
            ("bin/blob.dat", "binary"),
            ("svg/icon.svg", "SVG, shapes changed"),
            ("svg/label.svg", "SVG, only its text changed"),
            (
                "svg/costly.svg",
                "SVG refused for cost: could not be decoded",
            ),
            ("pdf/modified.pdf", "PDF, its one page changed"),
            ("pdf/added.pdf", "PDF added: one panel"),
            ("pdf/deleted.pdf", "PDF deleted: one panel"),
            (
                "pdf/page-three.pdf",
                "page 3 of 12 drawn, the page that changed",
            ),
            ("pdf/pages.pdf", "12 → 13 pages: page 13 alone, as added"),
            ("pdf/fewer.pdf", "13 → 12 pages: page 13 alone, as removed"),
            ("pdf/cut.pdf", "cut short: binary"),
            ("pdf/locked.pdf", "needs a password: binary"),
            (
                "pdf/heavy.pdf",
                "a page too costly to draw: could not be decoded",
            ),
            (
                "pdf/large.pdf",
                "4 MiB: its bytes first, its size from the worker",
            ),
        ]
        .into_iter()
        .map(|(path, shows)| (path.to_owned(), shows)),
    );
    states
}

/// A PNG `width` by `height` from `seed`, with the pixel at `(x, y)`
/// recoloured.
fn png_with_pixel(width: u32, height: u32, seed: u8, (x, y): (u32, u32)) -> Vec<u8> {
    let mut image = assets::gradient(width, height, seed);
    image.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
    assets::encode_image(&image, ImageFormat::Png)
}

/// Source text of `count` numbered lines, each tagged by `tag_of` its number.
fn lines<'a>(count: usize, tag_of: impl Fn(usize) -> &'a str) -> String {
    (1..=count)
        .map(|n| format!("fn line_{n}() -> &'static str {{ \"{}\" }}\n", tag_of(n)))
        .collect()
}

/// `count` lines tagged `base`, with `edits` lines from `from` on tagged
/// `edited`.
fn edited(count: usize, from: usize, edits: usize) -> String {
    lines(count, |n| {
        if (from..from + edits).contains(&n) {
            "edited"
        } else {
            "base"
        }
    })
}

/// The seed of the LFS object, and of its pointer's oid.
const LFS_SEED: u8 = 0xab;

/// The LFS object the committed pointer names.
fn lfs_object() -> Vec<u8> {
    assets::png(96, 64, LFS_SEED)
}

const MISSING_SEED: u8 = 0x22;
/// The worktree's picture for the missing object, unlike the committed one.
const MISSING_NEW_SEED: u8 = 0x23;

/// The LFS object `lfs/missing.png`'s pointer names, which only `replay` lands.
fn missing_object() -> Vec<u8> {
    assets::png(96, 64, MISSING_SEED)
}

/// Where the store keeps the object `oid`.
fn object_path(oid: &str) -> String {
    format!(".git/lfs/objects/{}/{}/{oid}", &oid[0..2], &oid[2..4])
}

/// A two-frame GIF whose first frame comes from `seed`.
fn two_frame_gif(seed: u8) -> Vec<u8> {
    assets::animated_gif(&[
        assets::gradient(48, 32, seed),
        assets::gradient(48, 32, seed.wrapping_add(100)),
    ])
}

/// Twelve pages, each its own colour, with `page` (from 1) from `seed`.
fn twelve_pages(page: usize, seed: u8) -> Vec<u8> {
    let ops: Vec<Vec<u8>> = (1..=12)
        .map(|n| assets::icon_ops(if n == page { seed } else { n as u8 * 10 }))
        .collect();
    let pages: Vec<(u32, u32, &[u8])> = ops.iter().map(|ops| (24, 24, ops.as_slice())).collect();
    assets::pdf(&pages)
}

/// `count` pages, all alike.
fn repeated_pages(count: usize) -> Vec<u8> {
    let ops = assets::icon_ops(1);
    assets::pdf(&vec![(24, 24, ops.as_slice()); count])
}

/// A page whose one 10 KB content stream is listed `repeat` times: a short
/// file with a long page.
fn heavy_pdf(repeat: usize) -> Vec<u8> {
    let mut ops = assets::icon_ops(200);
    ops.push(b'\n');
    while ops.len() < 10_000 {
        ops.extend_from_slice(b"0.5 0.5 0.5 rg 1 1 3 3 re f\n");
    }
    assets::pdf_doc(&[(24, 24, &ops)], "", repeat, &[], "")
}

/// A 4 MiB PDF, past what the frame path sizes itself.
fn large_pdf() -> Vec<u8> {
    let mut ops = assets::icon_ops(200);
    ops.push(b'\n');
    while ops.len() < 4 << 20 {
        ops.extend_from_slice(b"0.5 0.5 0.5 rg 1 1 3 3 re f\n");
    }
    assets::pdf(&[(24, 24, &ops)])
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

/// The bed's repository, and how to write into it.
struct Scene {
    dir: PathBuf,
}

impl Scene {
    /// A new repository at `dir`, configured as every fixture repository is,
    /// so nothing of the reader's own git config changes what it shows.
    fn new(dir: &Path) -> Self {
        let scene = Self {
            dir: dir.to_path_buf(),
        };
        scene.git(&["init", "-q", "-b", "main"]);
        for (key, value) in assets::GIT_CONFIG {
            scene.git(&["config", key, value]);
        }
        scene
    }

    fn git(&self, args: &[&str]) {
        // An installed git-lfs marks its driver required in the global
        // config, and the bed's own commands must get past the missing one
        // it names for the pane.
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "filter.lfs.required=false"])
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
        self.dir.join(rela)
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

    /// Stage `before` renamed to `after`, then change `after` in the working
    /// tree: the staged run shows the rename and the unstaged run the change.
    fn rename_then_change(&self, raster: &Raster) {
        let (before, after) = (raster.file("before"), raster.file("after"));
        self.remove(&before);
        self.write(&after, assets::encoded(raster.format, 96, 64, 40));
        self.git(&["add", "--", &before, &after]);
        self.write(&after, assets::encoded(raster.format, 96, 64, 41));
    }

    /// Every file the changes start from, committed.
    fn baseline(&self) {
        self.write("src/deleted.rs", lines(10, |_| "base"));
        for (name, count) in [("small", 20), ("medium", 40), ("large", 80)] {
            self.write(&format!("src/{name}.rs"), edited(count, 0, 0));
        }
        self.write("src/feature.rs", "");
        for raster in &RASTERS {
            let format = raster.format;
            self.write(&raster.file("deleted"), assets::encoded(format, 96, 64, 10));
            self.write(
                &raster.file("modified"),
                assets::encoded(format, 96, 64, 20),
            );
            self.write(&raster.file("resized"), assets::encoded(format, 48, 32, 30));
            self.write(&raster.file("before"), assets::encoded(format, 96, 64, 40));
        }
        self.write("png/one-pixel.png", assets::png(96, 64, 50));
        self.write("png/original.png", assets::edited_png(96, 64).0);
        self.write(
            "png/profile.png",
            assets::profiled_png(96, 64, 60, b"profile one"),
        );
        self.write("gif/animated.gif", two_frame_gif(70));
        for name in ["truncated", "truncated-header", "corrupt", "too-large"] {
            self.write(&format!("broken/{name}.png"), assets::png(48, 32, 80));
        }
        self.write("bin/blob.dat", blob(1));
        self.write("svg/icon.svg", assets::icon_svg(1));
        self.write("svg/label.svg", assets::labelled_svg("Save"));
        self.write("svg/costly.svg", assets::icon_svg(3));
        for name in ["modified", "deleted", "cut", "locked", "heavy", "large"] {
            self.write(&format!("pdf/{name}.pdf"), assets::icon_pdf(1));
        }
        self.write("pdf/page-three.pdf", twelve_pages(3, 30));
        self.write("pdf/pages.pdf", repeated_pages(12));
        self.write("pdf/fewer.pdf", repeated_pages(13));
        // Committed as text before the attribute exists, so no filter runs.
        let object = lfs_object();
        let (oid, pointer) = assets::lfs_pointer(&object, LFS_SEED);
        self.write("lfs/pointer.png", pointer);
        let (_, unchanged) = assets::lfs_pointer(&assets::png(32, 32, 0x11), 0x11);
        self.write("lfs/unchanged.png", unchanged);
        let (_, missing) = assets::lfs_pointer(&missing_object(), MISSING_SEED);
        self.write("lfs/missing.png", missing);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "baseline"]);
        self.write(
            ".gitattributes",
            "lfs/*.png filter=lfs diff=lfs merge=lfs -text\n",
        );
        self.git(&["add", ".gitattributes"]);
        self.git(&["commit", "-q", "-m", "lfs attribute"]);
        self.write(&object_path(&oid), object);
        self.write("lfs/missing.png", assets::png(96, 64, MISSING_NEW_SEED));
        self.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
        self.racy("lfs/unchanged.png");
    }

    /// Make `rela`'s index entry racily clean: the index gets the file's own
    /// timestamp, so status must read the file again whatever the
    /// filesystem's timestamp resolution.
    fn racy(&self, rela: &str) {
        let modified = std::fs::metadata(self.inside(rela))
            .and_then(|meta| meta.modified())
            .expect("read a scene file's timestamp");
        std::fs::File::options()
            .write(true)
            .open(self.dir.join(".git/index"))
            .and_then(|index| index.set_modified(modified))
            .expect("set the index's timestamp");
    }

    /// Every change of `static`, written at once.
    fn changes(&self) {
        self.write("src/added.rs", lines(12, |_| "new"));
        self.remove("src/deleted.rs");
        self.write("src/small.rs", edited(20, 10, 1));
        self.write("src/medium.rs", edited(40, 12, 8));
        self.write("src/large.rs", edited(80, 20, 40));
        for raster in &RASTERS {
            let format = raster.format;
            // Several times the deleted one's size, so git, and the pane, never
            // weigh the two as a rename.
            self.write(&raster.file("added"), assets::encoded(format, 300, 200, 15));
            self.remove(&raster.file("deleted"));
            self.write(
                &raster.file("modified"),
                assets::encoded(format, 96, 64, 25),
            );
            self.write(&raster.file("resized"), assets::encoded(format, 64, 40, 30));
            self.rename_then_change(raster);
        }
        self.write("png/one-pixel.png", png_with_pixel(96, 64, 50, (40, 30)));
        self.remove("png/original.png");
        self.write("png/moved.png", assets::edited_png(96, 64).1);
        self.write(
            "png/profile.png",
            assets::profiled_png(96, 64, 60, b"profile two"),
        );
        self.write("gif/animated.gif", two_frame_gif(90));
        let whole = assets::png(48, 32, 81);
        self.write("broken/truncated.png", &whole[..whole.len() / 2]);
        self.write("broken/truncated-header.png", &whole[..20]);
        self.write("broken/corrupt.png", assets::corrupt_png(48, 32, 81));
        self.write("broken/too-large.png", assets::claimed_png(20_000, 20_000));
        self.write("lfs/pointer.png", assets::png(96, 64, 0xcd));
        self.write("bin/blob.dat", blob(2));
        self.write("svg/icon.svg", assets::icon_svg(2));
        self.write("svg/label.svg", assets::labelled_svg("Load"));
        self.write("svg/costly.svg", assets::costly_svg());
        self.write("pdf/modified.pdf", assets::icon_pdf(200));
        // Unlike the deleted one, so the two are not paired as a rename.
        self.write("pdf/added.pdf", twelve_pages(5, 77));
        self.remove("pdf/deleted.pdf");
        self.write("pdf/page-three.pdf", twelve_pages(3, 250));
        self.write("pdf/pages.pdf", repeated_pages(13));
        self.write("pdf/fewer.pdf", repeated_pages(12));
        let whole = assets::icon_pdf(2);
        // Each with a few NULs past its end, as a real PDF's streams carry.
        self.write("pdf/cut.pdf", [&whole[..60], b"\0\0\0"].concat());
        self.write(
            "pdf/locked.pdf",
            [assets::locked_pdf(), b"%\0\0\0\n".to_vec()].concat(),
        );
        self.write("pdf/heavy.pdf", heavy_pdf(2_400));
        self.write("pdf/large.pdf", large_pdf());
        // After the last index write: status must read it again and neither
        // start the driver nor list it.
        self.racy("lfs/unchanged.png");
    }

    /// The bursts of `replay`, each pause scaled by `pace`, with one line said
    /// per burst.
    fn replay(&self, pace: f64, mut say: impl FnMut(&str)) {
        let pause = |seconds: f64| std::thread::sleep(Duration::from_secs_f64(seconds * pace));
        say("an agent writes src/feature.rs, line by line");
        for count in 1..=12 {
            self.write("src/feature.rs", lines(count, |_| "feature"));
            pause(0.15);
        }
        pause(3.0);
        say("png/modified.png rewritten");
        self.write("png/modified.png", assets::png(96, 64, 25));
        pause(3.0);
        say("svg/new.svg added");
        self.write("svg/new.svg", assets::icon_svg(4));
        pause(3.0);
        say("png/before.png renamed to png/after.png and staged, then changed");
        self.rename_then_change(&RASTERS[0]);
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
        self.write("gif/animated.gif", two_frame_gif(90));
        pause(3.0);
        say("lfs/missing.png's object arrives, as git lfs fetch leaves it");
        let (oid, _) = assets::lfs_pointer(&missing_object(), MISSING_SEED);
        self.write(&object_path(&oid), missing_object());
        say("done");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vigia::pictures::Kind;
    use vigia_core::{ChangeKind, Origin, Worktree};

    /// What the core lists for one changed path.
    #[derive(Debug)]
    struct Listed {
        kind: ChangeKind,
        binary: bool,
        before: bool,
        after: bool,
        added: u32,
    }

    /// A scene's directory, removed when its test passes and kept when it
    /// fails.
    struct Cleaned(PathBuf);

    impl Drop for Cleaned {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!("the scene is kept for a look: {}", self.0.display());
            } else {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    impl std::ops::Deref for Cleaned {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    fn cleaned() -> Cleaned {
        Cleaned(fresh_dir().expect("a scene directory"))
    }

    /// The unchanged LFS pointer's index entry is racily clean.
    fn assert_racy(dir: &Path) {
        let modified = |rela: &str| {
            std::fs::metadata(dir.join(rela))
                .and_then(|meta| meta.modified())
                .expect("read a timestamp")
        };
        assert_eq!(
            modified("lfs/unchanged.png"),
            modified(".git/index"),
            "the unchanged LFS pointer is not racily clean"
        );
    }

    /// The static scene, built in a fresh directory.
    fn built() -> Cleaned {
        let dir = cleaned();
        let scene = Scene::new(&dir);
        scene.baseline();
        scene.changes();
        dir
    }

    /// A path keyed by the run it is in.
    fn keyed(origin: Origin, path: &str) -> String {
        match origin {
            Origin::Staged => format!("staged:{path}"),
            Origin::Unstaged => path.to_owned(),
        }
    }

    /// Both runs of the repository at `dir`, as the core lists them.
    fn listed(dir: &Path) -> BTreeMap<String, Listed> {
        let worktree = Worktree::discover(dir).expect("the scene is a repository");
        let mut frame = worktree.frame();
        frame.show_staged(true);
        frame.advance().expect("advance");
        (0..frame.files().len())
            .map(|index| {
                let (change, diff) = frame.diff(index).expect("diff");
                let picture = diff.picture;
                (
                    keyed(change.origin, &change.path),
                    Listed {
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
        let found = listed(&dir);
        assert!(states().len() > 20, "the scene lists almost no states");
        for (path, _) in states() {
            assert!(
                found.contains_key(&path),
                "the printed list names {path}, which the scene did not change"
            );
        }
        let at = |path: &str| {
            found
                .get(path)
                .unwrap_or_else(|| panic!("{path} is not listed: {:?}", found.keys()))
        };
        assert_eq!(at("src/added.rs").kind, ChangeKind::Added);
        assert_eq!(at("src/deleted.rs").kind, ChangeKind::Removed);
        let missing = at("lfs/missing.png");
        assert!(
            missing.binary && !missing.before,
            "a pointer whose object is missing drew its old side: {missing:?}"
        );
        for (path, count) in [
            ("src/small.rs", 1),
            ("src/medium.rs", 8),
            ("src/large.rs", 40),
        ] {
            assert_eq!(at(path).added, count, "{path} has the wrong hunk");
        }
        for raster in &RASTERS {
            let sides = |name: &str| {
                let listed = at(&raster.file(name));
                (listed.before, listed.after)
            };
            assert_eq!(sides("added"), (false, true), "{}", raster.file("added"));
            assert_eq!(
                sides("deleted"),
                (true, false),
                "{}",
                raster.file("deleted")
            );
            assert_eq!(
                sides("modified"),
                (true, true),
                "{}",
                raster.file("modified")
            );
            assert_eq!(sides("resized"), (true, true), "{}", raster.file("resized"));
            assert_eq!(
                at(&format!("staged:{}", raster.file("after"))).kind,
                ChangeKind::Renamed {
                    from: raster.file("before")
                },
                "{} is not a staged rename",
                raster.file("after")
            );
            assert_eq!(sides("after"), (true, true), "{}", raster.file("after"));
        }
        assert_eq!(
            at("png/moved.png").kind,
            ChangeKind::Renamed {
                from: "png/original.png".to_owned()
            },
            "png/moved.png is not an unstaged rename"
        );
        let moved = at("png/moved.png");
        assert!(
            moved.before && moved.after,
            "png/moved.png has no size line: {moved:?}"
        );
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
            let listed = at(path);
            assert!(
                listed.before && listed.after,
                "{path} has no size line: {listed:?}"
            );
        }
        for path in [
            "pdf/modified.pdf",
            "pdf/page-three.pdf",
            "pdf/pages.pdf",
            "pdf/fewer.pdf",
            "pdf/heavy.pdf",
            "pdf/large.pdf",
        ] {
            let listed = at(path);
            assert!(
                listed.before && listed.after,
                "{path} has no size line: {listed:?}"
            );
        }
        let sides = |path: &str| (at(path).before, at(path).after);
        assert_eq!(sides("pdf/added.pdf"), (false, true));
        assert_eq!(sides("pdf/deleted.pdf"), (true, false));
        for path in [
            "broken/truncated-header.png",
            "bin/blob.dat",
            "pdf/cut.pdf",
            "pdf/locked.pdf",
        ] {
            let listed = at(path);
            assert!(
                listed.binary && !listed.before && !listed.after,
                "{path} is not plain binary: {listed:?}"
            );
        }
    }

    /// What the pane drew for one file, over every view that showed it.
    #[derive(Debug, Default)]
    struct Drawn {
        /// Its size line, the panels it was given, and what the built
        /// picture said after the line.
        sized: Option<(String, Kind, Option<String>)>,
        picture_rows: usize,
        reasons: Vec<String>,
        hunks: usize,
        /// The path a rename's heading names it from.
        from: Option<String>,
    }

    /// The static scene drawn by `vigia`'s own view at `width` by `height`,
    /// with the staged run beside the unstaged one when `staged`, one view
    /// per file and every picture built while it is on screen. Each view is
    /// also painted into a buffer.
    fn draw_scene(dir: &Path, width: u16, height: u16, staged: bool) -> BTreeMap<String, Drawn> {
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
        if staged {
            app.apply(Action::ToggleStaged, &mut frame, usize::from(height))
                .expect("show the staged run");
            assert!(app.staged(), "the staged run is not shown");
        }
        let mut found: BTreeMap<String, Drawn> = BTreeMap::new();
        for _ in 0..frame.files().len() {
            let chrome = chrome(&app);
            let body = body_layout(
                area,
                &chrome,
                frame.files().len(),
                vigia::list_rows_wanted(frame.files()),
            );
            // Queued, built, and drawn again; as the shell does, a size the
            // worker found is given to the frame, and drawn once more.
            let view = loop {
                let mut drawn = app
                    .view(&mut frame, &mut highlighter, &history, body)
                    .expect("view");
                pictures.attach(&mut drawn.rows, &frame);
                assert!(
                    pictures.settle(Duration::from_secs(60)),
                    "the pictures were not built"
                );
                let mut drawn = app
                    .view(&mut frame, &mut highlighter, &history, body)
                    .expect("view");
                pictures.attach(&mut drawn.rows, &frame);
                let sizes = pictures.found_sizes();
                if sizes.is_empty() {
                    break drawn;
                }
                for (stamp, width, height) in sizes {
                    frame.learn_size(stamp, width, height);
                }
            };
            render(
                &mut Buffer::empty(area),
                area,
                &view,
                &Theme::dark(),
                Glyphs::default(),
                &chrome,
            );
            let key_of = |file: usize| {
                let change = &frame.files()[file];
                keyed(change.origin, &change.path)
            };
            let mut heading: Option<String> = None;
            for row in &view.rows {
                match row {
                    Row::File(entry) => {
                        let key = keyed(entry.origin, &entry.path);
                        found.entry(key.clone()).or_default().from = entry.from.clone();
                        heading = Some(key);
                    }
                    Row::Sized {
                        line,
                        file,
                        key,
                        ready,
                        ..
                    } => {
                        let suffix = ready.as_ref().and_then(|ready| ready.0.suffix());
                        found.entry(key_of(*file)).or_default().sized =
                            Some((line.clone(), key.shape.kind, suffix));
                    }
                    Row::Picture { file, .. } => {
                        found.entry(key_of(*file)).or_default().picture_rows += 1
                    }
                    Row::Reason(reason) => {
                        if let Some(key) = &heading {
                            found
                                .entry(key.clone())
                                .or_default()
                                .reasons
                                .push(reason.clone());
                        }
                    }
                    Row::Hunk { .. } => {
                        if let Some(key) = &heading {
                            found.entry(key.clone()).or_default().hunks += 1;
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
        assert_racy(&dir);
        for (width, height) in [(120, 40), (80, 24)] {
            let found = draw_scene(&dir, width, height, false);
            let at = |path: &str| {
                found.get(path).unwrap_or_else(|| {
                    panic!(
                        "{path} was never drawn at {width}x{height}: {:?}",
                        found.keys()
                    )
                })
            };
            let sized = |path: &str| {
                at(path).sized.clone().unwrap_or_else(|| {
                    panic!(
                        "{path} drew no size line at {width}x{height}: {:?}",
                        at(path)
                    )
                })
            };
            let said = |path: &str| sized(path).2.unwrap_or_default();
            for raster in &RASTERS {
                for (name, panels) in [
                    ("added", &[Kind::Added][..]),
                    ("deleted", &[Kind::Deleted][..]),
                    ("modified", &[Kind::Three, Kind::Mask][..]),
                    ("resized", &[Kind::Pair, Kind::New][..]),
                    ("after", &[Kind::Three, Kind::Mask][..]),
                ] {
                    let path = raster.file(name);
                    let kind = sized(&path).1;
                    assert!(
                        panels.contains(&kind),
                        "{path} drew {kind:?} at {width}x{height}"
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
                !found.contains_key("lfs/unchanged.png"),
                "an unchanged LFS pointer the walk read again was listed at {width}x{height}"
            );
            // Old and new both drawn, so the old side came from the object.
            assert!(
                matches!(sized("lfs/pointer.png").1, Kind::Three | Kind::Mask),
                "the LFS pointer did not draw its object as the old side: {:?}",
                at("lfs/pointer.png")
            );
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
            for path in [
                "broken/truncated-header.png",
                "bin/blob.dat",
                "pdf/cut.pdf",
                "pdf/locked.pdf",
            ] {
                // A file near the end can be in two views.
                let reasons = &at(path).reasons;
                assert!(
                    !reasons.is_empty() && reasons.iter().all(|reason| reason == "binary"),
                    "{path} at {width}x{height}: {reasons:?}"
                );
            }
            for path in [
                "pdf/modified.pdf",
                "pdf/page-three.pdf",
                "pdf/pages.pdf",
                "pdf/fewer.pdf",
                "pdf/large.pdf",
            ] {
                assert!(
                    at(path).picture_rows > 0,
                    "{path} drew no picture at {width}x{height}"
                );
            }
            assert!(
                said("pdf/page-three.pdf").starts_with("page 3 of 12 · "),
                "{:?}",
                at("pdf/page-three.pdf")
            );
            assert!(
                said("pdf/pages.pdf").starts_with("page 13 of 13 added · 12 → 13 pages"),
                "{:?}",
                at("pdf/pages.pdf")
            );
            assert!(
                said("pdf/fewer.pdf").starts_with("page 13 of 13 removed · 13 → 12 pages"),
                "{:?}",
                at("pdf/fewer.pdf")
            );
            assert_eq!(
                said("pdf/heavy.pdf"),
                "could not be decoded",
                "at {width}x{height}"
            );
            assert!(
                sized("pdf/large.pdf").0.starts_with("PDF 24×24 pt"),
                "the worker's size never reached the line: {:?}",
                at("pdf/large.pdf")
            );
            for path in [
                "src/added.rs",
                "src/deleted.rs",
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
        // The unstaged run draws a binary moved and changed as a rename too.
        let unstaged = draw_scene(&dir, 120, 40, false);
        let moved = unstaged
            .get("png/moved.png")
            .unwrap_or_else(|| panic!("png/moved.png was never drawn: {:?}", unstaged.keys()));
        assert_eq!(
            moved.from.as_deref(),
            Some("png/original.png"),
            "png/moved.png is not drawn as a rename"
        );
        assert!(moved.picture_rows > 0, "png/moved.png drew no picture");
        // The staged run draws each rename as one, with its pictures.
        let found = draw_scene(&dir, 120, 40, true);
        for raster in &RASTERS {
            let key = format!("staged:{}", raster.file("after"));
            let drawn = found
                .get(&key)
                .unwrap_or_else(|| panic!("{key} was never drawn: {:?}", found.keys()));
            assert_eq!(
                drawn.from.as_deref(),
                Some(raster.file("before").as_str()),
                "{key} is not drawn as a rename"
            );
            assert!(drawn.picture_rows > 0, "{key} drew no picture");
        }
    }

    #[test]
    fn scene_replay_ends() {
        let dir = cleaned();
        let scene = Scene::new(&dir);
        scene.baseline();
        assert_racy(&dir);
        let mut said = Vec::new();
        scene.replay(0.0, |line| said.push(line.to_owned()));
        assert_eq!(said.last().map(String::as_str), Some("done"), "{said:?}");
        let found = listed(&dir);
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
        let modified = at("png/modified.png");
        assert!(
            modified.before && modified.after,
            "the rewrite did not draw: {modified:?}"
        );
        assert_eq!(at("svg/new.svg").kind, ChangeKind::Added);
        assert_eq!(
            at("staged:png/after.png").kind,
            ChangeKind::Renamed {
                from: "png/before.png".to_owned()
            }
        );
        assert_eq!(at("png/after.png").kind, ChangeKind::Modified);
        assert_eq!(at("jpeg/deleted.jpg").kind, ChangeKind::Removed);
        assert_eq!(at("src/deleted.rs").kind, ChangeKind::Removed);
        let flaky = at("broken/flaky.png");
        assert!(flaky.after, "the fixed image does not size: {flaky:?}");
        let arrived = at("lfs/missing.png");
        assert!(
            arrived.before,
            "the landed object does not size: {arrived:?}"
        );
        assert_eq!(at("gif/animated.gif").kind, ChangeKind::Modified);
    }

    #[test]
    fn scene_temp_only() {
        let dir = cleaned();
        let scene = Scene::new(&dir);
        // A real file beside the scene, which no escape may write or remove.
        let name = format!(
            "{}-outside.txt",
            dir.file_name().expect("a name").to_string_lossy()
        );
        let outside = dir.parent().expect("under the temp dir").join(&name);
        std::fs::write(&outside, "kept").expect("write beside the scene");
        let absolute = outside.to_string_lossy().into_owned();
        for escape in [format!("../{name}"), format!("png/../../{name}"), absolute] {
            for (verb, touch) in [
                ("written", &(|| scene.write(&escape, "x")) as &dyn Fn()),
                ("removed", &|| scene.remove(&escape)),
            ] {
                let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(touch));
                assert!(refused.is_err(), "{escape} was {verb}");
                assert_eq!(
                    std::fs::read_to_string(&outside).ok().as_deref(),
                    Some("kept"),
                    "{escape} was {verb}"
                );
            }
        }
        let _ = std::fs::remove_file(&outside);
    }
}
