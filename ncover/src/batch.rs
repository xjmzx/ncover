use super::*;

// ---------- batch ----------
//
// Templating one cover is a demo; templating a discography is the workflow. The
// recipe is what you already dialled in on the canvas — size, framing, disc,
// corner fill — applied across a folder.
//
// What CANNOT be batched is the drag: a hand-placed dx/dy means nothing on the
// next image, which has different dimensions. So batch offers the two framings
// that are defined for any source — Cover and Fit — and says so, rather than
// pretending your placement generalises.

/// What we will open. Anything else in the tree is simply not an image.
pub(crate) const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "svg"];

/// How many result rows the list will show. Beyond this the run is still
/// complete and still counted — the LIST is what is capped, and it says so.
/// A silent truncation reads as "that was everything".
pub(crate) const RESULT_ROWS: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framing {
    Cover,
    Fit,
}

#[derive(Clone, Copy)]
pub(crate) struct Recipe {
    pub(crate) canvas: i32,
    pub(crate) framing: Framing,
    /// `None` = no disc: just the framed square. The disc is an option, not the
    /// point — resizing a folder of covers is a job on its own.
    pub(crate) disc: Option<OuterFill>,
}

/// Refuse a source/output pairing that would eat itself.
///
/// This is ntree's `guard_deletable` lesson in its non-destructive form: the
/// output folder is a user-typed path, and the originals are the irreplaceable
/// thing. Batch never writes into the source tree — not as an option, not with a
/// confirmation. It also refuses output nested INSIDE the source, which would
/// not overwrite anything but would make the next run batch its own output.
pub(crate) fn guard_batch(src: &Path, out: &Path) -> Result<()> {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (s, o) = (real(src), real(out));
    if o == s {
        anyhow::bail!("The output folder is the source folder. Batch never writes over your originals.");
    }
    if o.starts_with(&s) {
        anyhow::bail!(
            "The output folder is inside the source folder.\n\nNothing would be overwritten, but the next run would pick up its own output as input."
        );
    }
    if s.starts_with(&o) {
        anyhow::bail!("The source folder is inside the output folder. Choose an output folder beside it, not above it.");
    }
    Ok(())
}

/// Walk `root` for images whose filename contains `needle` (case-insensitively;
/// empty matches everything). Hidden directories are skipped — nothing in a
/// `.git` is artwork.
pub(crate) fn find_images(root: &Path, needle: &str, into: &mut Vec<PathBuf>) -> Result<()> {
    let needle = needle.to_lowercase();
    let rd = fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))?;
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort(); // a stable order, so a dry run and the real run agree
    for path in entries {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            // A folder we cannot read is not fatal to the whole run.
            let _ = find_images(&path, &needle, into);
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()));
        if ext_ok && (needle.is_empty() || name.to_lowercase().contains(&needle)) {
            into.push(path);
        }
    }
    Ok(())
}

// ---------- published-discography scope ----------
//
// A folder is the obvious source, but the one that makes this part of the suite
// rather than a standalone utility is ndisc's manifest: the exact set of releases
// published to Nostr, in the suite's own scope language. xcolor reading it costs
// nothing and turns "disc-label every published release" into one choice.

/// ndisc's export: the releases it has published, each with its on-disk dir.
/// Only the fields batch needs — serde ignores the rest (id, artist, title…).
#[derive(serde::Deserialize)]
pub(crate) struct Manifest {
    #[serde(rename = "libraryRoot")]
    pub(crate) library_root: PathBuf,
    pub(crate) releases: Vec<ManifestRelease>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ManifestRelease {
    pub(crate) dir: PathBuf,
}

/// Where ndisc writes it. `None` if it has never exported one — in which case
/// the published scope is not offered, because a scope that matches nothing is
/// worse than no scope.
pub(crate) fn published_manifest_path() -> Option<PathBuf> {
    let p = dirs::data_dir()?.join("ndisc-suite").join("published.json");
    p.exists().then_some(p)
}

pub(crate) fn load_manifest(path: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let m: Manifest = serde_json::from_str(&text).context("published.json is not the shape we expect")?;
    Ok(m)
}

/// Where a batch run gets its images, and what to strip to mirror the tree.
///
/// The distinction the two scopes share is exactly this: a set of roots to walk,
/// and the prefix that turns an absolute hit into the relpath the output mirrors.
/// For a folder both are the same path; for the manifest the roots are the
/// release dirs and the prefix is the library root — so `artist/release/cover.png`
/// comes out the same shape either way.
pub(crate) struct Scope {
    /// Every directory to search. One for a folder; N release dirs for published.
    pub(crate) roots: Vec<PathBuf>,
    /// Stripped off each hit to form the mirrored relpath.
    pub(crate) strip: PathBuf,
    /// The source boundary the guard protects. The originals live under here.
    pub(crate) guard_root: PathBuf,
}

impl Scope {
    pub(crate) fn folder(root: PathBuf) -> Scope {
        Scope {
            roots: vec![root.clone()],
            strip: root.clone(),
            guard_root: root,
        }
    }

