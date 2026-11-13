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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Framing {
    Cover,
    Fit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
/// Only the fields batch needs — serde ignores the rest (id, …).
#[derive(serde::Deserialize)]
pub(crate) struct Manifest {
    #[serde(rename = "libraryRoot")]
    pub(crate) library_root: PathBuf,
    pub(crate) releases: Vec<ManifestRelease>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ManifestRelease {
    pub(crate) dir: PathBuf,
    /// Kept so the survey can name a release as a record ("Artist — Title")
    /// rather than as a path. Optional: an older manifest without them still
    /// loads, and the release is named by its folder instead.
    #[serde(default)]
    pub(crate) artist: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
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
    /// Display names for walk roots, where something better than the path is
    /// known: "Artist — Title" from the manifest. Empty for a folder.
    pub(crate) names: HashMap<PathBuf, String>,
}

impl Scope {
    pub(crate) fn folder(root: PathBuf) -> Scope {
        Scope {
            roots: vec![root.clone()],
            strip: root.clone(),
            guard_root: root,
            names: HashMap::new(),
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
            names: m
                .releases
                .iter()
                .filter_map(|r| match (r.artist.as_deref(), r.title.as_deref()) {
                    (Some(a), Some(t)) if !a.is_empty() && !t.is_empty() => {
                        Some((r.dir.clone(), format!("{a} \u{2014} {t}")))
                    }
                    _ => None,
                })
                .collect(),
        }
    }

    /// What to call a walk root in the survey: the manifest's name, else its
    /// path under the strip prefix, else (a folder scope, root == strip) its
    /// own folder name.
    pub(crate) fn name_of(&self, root: &Path) -> String {
        if let Some(n) = self.names.get(root) {
            return n.clone();
        }
        match root.strip_prefix(&self.strip) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
            _ => root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
        }
    }
}

// ---------- what a run would touch ----------
//
// The survey and the run must select EXACTLY the same files, or the count on
// the Run button is a lie. So there is one walk, `walk_scope`, and both read
// it: `find_in_scope` flattens it for a run or a preview, `survey_batch`
// groups it for display. They cannot de-duplicate differently because neither
// de-duplicates at all.

/// Walk roots that could not be read, each with the reason.
pub(crate) type Unreadable = Vec<(PathBuf, String)>;

/// Every matching image, tagged with the index of the root that found it. A
/// file reachable from two roots (nested release dirs) belongs to the FIRST
/// root that found it and appears once. Unreadable roots come back as errors
/// alongside, so a run can report them and a survey can count them.
pub(crate) fn walk_scope(scope: &Scope, needle: &str) -> (Vec<(usize, PathBuf)>, Unreadable) {
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    let mut errors = Vec::new();
    for (i, root) in scope.roots.iter().enumerate() {
        let mut found = Vec::new();
        if let Err(e) = find_images(root, needle, &mut found) {
            errors.push((root.clone(), format!("{e:#}")));
        }
        for f in found {
            if seen.insert(f.clone()) {
                hits.push((i, f));
            }
        }
    }
    (hits, errors)
}

/// The files a run processes, in the stable order a dry run and a real run
/// share.
pub(crate) fn find_in_scope(scope: &Scope, needle: &str) -> (Vec<PathBuf>, Unreadable) {
    let (hits, errors) = walk_scope(scope, needle);
    let mut files: Vec<PathBuf> = hits.into_iter().map(|(_, f)| f).collect();
    files.sort();
    (files, errors)
}

/// One walk root that matched something.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SurveyGroup {
    pub(crate) name: String,
    /// Paths relative to the scope's strip prefix, sorted.
    pub(crate) files: Vec<String>,
}

/// What a scope and a filter select, before anything is decoded. Cheap enough
/// to keep current while the filter is typed: a filter whose effect you cannot
/// see is a guess.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Survey {
    /// Sorted by name.
    pub(crate) groups: Vec<SurveyGroup>,
    pub(crate) total: usize,
    /// Roots that matched nothing. Counted, not listed: "9 releases have no
    /// cover" is the useful form.
    pub(crate) empty_roots: usize,
    /// Roots that could not be read at all.
    pub(crate) unreadable: usize,
    /// Output names two sources would share (`cover.jpg` + `cover.png`).
    /// The run keeps them apart by extension; the survey says so up front.
    pub(crate) shared_names: usize,
    /// The first files in RUN order, for the automatic sample — so the sample
    /// is literally what a run starts with.
    pub(crate) first: Vec<PathBuf>,
}