    /// The published releases become the walk roots; the library root is both the
    /// mirror prefix and the boundary the guard protects — the originals ARE the
    /// library, and every release dir lives under it.
    pub(crate) fn published(m: &Manifest) -> Scope {
        Scope {
            roots: m.releases.iter().map(|r| r.dir.clone()).collect(),
            strip: m.library_root.clone(),
            guard_root: m.library_root.clone(),
        }
    }
}

/// Is the current source a PNG? Only then may Overwrite write the result back
/// over it — we save PNG, so overwriting anything else would silently change the
/// format (and, for a JPEG, quietly re-encode as PNG under the same name).
pub(crate) fn src_is_png(path: &Option<PathBuf>) -> bool {
    path.as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

/// Load one image path into the pieces the view needs: the pixbuf, a display
/// name, and — for an SVG — its declared colours/fonts/structure. The single
/// place a file becomes something on the canvas, shared by Open and the browser
/// so they can never drift apart.
pub(crate) fn load_image_file(path: &Path) -> Result<(gdk_pixbuf::Pixbuf, String, Option<SvgInfo>)> {
    // SVG has no intrinsic pixel size worth trusting, so rasterise it big enough
    // to interrogate. PNG/JPEG/WebP load at their own size.
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let pb = if is_svg {
        gdk_pixbuf::Pixbuf::from_file_at_scale(path, 1024, 1024, true)
    } else {
        gdk_pixbuf::Pixbuf::from_file(path)
    }
    .with_context(|| format!("could not open {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    // An SVG is read as well as rendered: the raster is for looking at, the parse
    // is for knowing.
    let info = if is_svg {
        fs::read_to_string(path).ok().and_then(|t| inspect_svg(&t).ok())
    } else {
        None
    };
    Ok((pb, name, info))
}

/// Output path: the source tree's shape, mirrored under `out_root`. Always PNG —
/// an alpha corner fill has nowhere to live in a JPEG.
pub(crate) fn batch_dest(rel: &Path, out_root: &Path, disc: bool) -> PathBuf {
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    let suffix = if disc { "-disc" } else { "" };
    let mut p = out_root.to_path_buf();
    if let Some(parent) = rel.parent() {
        p.push(parent);
    }
    p.push(format!("{stem}{suffix}.png"));
    p
}

/// Render one source through the recipe to the final pixbuf — read, frame, disc.
/// The ONE place the batch output is produced, so what a Preview shows and what a
/// Run writes are byte-identical by construction, not by two code paths agreeing.
pub(crate) fn batch_render(src: &Path, r: Recipe) -> Result<gdk_pixbuf::Pixbuf> {
    let is_svg = src
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let pb = if is_svg {
        // An SVG has no pixel size worth trusting; rasterise it comfortably
        // above any canvas we offer so scaling down is the only thing we do.
        gdk_pixbuf::Pixbuf::from_file_at_scale(src, 1024, 1024, true)
    } else {
        gdk_pixbuf::Pixbuf::from_file(src)
    }
    .with_context(|| "could not read the image".to_string())?;

    let (w, h) = (pb.width(), pb.height());
    if w == 0 || h == 0 {
        anyhow::bail!("empty image");
    }
    let place = match r.framing {
        Framing::Cover => Placement::cover(w, h, r.canvas),
        Framing::Fit => Placement::fit(w, h, r.canvas),
    };
    let framed = compose(&pb, place, r.canvas)?;
    match r.disc {
        Some(fill) => disc_template(&framed, fill),
        None => Ok(framed),
    }
}

/// One file, start to finish. `dry` renders (so it still fails on a file that
/// would really fail) but does everything except write.
pub(crate) fn batch_one(src: &Path, rel: &Path, out_root: &Path, r: Recipe, dry: bool) -> Result<PathBuf> {
    let final_pb = batch_render(src, r)?;
    let dest = batch_dest(rel, out_root, r.disc.is_some());
    if dry {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    final_pb
        .savev(&dest, "png", &[])
        .with_context(|| format!("could not write {}", dest.display()))?;
    Ok(dest)
}

/// Progress from the worker thread. Deliberately carries the REASON on failure:
/// "31 failed" is unactionable and sends you guessing at the tool from the
/// outside — the single most valuable thing ntree's sampler learned.
pub(crate) enum BatchMsg {
    Total(usize),
    One {
        rel: String,
        result: std::result::Result<PathBuf, String>,
    },
    Done {
        ok: usize,
        failed: usize,
        stopped: bool,
    },
}

/// How many outputs a Preview renders. A contact sheet is a SAMPLE you eyeball to
/// catch "my recipe is wrong" before it touches 500 files — not the whole run.
pub(crate) const PREVIEW_N: usize = 12;

/// Thumbnail side, px. Big enough to see a bad crop or a wrong corner fill.
pub(crate) const PREVIEW_PX: i32 = 132;

/// A rendered thumbnail on its way back from the worker. Pixbuf is `!Send`, so it
/// cannot cross the thread — the raw RGBA bytes (which ARE Send) do, and the main
/// thread rebuilds the pixbuf. Rendering itself is fine off-thread: it is pure
/// image data, never GTK, exactly as the real run already does.
pub(crate) enum PreviewMsg {
    Total(usize),
    Thumb {
        label: String,
        /// `(rgba, w, h, rowstride, has_alpha)` on success; `None` with a reason
        /// on failure — a preview must show the failures too, or it lies about
        /// what the run will do.
        thumb: Option<(Vec<u8>, i32, i32, i32, bool)>,
        err: Option<String>,
    },
    Done,
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xcolor-batch-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_output_folder_may_not_be_the_source_folder() {
        let d = tmpdir("same");
        assert!(guard_batch(&d, &d).is_err(), "that would overwrite originals");
    }

    #[test]
    fn the_output_folder_may_not_sit_inside_the_source() {
        // Nothing gets overwritten, but the NEXT run would take its own output
        // as input — the batch would compound on itself.
        let src = tmpdir("nested");
        let out = src.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(guard_batch(&src, &out).is_err());
    }

    #[test]
    fn the_source_may_not_sit_inside_the_output() {
        let out = tmpdir("parent");
        let src = out.join("art");
        fs::create_dir_all(&src).unwrap();
        assert!(guard_batch(&src, &out).is_err());
    }

    #[test]
    fn siblings_are_fine() {
        let base = tmpdir("siblings");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&out).unwrap();
        assert!(guard_batch(&src, &out).is_ok());
    }

    #[test]
    fn the_destination_mirrors_the_source_tree() {
        // The shape of the library is the only thing that tells 1,600 files named
        // cover.png apart. Flatten it and they all land on each other.
        let rel = Path::new("Autechre/Amber/cover.jpg");
        let d = batch_dest(rel, Path::new("/tmp/out"), true);
        assert_eq!(d, Path::new("/tmp/out/Autechre/Amber/cover-disc.png"));
        // No disc: no suffix. Always PNG — alpha has nowhere to live in a JPEG.
        let d = batch_dest(rel, Path::new("/tmp/out"), false);
        assert_eq!(d, Path::new("/tmp/out/Autechre/Amber/cover.png"));
    }

    #[test]
    fn discovery_recurses_filters_by_name_and_skips_hidden() {
        let root = tmpdir("find");
        let rel = root.join("Artist/Release");
        fs::create_dir_all(&rel).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        for f in ["cover.png", "back.png", "notes.txt"] {
            fs::write(rel.join(f), b"x").unwrap();
        }
        fs::write(root.join(".git/cover.png"), b"x").unwrap();

        let mut hits = Vec::new();
        find_images(&root, "cover", &mut hits).unwrap();
        assert_eq!(hits, vec![rel.join("cover.png")], "name filter + no dotdirs");

        let mut all = Vec::new();
        find_images(&root, "", &mut all).unwrap();
        assert_eq!(all.len(), 2, "empty filter = every image, but .txt is not one");
    }

    #[test]
    fn changing_the_output_size_keeps_the_framing() {
        // Framing is a decision. Doubling the canvas must not re-crop it — the
        // square grows, the picture stays where it was put.
        let a = Placement::cover(800, 400, 400);
        let b = a.rescaled(400, 800);
        let direct = Placement::cover(800, 400, 800);
        assert!((b.scale - direct.scale).abs() < 1e-9);
        assert!((b.dx - direct.dx).abs() < 1e-9);
        assert!((b.dy - direct.dy).abs() < 1e-9);
    }

    #[test]
    fn a_stored_size_we_do_not_offer_is_not_honoured() {
        // It would be a canvas with no way back to it in the picker.
        assert_eq!(sane_canvas(437), CANVAS_DEFAULT);
        assert_eq!(sane_canvas(1000), 1000);
        assert!(CANVAS_SIZES.contains(&CANVAS_DEFAULT));
    }

    #[test]
    #[ignore] // touches /data/music; run explicitly
    fn preview_render_of_a_real_cover_is_a_square_thumb() {
        let src = std::path::Path::new("/data/music/214/Fuel Cells/cover.jpg");
        let r = Recipe { canvas: 600, framing: Framing::Cover, disc: Some(OuterFill::Alpha) };
        let full = batch_render(src, r).unwrap();
        assert_eq!((full.width(), full.height()), (600, 600));
        let t = full.scale_simple(PREVIEW_PX, PREVIEW_PX, gdk_pixbuf::InterpType::Bilinear).unwrap();
        assert_eq!((t.width(), t.height()), (PREVIEW_PX, PREVIEW_PX));
        // Alpha corner present.
        assert!(t.has_alpha());
    }

    #[test]
    fn batch_writes_a_square_png_at_the_chosen_size() {
        let base = tmpdir("run");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(src.join("A/B")).unwrap();
        // A deliberately non-square source: the point is that it gets PLACED.
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 90, 60).unwrap();
        pb.fill(0x3366FFFF);
        pb.savev(src.join("A/B/cover.png"), "png", &[]).unwrap();

        let r = Recipe {
            canvas: 200,
            framing: Framing::Cover,
            disc: Some(OuterFill::Alpha),
        };
        let rel = Path::new("A/B/cover.png");
        let dest = batch_one(&src.join(rel), rel, &out, r, false).unwrap();
        assert_eq!(dest, out.join("A/B/cover-disc.png"));
        let got = gdk_pixbuf::Pixbuf::from_file(&dest).unwrap();
        assert_eq!((got.width(), got.height()), (200, 200));
        // Alpha corners: the disc is inscribed, so (0,0) is outside it.
        let b = unsafe { got.pixels() };
        assert_eq!(b[3], 0, "the corner must be transparent");
    }

    fn px(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let n = pb.n_channels() as usize;
        let b = unsafe { pb.pixels() };
        let i = y as usize * pb.rowstride() as usize + x as usize * n;
        (b[i], b[i + 1], b[i + 2], if pb.has_alpha() { b[i + 3] } else { 255 })
    }

    #[test]
    fn a_blank_canvas_is_opaque_white() {
        let pb = blank_canvas(64).unwrap();
        assert_eq!((pb.width(), pb.height()), (64, 64));
        assert_eq!(px(&pb, 0, 0), (255, 255, 255, 255));
        assert_eq!(px(&pb, 63, 63), (255, 255, 255, 255));
    }

    #[test]
    fn invert_flips_rgb_and_leaves_alpha_alone() {
        let src = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 4, 4).unwrap();
        // A semi-transparent teal: 10,200,150 @ alpha 80.
        {
            let n = src.n_channels() as usize;
            let b = unsafe { src.pixels() };
            for i in (0..b.len()).step_by(n) {
                b[i] = 10;
                b[i + 1] = 200;
                b[i + 2] = 150;
                b[i + 3] = 80;
            }
        }
        let inv = invert_rgb(&src).unwrap();
        assert_eq!(
            px(&inv, 1, 1),
            (245, 55, 105, 80),
            "RGB inverted (255-x); alpha untouched — a colour tool inverts colour, not shape"
        );
        // Twice is identity.
        let back = invert_rgb(&inv).unwrap();
        assert_eq!(px(&back, 2, 2), (10, 200, 150, 80));
    }

    #[test]
    fn a_square_is_centred_black_and_opaque() {
        // 100px canvas, 40% square = 40px, centred → spans [30,70).
        let base = blank_canvas(100).unwrap();
        let out = stamp_square(&base, 0.40, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&out, 50, 50), (0, 0, 0, 255), "centre is inside the square");
        assert_eq!(px(&out, 31, 31), (0, 0, 0, 255), "just inside the top-left corner");
        assert_eq!(px(&out, 28, 28), (255, 255, 255, 255), "just outside stays white");
        assert_eq!(px(&out, 0, 0), (255, 255, 255, 255), "the ground is untouched");
    }

    #[test]
    fn squares_stack_and_a_zero_square_is_a_no_op() {
        // Build: a big square, then a smaller one on top — both black here, but
        // the point is the second reads the first's result, not the ground.
        let base = blank_canvas(100).unwrap();
        let big = stamp_square(&base, 0.80, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        let small = stamp_square(&big, 0.20, Rgb { r: 255, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&small, 50, 50), (255, 0, 0, 255), "the small square is on top");
        assert_eq!(px(&small, 15, 15), (0, 0, 0, 255), "the big one still shows around it");
        // A zero-size square changes nothing.
        let noop = stamp_square(&base, 0.0, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&noop, 50, 50), (255, 255, 255, 255));
    }

    #[test]
    fn the_manifest_becomes_release_dirs_stripped_to_the_library_root() {
        // The point of published scope: cover.png in each release mirrors to
        // artist/release/ under the output, the same shape a folder walk gives —
        // because the strip prefix is the library root, not each release dir.
        let m: Manifest = serde_json::from_str(
            r#"{
                "libraryRoot": "/data/music",
                "releases": [
                    {"id": 1, "artist": "214", "title": "Fuel Cells", "dir": "/data/music/214/Fuel Cells"},
                    {"id": 4, "artist": "2562", "title": "Aerial", "dir": "/data/music/2562/Aerial"}
                ]
            }"#,
        )
        .unwrap();
        let s = Scope::published(&m);
        assert_eq!(s.roots.len(), 2);
        assert_eq!(s.strip, Path::new("/data/music"));
        assert_eq!(s.guard_root, Path::new("/data/music"));
        // A hit under a release dir strips to artist/release/file.
        let hit = Path::new("/data/music/214/Fuel Cells/cover.png");
        let rel = hit.strip_prefix(&s.strip).unwrap();
        assert_eq!(
            batch_dest(rel, Path::new("/tmp/out"), true),
            Path::new("/tmp/out/214/Fuel Cells/cover-disc.png")
        );
    }