pub(crate) fn survey_batch(scope: &Scope, needle: &str, first_n: usize) -> Survey {
    let (hits, errors) = walk_scope(scope, needle);
    let mut by_root: Vec<Vec<String>> = vec![Vec::new(); scope.roots.len()];
    let mut all: Vec<&PathBuf> = Vec::with_capacity(hits.len());
    let rels: Vec<PathBuf> = hits
        .iter()
        .map(|(_, f)| f.strip_prefix(&scope.strip).unwrap_or(f).to_path_buf())
        .collect();
    for (i, f) in &hits {
        by_root[*i].push(f.strip_prefix(&scope.strip).unwrap_or(f).display().to_string());
        all.push(f);
    }
    all.sort();
    let unreadable = errors.len();
    let bad: std::collections::HashSet<&PathBuf> = errors.iter().map(|(r, _)| r).collect();
    let mut groups = Vec::new();
    let mut empty_roots = 0;
    for (i, mut files) in by_root.into_iter().enumerate() {
        let root = &scope.roots[i];
        if files.is_empty() {
            if !bad.contains(root) {
                empty_roots += 1;
            }
            continue;
        }
        files.sort();
        groups.push(SurveyGroup { name: scope.name_of(root), files });
    }
    groups.sort_by_key(|g| g.name.to_lowercase());
    Survey {
        total: hits.len(),
        groups,
        empty_roots,
        unreadable,
        shared_names: shared_names(&rels),
        first: all.into_iter().take(first_n).cloned().collect(),
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

/// Every output path for a run, relative to the output root, in the order of
/// `rels`.
///
/// `batch_dest` is the plain answer for one file, and it is not enough for a
/// set: every output is `.png`, so `cover.jpg` and `cover.png` in one release
/// both become `cover.png`, and the second write silently replaces the
/// first. The dry run cannot catch that by rendering, since each file renders
/// fine on its own. So destinations are planned for the WHOLE set, and
/// sources that would share a name keep their extension in it:
/// `cover-jpg.png` and `cover-png.png`. Files that do not clash keep the
/// plain name, so a library with one cover per release comes out exactly as
/// before.
///
/// An `Err` is a file that would still overwrite an earlier one after that
/// (only possible if the library already holds a file literally named like
/// `cover-jpg.png` beside the pair). It is refused with the reason rather
/// than written over.
pub(crate) fn plan_dests(rels: &[PathBuf], disc: bool) -> Vec<std::result::Result<PathBuf, String>> {
    let suffix = if disc { "-disc" } else { "" };
    let plain: Vec<PathBuf> = rels.iter().map(|r| batch_dest(r, Path::new(""), disc)).collect();
    let mut uses: HashMap<&PathBuf, usize> = HashMap::new();
    for p in &plain {
        *uses.entry(p).or_default() += 1;
    }
    let mut taken: HashMap<PathBuf, &PathBuf> = HashMap::new();
    rels.iter()
        .zip(&plain)
        .map(|(rel, p)| {
            let dest = if uses[p] > 1 {
                let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
                let ext = rel
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                p.with_file_name(format!("{stem}-{ext}{suffix}.png"))
            } else {
                p.clone()
            };
            match taken.get(&dest) {
                Some(first) => Err(format!(
                    "would overwrite the output of {} ({}); not written",
                    first.display(),
                    dest.display()
                )),
                None => {
                    taken.insert(dest.clone(), rel);
                    Ok(dest)
                }
            }
        })
        .collect()
}

/// How many plain output names are shared by more than one source: the
/// releases (or folders) `plan_dests` will keep apart by extension.
pub(crate) fn shared_names(rels: &[PathBuf]) -> usize {
    let mut uses: HashMap<PathBuf, usize> = HashMap::new();
    for r in rels {
        *uses.entry(batch_dest(r, Path::new(""), false)).or_default() += 1;
    }
    uses.values().filter(|&&n| n > 1).count()
}

/// One file, start to finish, to a destination `plan_dests` chose. `dry`
/// renders (so it still fails on a file that would really fail) but does
/// everything except write.
pub(crate) fn batch_one(src: &Path, dest: &Path, r: Recipe, dry: bool) -> Result<PathBuf> {
    let final_pb = batch_render(src, r)?;
    let dest = dest.to_path_buf();
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

/// How many outputs render themselves, unasked, whenever the scope or the
/// recipe settles. Four, not twelve, because it re-runs on every change — and
/// it exists because a preview you must press is one you can forget to press,
/// which is how a folder of covers comes out as discs.
pub(crate) const SAMPLE_N: usize = 4;

/// Sample thumbnail side, px.
pub(crate) const SAMPLE_PX: i32 = 88;

/// A thumbnail's raw parts: `(rgba, w, h, rowstride, has_alpha)`. Pixbuf is
/// `!Send`; these bytes are, so this is what crosses back from a worker.
pub(crate) type ThumbBytes = (Vec<u8>, i32, i32, i32, bool);

/// Render one file through the recipe and shrink it to a `px` thumbnail —
/// through `batch_render`, the same function a run writes from.
pub(crate) fn render_thumb(src: &Path, r: Recipe, px: i32) -> Result<ThumbBytes> {
    let t = batch_render(src, r)?
        .scale_simple(px, px, gdk_pixbuf::InterpType::Bilinear)
        .context("could not scale the thumbnail")?;
    Ok((unsafe { t.pixels() }.to_vec(), t.width(), t.height(), t.rowstride(), t.has_alpha()))
}

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
        thumb: Option<ThumbBytes>,
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
    fn two_covers_in_one_release_do_not_land_on_one_file() {
        // The real case: 14 releases in the published catalogue hold both a
        // cover.jpg and a cover.png (or .jpeg + .jpg). Every output is PNG, so
        // both became cover.png and the second write replaced the first.
        let rels: Vec<PathBuf> = [
            "2562/Aerial/cover.jpg",
            "2562/Aerial/cover.png",
            "Cylob/Formant Potaton/cover.jpeg",
            "Cylob/Formant Potaton/cover.jpg",
            "Autechre/Amber/cover.jpg",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let got: Vec<PathBuf> = plan_dests(&rels, false).into_iter().map(|d| d.unwrap()).collect();
        assert_eq!(
            got,
            [
                "2562/Aerial/cover-jpg.png",
                "2562/Aerial/cover-png.png",
                "Cylob/Formant Potaton/cover-jpeg.png",
                "Cylob/Formant Potaton/cover-jpg.png",
                // No clash: the plain name, exactly as before.
                "Autechre/Amber/cover.png",
            ]
            .map(PathBuf::from)
        );
        let disc: Vec<PathBuf> = plan_dests(&rels[..2], true).into_iter().map(|d| d.unwrap()).collect();
        assert_eq!(disc, ["2562/Aerial/cover-jpg-disc.png", "2562/Aerial/cover-png-disc.png"].map(PathBuf::from));
        assert_eq!(shared_names(&rels), 2);
    }

    #[test]
    fn a_name_that_still_clashes_is_refused_not_overwritten() {
        // cover.jpg + cover.png become cover-jpg.png / cover-png.png, and the
        // folder also holds a real cover-jpg.png. The third must not win.
        let rels: Vec<PathBuf> = ["A/cover.jpg", "A/cover.png", "A/cover-jpg.png"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let got = plan_dests(&rels, false);
        assert_eq!(got[0].as_deref().unwrap(), Path::new("A/cover-jpg.png"));
        assert_eq!(got[1].as_deref().unwrap(), Path::new("A/cover-png.png"));
        assert!(got[2].as_ref().unwrap_err().contains("would overwrite"), "{:?}", got[2]);
    }

    #[test]
    #[ignore] // reads the real published.json and /data/music; run explicitly
    fn the_real_catalogue_plans_one_output_per_cover() {
        let m = load_manifest(&published_manifest_path().expect("no manifest")).unwrap();
        let scope = Scope::published(&m);
        let (files, _) = find_in_scope(&scope, "cover");
        let rels: Vec<PathBuf> = files
            .iter()
            .map(|f| f.strip_prefix(&scope.strip).unwrap().to_path_buf())
            .collect();
        let plan = plan_dests(&rels, false);
        let dests: std::collections::HashSet<_> = plan.iter().map(|d| d.clone().unwrap()).collect();
        assert_eq!(dests.len(), files.len(), "every cover gets its own output");
        println!("{} files, {} shared names", files.len(), shared_names(&rels));
        for (r, d) in rels.iter().zip(&plan) {
            let d = d.as_ref().unwrap();
            if d.file_name() != batch_dest(r, Path::new(""), false).file_name() {
                println!("{}  ->  {}", r.display(), d.file_name().unwrap().to_string_lossy());
            }
        }
    }

    #[test]
    fn the_survey_counts_shared_names() {
        let root = tmpdir("survey-shared");
        let d = root.join("A/B");
        fs::create_dir_all(&d).unwrap();
        for f in ["cover.jpg", "cover.png", "back.png"] {
            fs::write(d.join(f), b"x").unwrap();
        }
        let s = survey_batch(&Scope::folder(root), "", 4);
        assert_eq!((s.total, s.shared_names), (3, 1));
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
        let dest = batch_one(&src.join(rel), &out.join(batch_dest(rel, Path::new(""), true)), r, false).unwrap();
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

    fn manifest_scope(root: &Path) -> Scope {
        let r = root.display();
        let m: Manifest = serde_json::from_str(&format!(
            r#"{{"libraryRoot":"{r}","releases":[
              {{"dir":"{r}/Mr.Oizo/Lambs Anger","artist":"Mr.Oizo","title":"Lambs Anger"}},
              {{"dir":"{r}/Lifted/Trellis","artist":"Lifted","title":"Trellis"}},
              {{"dir":"{r}/Nobody/Nothing","artist":"Nobody","title":"Nothing"}}
            ]}}"#
        ))
        .unwrap();
        Scope::published(&m)
    }

    #[test]
    fn the_survey_says_what_would_be_touched_without_rendering_anything() {
        // Ported from the macOS BatchTests of the same name.
        let root = tmpdir("survey");
        for (artist, title, files) in [
            ("Mr.Oizo", "Lambs Anger", &["cover.jpg", "back.jpg"][..]),
            ("Lifted", "Trellis", &["cover.jpg"][..]),
            ("Nobody", "Nothing", &["notes.txt"][..]),
        ] {
            let d = root.join(artist).join(title);
            fs::create_dir_all(&d).unwrap();
            for f in files {
                fs::write(d.join(f), b"x").unwrap();
            }
        }
        let scope = manifest_scope(&root);

        let all = survey_batch(&scope, "", 4);
        assert_eq!(all.total, 3);
        assert_eq!(all.groups.len(), 2, "the release with only a .txt is not a group");
        assert_eq!(all.empty_roots, 1, "but it is counted, because 'nothing matched' is news");
        // Named from the manifest, in name order: a list of records answers
        // "what will this touch" better than a list of paths.
        let names: Vec<&str> = all.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Lifted \u{2014} Trellis", "Mr.Oizo \u{2014} Lambs Anger"]);
        assert_eq!(all.groups[0].files, ["Lifted/Trellis/cover.jpg"]);

        let covers = survey_batch(&scope, "cover", 4);
        assert_eq!(covers.total, 2);
        assert!(covers.groups.iter().all(|g| g.files.iter().all(|f| f.ends_with("cover.jpg"))));

        // The survey and the run must agree, or the count on the button is a lie.
        assert_eq!(covers.total, find_in_scope(&scope, "cover").0.len());
        assert_eq!(all.total, find_in_scope(&scope, "").0.len());
        // And the sample is what a run starts with, in run order.
        assert_eq!(all.first, find_in_scope(&scope, "").0[..3]);
    }

    #[test]
    fn a_folder_scope_is_named_by_its_own_shape_since_no_manifest_names_it() {
        let root = tmpdir("survey-folder");
        let d = root.join("Artist/Release");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("cover.png"), b"x").unwrap();
        let s = survey_batch(&Scope::folder(root.clone()), "", 4);
        assert_eq!(s.total, 1);
        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.groups[0].files, ["Artist/Release/cover.png"]);
        assert_eq!(s.groups[0].name, "xcolor-batch-survey-folder");
    }

    #[test]
    fn a_file_under_two_roots_is_counted_once() {
        let root = tmpdir("survey-dupes");
        let inner = root.join("a");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("cover.png"), b"x").unwrap();
        let scope = Scope {
            roots: vec![root.clone(), inner],
            strip: root.clone(),
            guard_root: root,
            names: HashMap::new(),
        };
        let s = survey_batch(&scope, "", 4);
        assert_eq!(s.total, 1, "the count must match what a run writes");
        assert_eq!(s.total, find_in_scope(&scope, "").0.len());
        assert_eq!(s.empty_roots, 1, "the inner root's only file went to the outer one");
    }

    #[test]
    fn an_unreadable_release_is_reported_not_counted_as_empty() {
        let root = tmpdir("survey-missing");
        let scope = manifest_scope(&root); // none of the three dirs exist
        let s = survey_batch(&scope, "", 4);
        assert_eq!((s.total, s.empty_roots, s.unreadable), (0, 0, 3));
        assert_eq!(find_in_scope(&scope, "").1.len(), 3, "a run reports them too");
    }

    #[test]
    fn a_manifest_without_artist_or_title_still_loads() {
        let m: Manifest = serde_json::from_str(
            r#"{"libraryRoot":"/m","releases":[{"dir":"/m/A/B"}]}"#,
        )
        .unwrap();
        let s = Scope::published(&m);
        assert_eq!(s.name_of(Path::new("/m/A/B")), "A/B");
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

        let dest = batch_one(&src.join(rel), &batch_dest(rel, &out, false), r, true).unwrap();
        assert_eq!(dest, out.join("cover.png"));
        assert!(!dest.exists(), "a dry run writes nothing");

        // And a file that would really fail, fails in the dry run too — that is
        // what makes it worth running.
        fs::write(src.join("broken-cover.png"), b"not a png").unwrap();
        let bad = Path::new("broken-cover.png");
        assert!(batch_one(&src.join(bad), &batch_dest(bad, &out, false), r, true).is_err());
    }
}