    #[test]
    fn the_guard_protects_the_whole_library_under_published_scope() {
        // Output inside the library would be caught, because guard_root IS the
        // library root — the originals are the entire tree, not one release.
        let m: Manifest = serde_json::from_str(
            r#"{"libraryRoot": "/data/music", "releases": []}"#,
        )
        .unwrap();
        let s = Scope::published(&m);
        assert!(guard_batch(&s.guard_root, Path::new("/data/music/_discs")).is_err());
        assert!(guard_batch(&s.guard_root, Path::new("/data/discs")).is_ok());
    }

    #[test]
    fn a_dry_run_writes_nothing_but_still_opens_every_file() {
        let base = tmpdir("dry");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 40, 40).unwrap();
        pb.savev(src.join("cover.png"), "png", &[]).unwrap();
        let r = Recipe { canvas: 400, framing: Framing::Fit, disc: None };
        let rel = Path::new("cover.png");

        let dest = batch_one(&src.join(rel), rel, &out, r, true).unwrap();
        assert_eq!(dest, out.join("cover.png"));
        assert!(!dest.exists(), "a dry run writes nothing");

        // And a file that would really fail, fails in the dry run too — that is
        // what makes it worth running.
        fs::write(src.join("broken-cover.png"), b"not a png").unwrap();
        let bad = Path::new("broken-cover.png");
        assert!(batch_one(&src.join(bad), bad, &out, r, true).is_err());
    }
}
