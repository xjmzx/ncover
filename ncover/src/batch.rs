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
    /// Not a framing onto a square: shrink anything whose longer side is over
    /// `Recipe::canvas` down to it, keep the aspect, keep the file's format,
    /// and leave everything already within the cap alone (not written at
    /// all). This is the tool for tidying a library's covers, where the other
    /// two would crop or pad every non-square cover, enlarge every small one
    /// and turn 66 KB JPEGs into 420 KB PNGs.
    Cap,
    /// Crop a non-square image to a square of its shorter side, taking the
    /// centre of the long side, then shrink it to `Recipe::canvas` if it is
    /// still larger, as Cap does. Keeps the file's name and format and writes
    /// nothing for an image that is already square. Meant for "a select
    /// few" (a list scope), since a centre crop is a per-cover judgement.
    Square,
    /// Square without cropping: put a non-square image in the middle of a
    /// square of its LONGER side, fill the bars with the `PadFill`, then
    /// shrink to `Recipe::canvas` if larger. For an image whose edges carry
    /// art or type (a label logo, a cover with text to the edge). Like
    /// Square it keeps the file's name and format and writes nothing for an
    /// image that is already square, and it takes the same list lines.
    Pad(PadFill),
}

/// What fills Pad's bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PadFill {
    /// Each bar takes the average of the image edge it sits against (the
    /// top row for the top bar, and so on), alpha included, so a
    /// transparent PNG logo gets transparent bars.
    Edge,
    Solid(Rgb),
}

impl Framing {
    /// Cap and Square tidy a file in place: same name, same format, and a
    /// file they would not change is not written at all.
    pub(crate) fn keeps_file(self) -> bool {
        matches!(self, Framing::Cap | Framing::Square | Framing::Pad(_))
    }

    /// Square and Pad both make a non-square image square; they differ only
    /// in whether the long side is cropped or the short side is filled.
    pub(crate) fn squares(self) -> bool {
        matches!(self, Framing::Square | Framing::Pad(_))
    }

    /// The `ncover:<job>` action this framing carries out in ndisc's cover
    /// list. Cover and Fit are not jobs ndisc asks for.
    pub(crate) fn job(self) -> Option<&'static str> {
        match self {
            Framing::Cap => Some("cap"),
            // ndisc marks "not square"; cropping or padding is the
            // operator's choice, so Pad takes the same lines.
            Framing::Square | Framing::Pad(_) => Some("square"),
            Framing::Cover | Framing::Fit => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Recipe {
    /// The square's side for Cover / Fit; the longest side allowed for Cap.
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

/// One named file, under the same rules as the walk: an image extension, and
/// the name filter. A file the list names but the disk does not have is an
/// error for its root, so the survey counts it as unreadable rather than
/// quietly as "matched nothing".
fn named_image(file: &Path, needle: &str, into: &mut Vec<PathBuf>) -> Result<()> {
    if !file.is_file() {
        anyhow::bail!("the list names {}, which is not there", file.display());
    }
    let name = file.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let ext_ok = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()));
    if ext_ok && (needle.is_empty() || name.contains(&needle.to_lowercase())) {
        into.push(file.to_path_buf());
    }
    Ok(())
}

// ---------- a cover list (ndisc's Export list) ----------
//
// ndisc's "Local covers vs Blossom" exports one line per release, no header,
// tab-separated: folder · cover file · artist — title · disk WxH · published
// WxH · format · kinds · actions, with `-` for an empty field. Only ndisc knows
// what is published, so the list is how it says which covers need work;
// ncover reads it and never ndisc's database. A hand-made list of folders,
// one per line, works too: every column after the first is optional.

/// The actions vocabulary, in run order. ncover acts on the `ncover:` ones;
/// the rest are counted so the survey can say what is someone else's.
pub(crate) const LIST_ACTIONS: [(&str, &str); 7] = [
    ("resource", "need a new source"),
    ("review", "to review"),
    ("ncover:square", "to square"),
    ("ncover:cap", "to cap"),
    ("ncover:convert", "to convert"),
    ("ndisc:reconcile", "for ndisc to reconcile"),
    ("ndisc:upload", "for ndisc to upload"),
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListLine {
    pub(crate) folder: PathBuf,
    /// The release's cover, when the line names one. Otherwise the folder is
    /// walked like any other root.
    pub(crate) cover: Option<PathBuf>,
    /// "Artist — Title", for the Files list.
    pub(crate) name: Option<String>,
    pub(crate) actions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CoverList {
    pub(crate) lines: Vec<ListLine>,
    /// The folders' common parent: the mirror prefix and the guard boundary.
    pub(crate) root: PathBuf,
    /// Whether any line carries an actions column. Without one, every line
    /// is in every run.
    pub(crate) has_actions: bool,
    /// Lines that could not be used (not an absolute folder), with the line
    /// number, so a bad list says so instead of shrinking quietly.
    pub(crate) skipped: Vec<usize>,
}

impl CoverList {
    pub(crate) fn selects(&self, line: &ListLine, job: Option<&str>) -> bool {
        match job {
            Some(j) if self.has_actions => {
                let want = format!("ncover:{j}");
                line.actions.iter().any(|a| *a == want)
            }
            _ => true,
        }
    }

    /// What a run of `job` leaves out, by each line's first action in run
    /// order: "247 to square · 112 need a new source". Counted, not touched.
    pub(crate) fn left_out(&self, job: Option<&str>) -> String {
        if !self.has_actions || job.is_none() {
            return String::new();
        }
        let mut counts = [0usize; LIST_ACTIONS.len()];
        let mut none = 0;
        for line in self.lines.iter().filter(|l| !self.selects(l, job)) {
            match LIST_ACTIONS.iter().position(|(a, _)| line.actions.iter().any(|x| x == a)) {
                Some(i) => counts[i] += 1,
                None => none += 1,
            }
        }
        let mut parts: Vec<String> = LIST_ACTIONS
            .iter()
            .zip(counts)
            .filter(|(_, n)| *n > 0)
            .map(|((_, what), n)| format!("{n} {what}"))
            .collect();
        if none > 0 {
            parts.push(format!("{none} with nothing to do"));
        }
        parts.join(" \u{b7} ")
    }
}

/// A list field, or `None` for an empty one or ndisc's `-`.
fn list_field(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|v| !v.is_empty() && *v != "-")
}

pub(crate) fn parse_cover_list(text: &str) -> CoverList {
    let field = list_field;
    let mut lines = Vec::new();
    let mut skipped = Vec::new();
    let mut has_actions = false;
    for (i, raw) in text.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = raw.split('\t').collect();
        let folder = PathBuf::from(cols[0].trim());
        if !folder.is_absolute() {
            skipped.push(i + 1);
            continue;
        }
        // The second column is the cover file only when it is one: an image
        // path inside the folder. A hand-made list's notes are ignored.
        let cover = field(cols.get(1).copied()).map(PathBuf::from).filter(|p| {
            p.starts_with(&folder)
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| IMAGE_EXTS.contains(&e.to_lowercase().as_str()))
        });
        let actions: Vec<String> = match cols.get(7) {
            Some(a) => {
                has_actions = true;
                field(Some(a))
                    .map(|a| a.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                    .unwrap_or_default()
            }
            None => Vec::new(),
        };
        lines.push(ListLine {
            folder,
            cover,
            name: field(cols.get(2).copied()).map(str::to_string),
            actions,
        });
    }
    let root = common_parent(lines.iter().map(|l| l.folder.as_path()));
    CoverList { lines, root, has_actions, skipped }
}

pub(crate) fn load_cover_list(path: &Path) -> Result<CoverList> {
    let text = fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let l = parse_cover_list(&text);
    if l.lines.is_empty() {
        anyhow::bail!("{} holds no release folders", path.display());
    }
    Ok(l)
}

/// The deepest folder that holds every folder's PARENT, so the output keeps
/// at least one level of each release's path: a one-line list mirrors to
/// `<release>/cover.jpg`, not to a bare `cover.jpg`.
fn common_parent<'a>(folders: impl Iterator<Item = &'a Path>) -> PathBuf {
    let mut root: Option<PathBuf> = None;
    for f in folders {
        let parent = f.parent().unwrap_or(f);
        root = Some(match root {
            None => parent.to_path_buf(),
            Some(r) => {
                let mut c = PathBuf::new();
                for (a, b) in r.components().zip(parent.components()) {
                    if a != b {
                        break;
                    }
                    c.push(a.as_os_str());
                }
                c
            }
        });
    }
    root.unwrap_or_else(|| PathBuf::from("/"))
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
    /// Walk roots that contribute only NAMED files instead of a walk: a
    /// list's `cover file` column, so a release's back or inlay is never
    /// touched. Several lines may name files in ONE folder (label art: one
    /// folder of logos), so each root holds every file named in it, in list
    /// order. Empty for a folder or the manifest.
    pub(crate) only: HashMap<PathBuf, Vec<PathBuf>>,
    /// Display names for single files, where the line that named a file
    /// also named it ("Ultra Records"). Wins over `names`, which a folder
    /// shared by several lines cannot carry.
    pub(crate) file_names: HashMap<PathBuf, String>,
}

impl Scope {
    pub(crate) fn folder(root: PathBuf) -> Scope {
        Scope {
            roots: vec![root.clone()],
            strip: root.clone(),
            guard_root: root,
            names: HashMap::new(),
            only: HashMap::new(),
            file_names: HashMap::new(),
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
            only: HashMap::new(),
            file_names: HashMap::new(),
        }
    }

    /// The lines of a cover list this run would act on: the ones whose
    /// actions include `ncover:<job>`. A list without an actions column (a
    /// hand-made one), or a framing that is not a job (Cover, Fit), takes
    /// every line. Each line's folder is a walk root; where the line names
    /// its cover file, that file is the only one taken from the folder.
    pub(crate) fn list(l: &CoverList, job: Option<&str>) -> Scope {
        let mut s = Scope {
            roots: Vec::new(),
            strip: l.root.clone(),
            guard_root: l.root.clone(),
            names: HashMap::new(),
            only: HashMap::new(),
            file_names: HashMap::new(),
        };
        // A folder is one root however many lines name it. A line with no
        // cover file asks for the whole folder, which takes in anything
        // another line named there.
        let mut walk_whole = std::collections::HashSet::new();
        let mut lines_in: HashMap<PathBuf, usize> = HashMap::new();
        for line in l.lines.iter().filter(|line| l.selects(line, job)) {
            let n = lines_in.entry(line.folder.clone()).or_default();
            if *n == 0 {
                s.roots.push(line.folder.clone());
            }
            *n += 1;
            match &line.cover {
                Some(f) => {
                    let named = s.only.entry(line.folder.clone()).or_default();
                    if !named.contains(f) {
                        named.push(f.clone());
                    }
                    if let Some(n) = &line.name {
                        s.file_names.insert(f.clone(), n.clone());
                    }
                }
                None => {
                    walk_whole.insert(line.folder.clone());
                }
            }
            if let Some(n) = &line.name {
                s.names.insert(line.folder.clone(), n.clone());
            }
        }
        for f in &walk_whole {
            s.only.remove(f);
        }
        // A folder several lines share is called by its own name, not by
        // whichever line came last.
        for (f, n) in lines_in {
            if n > 1 {
                s.names.remove(&f);
            }
        }
        s
    }

    /// What to call one file in the grid and the Files list: the name on
    /// record for the nearest folder above it (so a cover in `Disc 2` is
    /// still "Artist — Title"), else its folder under the scope, so a folder
    /// scope's two Ambers read `Autechre/Amber` and `Boards of Canada/Amber`
    /// rather than both the scope's own name. The macOS sibling's
    /// `nameForFile`, ported.
    pub(crate) fn name_for_file(&self, file: &Path) -> String {
        if let Some(n) = self.file_names.get(file) {
            return n.clone();
        }
        let mut dir = file.parent();
        while let Some(d) = dir {
            if let Some(n) = self.names.get(d) {
                return n.clone();
            }
            if d == self.strip {
                break;
            }
            dir = d.parent();
        }
        match file.parent().and_then(|d| d.strip_prefix(&self.strip).ok()) {
            Some(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
            _ => self.name_of(&self.strip),
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
        if let Some(files) = scope.only.get(root) {
            // One error per missing file, each naming it, all under this root.
            for file in files {
                if let Err(e) = named_image(file, needle, &mut found) {
                    errors.push((root.clone(), format!("{e:#}")));
                }
            }
        } else if let Err(e) = find_images(root, needle, &mut found) {
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
    /// Every file in run order. Cap reads their headers to say how many it
    /// would actually change.
    pub(crate) files: Vec<PathBuf>,
    /// The release (or folder) each of `files` belongs to, by index.
    pub(crate) names: Vec<String>,
}

pub(crate) fn survey_batch(scope: &Scope, needle: &str) -> Survey {
    let (hits, errors) = walk_scope(scope, needle);
    let mut by_root: Vec<Vec<String>> = vec![Vec::new(); scope.roots.len()];
    let mut all: Vec<(&PathBuf, usize)> = Vec::with_capacity(hits.len());
    let rels: Vec<PathBuf> = hits
        .iter()
        .map(|(_, f)| f.strip_prefix(&scope.strip).unwrap_or(f).to_path_buf())
        .collect();
    for (i, f) in &hits {
        by_root[*i].push(f.strip_prefix(&scope.strip).unwrap_or(f).display().to_string());
        all.push((f, *i));
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
        names: all.iter().map(|(f, _)| scope.name_for_file(f)).collect(),
        files: all.into_iter().map(|(f, _)| f.clone()).collect(),
    }
}

/// May Overwrite write over this source? PNG and JPEG, each kept in its own
/// format (`cap_format`, the rule Cap and Square already follow): a JPEG is
/// never quietly turned into a PNG under the same name. Anything else
/// (WebP, SVG, GIF) is not offered.
pub(crate) fn src_can_overwrite(path: &Option<PathBuf>) -> bool {
    path.as_deref().is_some_and(|p| cap_format(p).is_ok())
}

/// Does any pixel have alpha below 255?
pub(crate) fn has_transparency(pb: &gdk_pixbuf::Pixbuf) -> bool {
    if !pb.has_alpha() {
        return false;
    }
    let (w, h, n, rs) = (pb.width() as usize, pb.height() as usize, pb.n_channels() as usize, pb.rowstride() as usize);
    let px = unsafe { pb.pixels() };
    (0..h).any(|y| (0..w).any(|x| px[y * rs + x * n + 3] != 255))
}

/// The same pixels without the alpha channel.
fn drop_alpha(pb: &gdk_pixbuf::Pixbuf) -> Result<gdk_pixbuf::Pixbuf> {
    let (w, h) = (pb.width(), pb.height());
    let out = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, w, h)
        .ok_or_else(|| anyhow::anyhow!("could not allocate {w}\u{d7}{h}"))?;
    let (n, rs, ors) = (pb.n_channels() as usize, pb.rowstride() as usize, out.rowstride() as usize);
    let src = unsafe { pb.pixels() };
    let dst = unsafe { out.pixels() };
    for y in 0..h as usize {
        for x in 0..w as usize {
            dst[y * ors + x * 3..y * ors + x * 3 + 3].copy_from_slice(&src[y * rs + x * n..y * rs + x * n + 3]);
        }
    }
    Ok(out)
}

/// JPEG has no alpha: a result with transparent pixels is refused for a
/// `.jpg` rather than flattened onto a colour nobody chose. Shared by
/// Overwrite and the library write-back, so a dry run refuses what the real
/// write would.
pub(crate) fn refuse_alpha_for(pb: &gdk_pixbuf::Pixbuf, path: &Path) -> Result<()> {
    if cap_format(path)?.0 == "jpeg" && has_transparency(pb) {
        anyhow::bail!(
            "the result has transparent areas and JPEG cannot hold them. Frame it with Cover, \
             or use a White or Colour disc, or Save PNG instead"
        );
    }
    Ok(())
}

/// Write `pb` over `path` in the file's own format (JPEG at quality 92, PNG
/// as PNG). JPEG has no alpha, so a result with transparent pixels is refused
/// with the reason rather than flattened onto a colour nobody chose. The
/// file is written beside the original and renamed over it, so a failed
/// encode leaves the original as it was.
pub(crate) fn overwrite_in_place(pb: &gdk_pixbuf::Pixbuf, path: &Path) -> Result<()> {
    let (fmt, opts) = cap_format(path)?;
    refuse_alpha_for(pb, path)?;
    // The JPEG encoder refuses RGBA outright (glycin on Ubuntu 26.04), even
    // when every pixel is opaque, so hand it RGB.
    let rgb;
    let pb = if fmt == "jpeg" && pb.has_alpha() {
        rgb = drop_alpha(pb)?;
        &rgb
    } else {
        pb
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("cover");
    let tmp = path.with_file_name(format!(".{name}.ncover-tmp"));
    if let Err(e) = pb.savev(&tmp, fmt, &opts) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow::Error::msg(e.to_string()));
    }
    // Keep the original's permissions: a library file may be read by others.
    if let Ok(m) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, m.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        anyhow::Error::msg(e.to_string())
    })
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
        Framing::Cap => {
            // The pixels as a viewer shows them: a phone photo stored
            // sideways with an EXIF turn would otherwise come out sideways,
            // because the re-encoded file no longer carries the tag.
            let pb = pb.apply_embedded_orientation().unwrap_or(pb);
            return match cap_size(pb.width(), pb.height(), r.canvas) {
                Some((nw, nh)) => area_resample(&pb, nw, nh),
                None => Ok(pb),
            };
        }
        Framing::Square => {
            let pb = pb.apply_embedded_orientation().unwrap_or(pb);
            let (w, h) = (pb.width(), pb.height());
            let side = w.min(h);
            let sq = pb.new_subpixbuf((w - side) / 2, (h - side) / 2, side, side).copy()
                .context("could not crop to a square")?;
            return if side > r.canvas {
                area_resample(&sq, r.canvas, r.canvas)
            } else {
                Ok(sq)
            };
        }
        Framing::Pad(fill) => {
            let pb = pb.apply_embedded_orientation().unwrap_or(pb);
            let sq = pad_to_square(&pb, fill)?;
            let side = sq.width();
            return if side > r.canvas {
                area_resample(&sq, r.canvas, r.canvas)
            } else {
                Ok(sq)
            };
        }
    };
    let framed = compose(&pb, place, r.canvas)?;
    match r.disc {
        Some(fill) => disc_template(&framed, fill),
        None => Ok(framed),
    }
}

/// `pb` in the middle of a square of its longer side, the bars filled per
/// `fill`. Same channels as the source: a JPEG stays RGB, so it can be
/// written back as JPEG. A square image comes back as a copy.
pub(crate) fn pad_to_square(pb: &gdk_pixbuf::Pixbuf, fill: PadFill) -> Result<gdk_pixbuf::Pixbuf> {
    let (w, h) = (pb.width(), pb.height());
    let side = w.max(h);
    let alpha = pb.has_alpha();
    let out = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, alpha, 8, side, side)
        .ok_or_else(|| anyhow::anyhow!("could not allocate {side}\u{d7}{side}"))?;
    let (n, rs) = (pb.n_channels() as usize, pb.rowstride() as usize);
    let src = unsafe { pb.pixels() };
    let px = |x: i32, y: i32| -> [u8; 4] {
        let i = y as usize * rs + x as usize * n;
        [src[i], src[i + 1], src[i + 2], if alpha { src[i + 3] } else { 255 }]
    };
    // The mean of a run of edge pixels.
    let mean = |pts: &mut dyn Iterator<Item = [u8; 4]>| -> [u8; 4] {
        let (mut acc, mut k) = ([0u64; 4], 0u64);
        for p in pts {
            for c in 0..4 {
                acc[c] += p[c] as u64;
            }
            k += 1;
        }
        let k = k.max(1);
        [(acc[0] / k) as u8, (acc[1] / k) as u8, (acc[2] / k) as u8, (acc[3] / k) as u8]
    };
    let solid = |c: Rgb| [c.r, c.g, c.b, 255];
    // (first bar, second bar): top/bottom for a wide image, left/right for a tall one.
    let (a, b) = match fill {
        PadFill::Solid(c) => (solid(c), solid(c)),
        PadFill::Edge if w >= h => (
            mean(&mut (0..w).map(|x| px(x, 0))),
            mean(&mut (0..w).map(|x| px(x, h - 1))),
        ),
        PadFill::Edge => (
            mean(&mut (0..h).map(|y| px(0, y))),
            mean(&mut (0..h).map(|y| px(w - 1, y))),
        ),
    };
    let (ox, oy) = ((side - w) / 2, (side - h) / 2);
    let on = out.n_channels() as usize;
    let ors = out.rowstride() as usize;
    let dst = unsafe { out.pixels() };
    for y in 0..side {
        for x in 0..side {
            let i = y as usize * ors + x as usize * on;
            let inside = x >= ox && x < ox + w && y >= oy && y < oy + h;
            let p = if inside {
                px(x - ox, y - oy)
            } else if (w >= h && y < oy) || (w < h && x < ox) {
                a
            } else {
                b
            };
            dst[i..i + on].copy_from_slice(&p[..on]);
        }
    }
    Ok(out)
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

/// An image's pixel size from its header. PNG and JPEG are parsed directly:
/// gdk-pixbuf's `file_info` reads far more of a JPEG than the header (about
/// 6 ms a file, 14 s for a 2,400-cover library), which is too slow to keep a
/// survey current. Anything else falls back to `file_info`.
pub(crate) fn image_dims(path: &Path) -> Option<(i32, i32)> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut head = [0u8; 24];
    f.read_exact(&mut head).ok()?;
    if head.starts_with(b"\x89PNG\r\n\x1a\n") && &head[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
        return Some((w as i32, h as i32));
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        // Walk the marker segments to the first start-of-frame. Usually in
        // the first few KB; give up after 1 MB rather than read a whole file.
        let mut buf = head.to_vec();
        f.take(1 << 20).read_to_end(&mut buf).ok()?;
        let mut i = 2;
        while i + 9 < buf.len() {
            if buf[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = buf[i + 1];
            if m == 0xFF {
                i += 1; // fill byte
                continue;
            }
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
                i += 2; // markers without a length
                continue;
            }
            let len = u16::from_be_bytes([buf[i + 2], buf[i + 3]]) as usize;
            // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC).
            if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                let h = u16::from_be_bytes([buf[i + 5], buf[i + 6]]);
                let w = u16::from_be_bytes([buf[i + 7], buf[i + 8]]);
                return Some((w as i32, h as i32));
            }
            i += 2 + len;
        }
        return None;
    }
    gdk_pixbuf::Pixbuf::file_info(path).map(|(_, w, h)| (w, h))
}

/// Which files Cap would actually change, read from headers only (nothing is
/// decoded, so this is fast enough to keep current). A file whose header
/// cannot be read counts as "would change": the run will open it properly,
/// and either shrink it or fail it with the reason. Returns the count and the
/// first `first_n` such files in run order, for the sample.
/// Would a Cap or Square run write this file? From the header alone, and the
/// same answer `batch_one` acts on. Unreadable headers say yes: the run opens
/// the file properly and either changes it or fails it with the reason.
pub(crate) fn would_change(f: &Path, r: Recipe) -> bool {
    if is_svg(f) {
        return false;
    }
    match (r.framing, image_dims(f)) {
        (Framing::Cap, Some((w, h))) => cap_size(w, h, r.canvas).is_some(),
        (Framing::Square | Framing::Pad(_), Some((w, h))) => w != h,
        _ => true,
    }
}

/// `cap_survey` for either framing that leaves files alone. The app counts
/// with `plan_items` now; this stays as the tests' independent tally of the
/// same `would_change`, which `plan_items` must agree with.
#[cfg(test)]
pub(crate) fn change_survey(files: &[PathBuf], r: Recipe, first_n: usize) -> (usize, Vec<PathBuf>) {
    let mut over = 0;
    let mut first = Vec::new();
    for f in files {
        if would_change(f, r) {
            over += 1;
            if first.len() < first_n {
                first.push(f.clone());
            }
        }
    }
    (over, first)
}

/// Output paths for a run under recipe `r`. Cap keeps every file's own name
/// and extension (it writes the same format back), so sources are already
/// distinct; the other framings write PNG and go through `plan_dests`.
pub(crate) fn plan_for(rels: &[PathBuf], r: Recipe) -> Vec<std::result::Result<PathBuf, String>> {
    if r.framing.keeps_file() {
        rels.iter().map(|p| Ok(p.clone())).collect()
    } else {
        plan_dests(rels, r.disc.is_some())
    }
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
///
/// `Ok(None)` means "left alone": only Cap and Square do that, for a file
/// already within the cap or already square (or a vector, which has no
/// pixel size).
pub(crate) fn batch_one(src: &Path, dest: &Path, r: Recipe, dry: bool) -> Result<Option<PathBuf>> {
    let dest = dest.to_path_buf();
    let format = if r.framing.keeps_file() {
        // The header is enough to know a file needs nothing (within the cap,
        // already square), so most of a library is never decoded.
        if !would_change(src, r) {
            return Ok(None);
        }
        cap_format(src)?
    } else {
        ("png", Vec::new())
    };
    let mut final_pb = batch_render(src, r)?;
    // The JPEG encoder refuses RGBA, even fully opaque. Square and Cap keep
    // a JPEG's RGB, but say so here rather than trust every framing to.
    if format.0 == "jpeg" && final_pb.has_alpha() {
        final_pb = drop_alpha(&final_pb)?;
    }
    if dry {
        return Ok(Some(dest));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let opts: Vec<(&str, &str)> = format.1.iter().map(|(k, v)| (*k, *v)).collect();
    final_pb
        .savev(&dest, format.0, &opts)
        .with_context(|| format!("could not write {}", dest.display()))?;
    Ok(Some(dest))
}

fn is_svg(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// The size a file shrinks to under `cap`, or `None` if it is already within
/// it. Aspect is kept; the longer side becomes exactly `cap`.
pub(crate) fn cap_size(w: i32, h: i32, cap: i32) -> Option<(i32, i32)> {
    let long = w.max(h);
    if long <= cap || w <= 0 || h <= 0 {
        return None;
    }
    let s = cap as f64 / long as f64;
    Some((((w as f64 * s).round() as i32).max(1), ((h as f64 * s).round() as i32).max(1)))
}

/// JPEG stays JPEG and PNG stays PNG: Cap tidies a file, it does not change
/// what kind of file it is. Quality 92 because the image is being made
/// smaller, which hides re-encoding loss, and a photo cover has no alpha to
/// need PNG for. Anything else (WebP, GIF) is refused with the reason, not
/// silently converted.
pub(crate) fn cap_format(src: &Path) -> Result<(&'static str, Vec<(&'static str, &'static str)>)> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Ok(("jpeg", vec![("quality", "92")])),
        "png" => Ok(("png", Vec::new())),
        _ => Err(anyhow::anyhow!(format!(
            "Cap and Square keep the file's format and cannot write .{ext}; left as it is"
        ))),
    }
}

/// Resize by AREA AVERAGING, for shrinking: each output pixel is the
/// coverage-weighted mean of the source pixels under it. The nearest-pixel
/// sampling `compose` uses is right for a colour tool's enlargements (it
/// invents no colours) but wrong for shrinking, where it throws most pixels
/// away and keeps whichever happen to land on the grid, so fine detail and
/// type shimmer. Averaging is done on PREMULTIPLIED values, or a transparent
/// pixel's colour bleeds into its opaque neighbours as a dark fringe.
pub(crate) fn area_resample(src: &gdk_pixbuf::Pixbuf, nw: i32, nh: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let (nw_u, nh_u) = (nw as usize, nh as usize);
    let n = src.n_channels() as usize;
    let rs = src.rowstride() as usize;
    let alpha = src.has_alpha();
    let px = unsafe { src.pixels() };

    // For each output index, the source indices it covers and by how much.
    fn spans(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
        let k = from as f64 / to as f64;
        (0..to)
            .map(|o| {
                let (a, b) = (o as f64 * k, (o + 1) as f64 * k);
                let mut v = Vec::new();
                let mut i = a.floor() as usize;
                while (i as f64) < b && i < from {
                    let cover = (b.min(i as f64 + 1.0) - a.max(i as f64)) as f32;
                    if cover > 0.0 {
                        v.push((i, cover));
                    }
                    i += 1;
                }
                v
            })
            .collect()
    }
    let xs = spans(w, nw_u);
    let ys = spans(h, nh_u);

    // Pass 1: rows, source width → output width, premultiplied f32 RGBA.
    let mut mid = vec![0f32; h * nw_u * 4];
    for y in 0..h {
        let row = &px[y * rs..];
        for (ox, span) in xs.iter().enumerate() {
            let mut acc = [0f32; 4];
            let mut wsum = 0f32;
            for &(sx, c) in span {
                let p = &row[sx * n..sx * n + n];
                let a = if alpha { p[3] as f32 / 255.0 } else { 1.0 };
                acc[0] += p[0] as f32 * a * c;
                acc[1] += p[1] as f32 * a * c;
                acc[2] += p[2] as f32 * a * c;
                acc[3] += a * c;
                wsum += c;
            }
            let o = (y * nw_u + ox) * 4;
            for k in 0..4 {
                mid[o + k] = acc[k] / wsum;
            }
        }
    }
    // Pass 2: columns, then un-premultiply into the output.
    let out = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, alpha, 8, nw, nh)
        .context("could not allocate the resized image")?;
    let ors = out.rowstride() as usize;
    let on = out.n_channels() as usize;
    let opx = unsafe { out.pixels() };
    for (oy, span) in ys.iter().enumerate() {
        for ox in 0..nw_u {
            let mut acc = [0f32; 4];
            let mut wsum = 0f32;
            for &(sy, c) in span {
                let m = &mid[(sy * nw_u + ox) * 4..];
                for k in 0..4 {
                    acc[k] += m[k] * c;
                }
                wsum += c;
            }
            let a = acc[3] / wsum;
            let o = oy * ors + ox * on;
            for k in 0..3 {
                let v = if a > 0.0 { acc[k] / wsum / a } else { 0.0 };
                opx[o + k] = v.round().clamp(0.0, 255.0) as u8;
            }
            if alpha {
                opx[o + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(out)
}

/// Progress from the worker thread. Deliberately carries the REASON on failure:
/// "31 failed" is unactionable and sends you guessing at the tool from the
/// outside — the single most valuable thing ntree's sampler learned.
pub(crate) enum BatchMsg {
    Total(usize),
    One {
        rel: String,
        /// `Ok(None)`: left alone (Cap, already within it).
        result: std::result::Result<Option<PathBuf>, String>,
    },
    Done {
        ok: usize,
        /// Left alone because they were already within the cap.
        left: usize,
        failed: usize,
        stopped: bool,
    },
}

/// Thumbnail side in the Batch grid, px. Big enough to see a bad crop or a
/// wrong corner fill.
pub(crate) const GRID_PX: i32 = 150;

/// A thumbnail's raw parts: `(rgba, w, h, rowstride, has_alpha)`. Pixbuf is
/// `!Send`; these bytes are, so this is what crosses back from a worker.
pub(crate) type ThumbBytes = (Vec<u8>, i32, i32, i32, bool);

/// Shrink a pixbuf to fit a `px` square, as Send-able bytes.
fn thumb_of(full: &gdk_pixbuf::Pixbuf, px: i32) -> Result<ThumbBytes> {
    // Fit, not stretch: a Cap output keeps its own shape.
    let k = px as f64 / full.width().max(full.height()) as f64;
    let (tw, th) = (
        ((full.width() as f64 * k).round() as i32).max(1),
        ((full.height() as f64 * k).round() as i32).max(1),
    );
    let t = full
        .scale_simple(tw, th, gdk_pixbuf::InterpType::Bilinear)
        .context("could not scale the thumbnail")?;
    Ok((unsafe { t.pixels() }.to_vec(), t.width(), t.height(), t.rowstride(), t.has_alpha()))
}

// ---------- the review list: every file a Cap or Square run would change ----------

/// How much of an image a centre crop to a square throws away, 0.0 – 1.0.
pub(crate) fn trim_share(w: i32, h: i32) -> f64 {
    let (long, short) = (w.max(h), w.min(h));
    if long <= 0 {
        return 0.0;
    }
    1.0 - short as f64 / long as f64
}

/// The files a Cap or Square run would write, in the order worth reviewing
/// them: under Square the most cropped first, under Cap the largest first.
/// Sizes come from headers, so ordering a few hundred covers is instant. A
/// file whose header cannot be read goes first: the run will open it
/// properly, and the review is where you find out how that goes.
pub(crate) fn review_order(hits: Vec<(usize, PathBuf)>, r: Recipe) -> Vec<(usize, PathBuf)> {
    let mut v: Vec<(f64, (usize, PathBuf))> = hits
        .into_iter()
        .filter(|(_, f)| would_change(f, r))
        .map(|h| {
            let key = match image_dims(&h.1) {
                None => f64::INFINITY,
                Some((w, h)) if r.framing.squares() => trim_share(w, h),
                Some((w, h)) => w.max(h) as f64,
            };
            (key, h)
        })
        .collect();
    // Stable, so equal keys keep the run's order.
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    v.into_iter().map(|(_, h)| h).collect()
}

/// One file of the review: the source as a viewer shows it, the output as the
/// run would write it, both with their sizes. The output goes through
/// `batch_render`, the function the run writes from.
pub(crate) struct Review {
    pub(crate) before: std::result::Result<(ThumbBytes, (i32, i32)), String>,
    pub(crate) after: std::result::Result<(ThumbBytes, (i32, i32)), String>,
}

pub(crate) fn review_one(src: &Path, r: Recipe, px: i32) -> Review {
    let before = gdk_pixbuf::Pixbuf::from_file(src)
        .context("could not read the image")
        .and_then(|pb| {
            let pb = pb.apply_embedded_orientation().unwrap_or(pb);
            Ok((thumb_of(&pb, px)?, (pb.width(), pb.height())))
        })
        .map_err(|e| format!("{e:#}"));
    let after = batch_render(src, r)
        .and_then(|pb| Ok((thumb_of(&pb, px)?, (pb.width(), pb.height()))))
        .map_err(|e| format!("{e:#}"));
    Review { before, after }
}

/// What a run does to one file, in words: "600 × 533 → 533 × 533 · 67 px off
/// the sides (11%)".
pub(crate) fn change_words(from: (i32, i32), to: (i32, i32)) -> String {
    let (w, h) = from;
    let mut t = format!("{w} \u{d7} {h} \u{2192} {} \u{d7} {}", to.0, to.1);
    if w != h && to.0 == to.1 {
        let where_ = if w > h { "the sides" } else { "top and bottom" };
        t += &format!(
            " \u{b7} {} px off {where_} ({:.0}%)",
            (w - h).abs(),
            trim_share(w, h) * 100.0
        );
    } else if to != from {
        t += &format!(" \u{b7} shrunk to {:.0}%", to.0 as f64 / w.max(1) as f64 * 100.0);
    }
    t
}

/// Pad's change in words: "600 × 533 → 600 × 600 · 67 px added top and
/// bottom". `to` is the written size, so a pad that is then shrunk says so.
pub(crate) fn pad_words(from: (i32, i32), to: (i32, i32)) -> String {
    let (w, h) = from;
    let mut t = format!("{w} \u{d7} {h} \u{2192} {} \u{d7} {}", to.0, to.1);
    if w != h {
        let where_ = if w > h { "top and bottom" } else { "the sides" };
        t += &format!(" \u{b7} {} px added {where_}", (w - h).abs());
        if to.0 < w.max(h) {
            t += &format!(", then shrunk to {:.0}%", to.0 as f64 / w.max(h) as f64 * 100.0);
        }
    }
    t
}

/// One file in the Batch grid and the numbered Files list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Planned {
    /// The release (or folder) it belongs to.
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    /// From the header; `None` when it cannot be read.
    pub(crate) dims: Option<(i32, i32)>,
}

/// What the grid and the Files list show, in the order they show it: under
/// Cap and Square only the files the run would change (`review_order`: the
/// most cropped or the largest first), otherwise every file in run order.
/// Both views read this one list, so their numbers are the same numbers, and
/// its length under Cap / Square is `change_survey`'s count.
pub(crate) fn plan_items(files: &[PathBuf], names: &[String], r: Recipe) -> Vec<Planned> {
    let picked: Vec<(usize, PathBuf)> = if r.framing.keeps_file() {
        review_order(files.iter().cloned().enumerate().collect(), r)
    } else {
        files.iter().cloned().enumerate().collect()
    };
    picked
        .into_iter()
        .map(|(i, path)| Planned {
            name: names.get(i).cloned().unwrap_or_default(),
            dims: image_dims(&path),
            path,
        })
        .collect()
}

/// The size the run writes for a source of `src` pixels, as `batch_render`
/// decides it. From the header, so it ignores an EXIF quarter turn; the grid
/// shows the rendered size once it has one.
pub(crate) fn out_dims(src: (i32, i32), r: Recipe) -> (i32, i32) {
    let (w, h) = src;
    match r.framing {
        Framing::Cover | Framing::Fit => (r.canvas, r.canvas),
        Framing::Cap => cap_size(w, h, r.canvas).unwrap_or((w, h)),
        Framing::Square => {
            let s = w.min(h).min(r.canvas);
            (s, s)
        }
        Framing::Pad(_) => {
            let s = w.max(h).min(r.canvas);
            (s, s)
        }
    }
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
    fn a_file_is_named_by_its_record_else_by_its_folder_under_the_scope() {
        let lib = PathBuf::from("/lib");
        let plain = Scope::folder(lib.clone());
        assert_eq!(plain.name_for_file(Path::new("/lib/Autechre/Amber/cover.jpg")), "Autechre/Amber");
        assert_eq!(plain.name_for_file(Path::new("/lib/Boards of Canada/Amber/cover.jpg")), "Boards of Canada/Amber");
        assert_eq!(plain.name_for_file(Path::new("/lib/cover.jpg")), "lib", "a file at the root: the folder's own name");
        let mut named = Scope::folder(lib.clone());
        named.names.insert(lib.join("Autechre/Amber"), "Autechre \u{2014} Amber".into());
        assert_eq!(
            named.name_for_file(Path::new("/lib/Autechre/Amber/Disc 2/cover.jpg")),
            "Autechre \u{2014} Amber",
            "a disc subfolder keeps the release's name"
        );
    }

    #[test]
    fn siblings_are_fine() {
        let base = tmpdir("siblings");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&out).unwrap();
        assert!(guard_batch(&src, &out).is_ok());
    }

    fn solid(w: i32, h: i32, alpha: bool, rgba: u32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, alpha, 8, w, h).unwrap();
        pb.fill(rgba);
        pb
    }
    fn set(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32, v: [u8; 4]) {
        let n = pb.n_channels() as usize;
        let i = y as usize * pb.rowstride() as usize + x as usize * n;
        let b = unsafe { pb.pixels() };
        b[i..i + n].copy_from_slice(&v[..n]);
    }

    #[test]
    fn overwrite_squares_a_jpeg_in_place_and_batch_then_leaves_it_alone() {
        let d = tmpdir("overwrite-jpeg");
        let f = d.join("cover.jpg");
        // A typical flagged cover: a little wider than tall, larger than the canvas.
        solid(1000, 873, false, 0x336699ff).savev(&f, "jpeg", &[("quality", "90")]).unwrap();
        let square = Recipe { canvas: 600, framing: Framing::Square, disc: None };
        assert!(would_change(&f, square));

        // What Picker holds after opening it: the source covering the canvas.
        let doc = Composition::new(gdk_pixbuf::Pixbuf::from_file(&f).unwrap(), 600);
        let pb = render_for_write(&doc).unwrap();
        assert!(!has_transparency(&pb), "Cover leaves no gap, even after rounding a shrink");
        overwrite_in_place(&pb, &f).unwrap();

        let bytes = fs::read(&f).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "still a JPEG under its own name");
        assert_eq!(image_dims(&f), Some((600, 600)));
        assert!(!would_change(&f, square), "Batch's recount drops it");
        let left: Vec<_> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("cover.jpg")], "no temporary file left behind");
    }

    #[test]
    fn overwrite_refuses_a_jpeg_with_transparent_areas_and_leaves_it_untouched() {
        let d = tmpdir("overwrite-alpha");
        let f = d.join("cover.jpg");
        solid(600, 300, false, 0x336699ff).savev(&f, "jpeg", &[]).unwrap();
        let before = fs::read(&f).unwrap();
        let mut doc = Composition::new(gdk_pixbuf::Pixbuf::from_file(&f).unwrap(), 600);
        doc.set_placement(Placement::fit(600, 300, 600)); // bars above and below
        let pb = render_for_write(&doc).unwrap();
        let e = overwrite_in_place(&pb, &f).unwrap_err().to_string();
        assert!(e.contains("transparent"), "{e}");
        assert_eq!(fs::read(&f).unwrap(), before);
    }

    #[test]
    fn overwrite_keeps_a_png_a_png_and_only_png_and_jpeg_are_offered() {
        let d = tmpdir("overwrite-png");
        let f = d.join("cover.png");
        solid(300, 300, true, 0x11223380).savev(&f, "png", &[]).unwrap();
        let doc = Composition::new(gdk_pixbuf::Pixbuf::from_file(&f).unwrap(), 200);
        overwrite_in_place(&render_for_write(&doc).unwrap(), &f).unwrap();
        assert_eq!(&fs::read(&f).unwrap()[1..4], b"PNG");
        assert_eq!(image_dims(&f), Some((200, 200)));
        for (name, ok) in [("a.png", true), ("a.JPG", true), ("a.jpeg", true), ("a.webp", false), ("a.svg", false)] {
            assert_eq!(src_can_overwrite(&Some(PathBuf::from(name))), ok, "{name}");
        }
        assert!(!src_can_overwrite(&None), "a Blank canvas has nothing to overwrite");
    }

    #[test]
    fn fit_plus_a_background_makes_an_off_square_jpeg_overwritable_without_cropping() {
        let d = tmpdir("overwrite-background");
        let f = d.join("cover.jpg");
        solid(600, 548, false, 0x2244aaff).savev(&f, "jpeg", &[("quality", "95")]).unwrap();
        let mut doc = Composition::new(gdk_pixbuf::Pixbuf::from_file(&f).unwrap(), 600);
        doc.set_placement(Placement::fit(600, 548, 600)); // bars of 26 px above and below
        assert!(overwrite_in_place(&render_for_write(&doc).unwrap(), &f).is_err(), "bars are transparent");

        let magenta = Rgb { r: 0xd0, g: 0x10, b: 0x80 };
        doc.ops.push(Operation::Background(magenta));
        let pb = render_for_write(&doc).unwrap();
        assert!(!has_transparency(&pb));
        let px = unsafe { pb.pixels() };
        let at = |x: usize, y: usize| {
            let i = y * pb.rowstride() as usize + x * 4;
            [px[i], px[i + 1], px[i + 2]]
        };
        assert_eq!(at(300, 5), [0xd0, 0x10, 0x80], "the top bar is the background");
        assert_eq!(at(300, 594), [0xd0, 0x10, 0x80], "and the bottom one");
        let mid = at(300, 300);
        assert!(mid[2] > 0x90 && mid[0] < 0x50, "the picture is untouched: {mid:?}");

        overwrite_in_place(&pb, &f).unwrap();
        assert_eq!(image_dims(&f), Some((600, 600)));
        let square = Recipe { canvas: 600, framing: Framing::Square, disc: None };
        assert!(!would_change(&f, square), "Batch's Square now leaves it alone");
    }

    #[test]
    fn a_background_blends_soft_edges_and_leaves_opaque_pixels() {
        let pb = solid(3, 1, true, 0x00000000);
        set(&pb, 0, 0, [200, 100, 0, 255]); // opaque
        set(&pb, 1, 0, [200, 100, 0, 128]); // half
        let out = fill_behind(&pb, Rgb { r: 0, g: 0, b: 255 }).unwrap();
        let px = unsafe { out.pixels() };
        assert_eq!(&px[0..4], &[200, 100, 0, 255]);
        assert_eq!(&px[4..8], &[100, 50, 127, 255]);
        assert_eq!(&px[8..12], &[0, 0, 255, 255]);
    }

    #[test]
    fn pad_squares_a_wide_jpeg_without_cropping_and_keeps_it_jpeg() {
        let d = tmpdir("pad-jpeg");
        let (src, out) = (d.join("in/cover.jpg"), d.join("out/cover.jpg"));
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        // 600×500: red bands at the top, blue at the bottom, grey between.
        // Bands, not single rows: a JPEG encoder that subsamples colour
        // (the CI runner's does) blurs a one-pixel row into its neighbours.
        let pb = solid(600, 500, false, 0x808080ff);
        for y in 0..16 {
            for x in 0..600 {
                set(&pb, x, y, [255, 0, 0, 255]);
                set(&pb, x, 499 - y, [0, 0, 255, 255]);
            }
        }
        pb.savev(&src, "jpeg", &[("quality", "100")]).unwrap();
        let r = Recipe { canvas: 600, framing: Framing::Pad(PadFill::Edge), disc: None };
        assert!(would_change(&src, r));
        assert_eq!(Framing::Pad(PadFill::Edge).job(), Some("square"), "the same list lines as Square");

        let got = batch_render(&src, r).unwrap();
        assert_eq!((got.width(), got.height()), (600, 600));
        assert_eq!(out_dims((600, 500), r), (600, 600));
        assert!(!got.has_alpha(), "a JPEG stays RGB");
        let px = unsafe { got.pixels() };
        let at = |x: usize, y: usize| px[y * got.rowstride() as usize + x * 3..][..3].to_vec();
        assert!(at(300, 10)[0] > 200 && at(300, 10)[2] < 60, "top bar takes the top edge: {:?}", at(300, 10));
        assert!(at(300, 590)[2] > 200 && at(300, 590)[0] < 60, "bottom bar takes the bottom edge: {:?}", at(300, 590));
        let mid = at(300, 300);
        assert!((110..=150).contains(&mid[0]), "the picture is all there, not cropped: {mid:?}");

        batch_one(&src, &out, r, false).unwrap().unwrap();
        assert_eq!(&fs::read(&out).unwrap()[..2], &[0xFF, 0xD8]);
        assert_eq!(image_dims(&out), Some((600, 600)));
        assert!(!would_change(&out, r), "already square now: left alone next time");
    }

    #[test]
    fn pad_gives_a_transparent_logo_transparent_bars_and_shrinks_a_large_one() {
        // A tall 400×800 PNG logo, transparent at its edges.
        let pb = solid(400, 800, true, 0x00000000);
        for y in 300..500 {
            for x in 100..300 {
                set(&pb, x, y, [10, 20, 30, 255]);
            }
        }
        let sq = pad_to_square(&pb, PadFill::Edge).unwrap();
        assert_eq!((sq.width(), sq.height()), (800, 800));
        let px = unsafe { sq.pixels() };
        let i = 400 * sq.rowstride() as usize + 50 * 4;
        assert_eq!(px[i + 3], 0, "the left bar is transparent, like the edge beside it");

        let d = tmpdir("pad-logo");
        let f = d.join("logo.png");
        pb.savev(&f, "png", &[]).unwrap();
        let white = Recipe { canvas: 600, framing: Framing::Pad(PadFill::Solid(Rgb { r: 255, g: 255, b: 255 })), disc: None };
        let got = batch_render(&f, white).unwrap();
        assert_eq!((got.width(), got.height()), (600, 600), "padded to 800, then shrunk to the size");
        assert_eq!(out_dims((400, 800), white), (600, 600));
        assert_eq!(pad_words((400, 800), (600, 600)), "400 \u{d7} 800 \u{2192} 600 \u{d7} 600 \u{b7} 400 px added the sides, then shrunk to 75%");
    }

    #[test]
    fn a_written_shrink_averages_rather_than_picking_pixels() {
        // One-pixel black and white stripes: nearest sampling at half size
        // keeps one colour; averaging gives mid grey everywhere.
        let src = solid(400, 400, false, 0xffffffff);
        for y in 0..400 {
            for x in (0..400).step_by(2) {
                set(&src, x, y, [0, 0, 0, 255]);
            }
        }
        let doc = Composition::new(src, 200);
        let pb = render_for_write(&doc).unwrap();
        let px = unsafe { pb.pixels() };
        let i = 100 * pb.rowstride() as usize + 100 * 4;
        assert!((120..=135).contains(&px[i]), "grey, got {}", px[i]);
    }

    #[test]
    fn the_cap_survey_counts_what_a_cap_run_would_write() {
        let base = tmpdir("cap-survey");
        solid(1000, 1000, false, 0xffffffff).savev(base.join("a.jpg"), "jpeg", &[]).unwrap();
        solid(600, 600, false, 0xffffffff).savev(base.join("b.jpg"), "jpeg", &[]).unwrap();
        solid(900, 300, false, 0xffffffff).savev(base.join("c.png"), "png", &[]).unwrap();
        let files: Vec<PathBuf> = ["a.jpg", "b.jpg", "c.png"].iter().map(|f| base.join(f)).collect();
        let (over, first) = change_survey(&files, Recipe { canvas: 800, framing: Framing::Cap, disc: None }, 4);
        assert_eq!(over, 2);
        assert_eq!(first, [base.join("a.jpg"), base.join("c.png")]);
        // And it agrees with what the run does, file by file.
        let r = Recipe { canvas: 800, framing: Framing::Cap, disc: None };
        let written = files
            .iter()
            .filter(|f| batch_one(f, &base.join("out").join(f.file_name().unwrap()), r, true).unwrap().is_some())
            .count();
        assert_eq!(written, over);
    }

    #[test]
    fn header_sizes_match_what_gdk_pixbuf_decodes() {
        let base = tmpdir("dims");
        solid(123, 45, false, 0x112233ff).savev(base.join("a.jpg"), "jpeg", &[]).unwrap();
        solid(67, 890, true, 0x11223380).savev(base.join("b.png"), "png", &[]).unwrap();
        solid(31, 31, false, 0x112233ff).savev(base.join("c.jpg"), "jpeg", &[("quality", "50")]).unwrap();
        assert_eq!(image_dims(&base.join("a.jpg")), Some((123, 45)));
        assert_eq!(image_dims(&base.join("b.png")), Some((67, 890)));
        assert_eq!(image_dims(&base.join("c.jpg")), Some((31, 31)));
        fs::write(base.join("d.png"), b"not a png").unwrap();
        assert_eq!(image_dims(&base.join("d.png")), None);
    }

    #[test]
    fn cap_shrinks_only_what_is_over_and_keeps_the_shape() {
        assert_eq!(cap_size(600, 600, 800), None, "within the cap: left alone");
        assert_eq!(cap_size(800, 800, 800), None, "exactly the cap is within it");
        assert_eq!(cap_size(1000, 1000, 800), Some((800, 800)));
        assert_eq!(cap_size(1600, 800, 800), Some((800, 400)), "aspect kept, no crop");
        assert_eq!(cap_size(599, 1200, 600), Some((300, 600)));
    }

    #[test]
    fn area_averaging_keeps_every_source_pixel_in_the_mix() {
        // A 1-px black/white checker shrunk 2:1 is mid grey everywhere.
        // Nearest-pixel sampling would return all black or all white.
        let pb = solid(4, 4, false, 0x000000ff);
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 1 {
                    set(&pb, x, y, [255, 255, 255, 255]);
                }
            }
        }
        let out = area_resample(&pb, 2, 2).unwrap();
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (r, g, b, _) = px(&out, x, y);
            assert!((127..=128).contains(&r) && r == g && g == b, "{x},{y}: {r}");
        }
    }

    #[test]
    fn area_averaging_weights_partial_pixels_by_coverage() {
        // 3 → 2: output 0 covers src 0 fully and half of src 1; output 1
        // covers the other half of src 1 and all of src 2.
        let pb = solid(3, 1, false, 0x000000ff);
        set(&pb, 2, 0, [255, 255, 255, 255]);
        let out = area_resample(&pb, 2, 1).unwrap();
        assert_eq!(px(&out, 0, 0).0, 0);
        assert_eq!(px(&out, 1, 0).0, 170, "(0·0.5 + 255·1) / 1.5");
    }

    #[test]
    fn area_averaging_is_premultiplied_so_edges_do_not_darken() {
        // Opaque red beside fully transparent black. Straight averaging
        // gives dark red at half alpha; premultiplied gives red at half alpha.
        let pb = solid(2, 1, true, 0x00000000);
        set(&pb, 0, 0, [255, 0, 0, 255]);
        let out = area_resample(&pb, 1, 1).unwrap();
        assert_eq!(px(&out, 0, 0), (255, 0, 0, 128));
    }

    #[test]
    fn cap_leaves_small_files_unwritten_and_keeps_the_format() {
        let base = tmpdir("cap");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        solid(1200, 600, false, 0x3366ffff).savev(src.join("big.jpg"), "jpeg", &[]).unwrap();
        solid(500, 500, false, 0x3366ffff).savev(src.join("small.jpg"), "jpeg", &[]).unwrap();
        solid(900, 900, true, 0x3366ff80).savev(src.join("alpha.png"), "png", &[]).unwrap();
        let r = Recipe { canvas: 800, framing: Framing::Cap, disc: None };

        let rels: Vec<PathBuf> = ["big.jpg", "small.jpg", "alpha.png"].map(PathBuf::from).to_vec();
        let plan = plan_for(&rels, r);
        let dest = |i: usize| out.join(plan[i].as_ref().unwrap());
        assert_eq!(plan[0].as_deref().unwrap(), Path::new("big.jpg"), "Cap keeps the name and extension");

        assert!(batch_one(&src.join("small.jpg"), &dest(1), r, false).unwrap().is_none());
        assert!(!dest(1).exists(), "within the cap: not written at all");

        let d = batch_one(&src.join("big.jpg"), &dest(0), r, false).unwrap().unwrap();
        let (fmt, w, h) = gdk_pixbuf::Pixbuf::file_info(&d).unwrap();
        assert_eq!((fmt.name().unwrap().as_str(), w, h), ("jpeg", 800, 400));

        let d = batch_one(&src.join("alpha.png"), &dest(2), r, false).unwrap().unwrap();
        let got = gdk_pixbuf::Pixbuf::from_file(&d).unwrap();
        assert_eq!((got.width(), got.height(), got.has_alpha()), (800, 800, true));
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
    #[ignore] // reads the real catalogue; run explicitly. Dry run only.
    fn cap_dry_run_over_the_real_catalogue() {
        let m = load_manifest(&published_manifest_path().expect("no manifest")).unwrap();
        let scope = Scope::published(&m);
        let (files, _) = find_in_scope(&scope, "cover");
        let mismatched: Vec<_> = files
            .iter()
            .filter(|f| image_dims(f) != gdk_pixbuf::Pixbuf::file_info(f).map(|(_, w, h)| (w, h)))
            .collect();
        println!("header reader disagrees with gdk-pixbuf on {} files: {mismatched:?}", mismatched.len());
        for cap in [800, 600] {
            let t = std::time::Instant::now();
            let (over, _) = change_survey(&files, Recipe { canvas: cap, framing: Framing::Cap, disc: None }, 4);
            let survey_ms = t.elapsed().as_millis();
            let r = Recipe { canvas: cap, framing: Framing::Cap, disc: None };
            let rels: Vec<PathBuf> = files.iter().map(|f| f.strip_prefix(&scope.strip).unwrap().to_path_buf()).collect();
            let t = std::time::Instant::now();
            let (mut ok, mut left, mut bad) = (0, 0, Vec::new());
            for ((f, rel), d) in files.iter().zip(&rels).zip(plan_for(&rels, r)) {
                match batch_one(f, &Path::new("/nonexistent-out").join(d.unwrap()), r, true) {
                    Ok(Some(_)) => ok += 1,
                    Ok(None) => left += 1,
                    Err(e) => bad.push(format!("{}: {e:#}", rel.display())),
                }
            }
            println!(
                "cap {cap}: survey says {over} ({survey_ms} ms); dry run: {ok} would be written, {left} left alone, {} failed ({} ms)",
                bad.len(),
                t.elapsed().as_millis()
            );
            for b in &bad {
                println!("   {b}");
            }
            assert_eq!(over, ok + bad.len(), "the count on the button is what the run touches");
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
        let s = survey_batch(&Scope::folder(root), "");
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
        let t = full.scale_simple(GRID_PX, GRID_PX, gdk_pixbuf::InterpType::Bilinear).unwrap();
        assert_eq!((t.width(), t.height()), (GRID_PX, GRID_PX));
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
        let dest = batch_one(&src.join(rel), &out.join(batch_dest(rel, Path::new(""), true)), r, false).unwrap().unwrap();
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

        let all = survey_batch(&scope, "");
        assert_eq!(all.total, 3);
        assert_eq!(all.groups.len(), 2, "the release with only a .txt is not a group");
        assert_eq!(all.empty_roots, 1, "but it is counted, because 'nothing matched' is news");
        // Named from the manifest, in name order: a list of records answers
        // "what will this touch" better than a list of paths.
        let names: Vec<&str> = all.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Lifted \u{2014} Trellis", "Mr.Oizo \u{2014} Lambs Anger"]);
        assert_eq!(all.groups[0].files, ["Lifted/Trellis/cover.jpg"]);

        let covers = survey_batch(&scope, "cover");
        assert_eq!(covers.total, 2);
        assert!(covers.groups.iter().all(|g| g.files.iter().all(|f| f.ends_with("cover.jpg"))));

        // The survey and the run must agree, or the count on the button is a lie.
        assert_eq!(covers.total, find_in_scope(&scope, "cover").0.len());
        assert_eq!(all.total, find_in_scope(&scope, "").0.len());
        // Files are in run order, each named by the release it came from.
        assert_eq!(all.files, find_in_scope(&scope, "").0);
        assert_eq!(all.names.len(), all.files.len());
    }

    #[test]
    fn a_folder_scope_is_named_by_its_own_shape_since_no_manifest_names_it() {
        let root = tmpdir("survey-folder");
        let d = root.join("Artist/Release");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("cover.png"), b"x").unwrap();
        let s = survey_batch(&Scope::folder(root.clone()), "");
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
            only: HashMap::new(),
            file_names: HashMap::new(),
        };
        let s = survey_batch(&scope, "");
        assert_eq!(s.total, 1, "the count must match what a run writes");
        assert_eq!(s.total, find_in_scope(&scope, "").0.len());
        assert_eq!(s.empty_roots, 1, "the inner root's only file went to the outer one");
    }

    #[test]
    fn an_unreadable_release_is_reported_not_counted_as_empty() {
        let root = tmpdir("survey-missing");
        let scope = manifest_scope(&root); // none of the three dirs exist
        let s = survey_batch(&scope, "");
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

        let dest = batch_one(&src.join(rel), &batch_dest(rel, &out, false), r, true).unwrap().unwrap();
        assert_eq!(dest, out.join("cover.png"));
        assert!(!dest.exists(), "a dry run writes nothing");

        // And a file that would really fail, fails in the dry run too — that is
        // what makes it worth running.
        fs::write(src.join("broken-cover.png"), b"not a png").unwrap();
        let bad = Path::new("broken-cover.png");
        assert!(batch_one(&src.join(bad), &batch_dest(bad, &out, false), r, true).is_err());
    }

    // ---------- cover lists and Square ----------

    const EXPORT: &str = "\
/lib/A/One\t/lib/A/One/cover.jpg\tA \u{2014} One\t600x535\t600x535\t-\toff-square-10.8%\tncover:square
/lib/B/Two\t/lib/B/Two/cover.png\tB \u{2014} Two\t1200x1200\t1200x1200\tpng\tover-600,png\tncover:cap,ncover:convert
/lib/C/Three\t-\tC \u{2014} Three\t-\t600x600\t-\tno-local\tresource
/lib/D/Four\t/lib/D/Four/cover.jpg\tD \u{2014} Four\t400x392\t600x598\t-\tdifferent-picture\treview
relative/folder\t-\t-
";

    #[test]
    fn a_cover_list_reads_ndiscs_export_columns() {
        let l = parse_cover_list(EXPORT);
        assert_eq!(l.lines.len(), 4);
        assert_eq!(l.skipped, [5], "a relative folder is reported, not used");
        assert!(l.has_actions);
        assert_eq!(l.root, PathBuf::from("/lib"), "the folders' common parent");
        let a = &l.lines[0];
        assert_eq!(a.cover.as_deref(), Some(Path::new("/lib/A/One/cover.jpg")));
        assert_eq!(a.name.as_deref(), Some("A \u{2014} One"));
        assert_eq!(a.actions, ["ncover:square"]);
        assert_eq!(l.lines[1].actions, ["ncover:cap", "ncover:convert"]);
        assert_eq!(l.lines[2].cover, None, "`-` is an empty field");
    }

    #[test]
    fn a_hand_made_list_of_folders_takes_every_line() {
        let l = parse_cover_list("/m/x/Release\n\n# a note\n/m/y/Other\tsome note\n");
        assert_eq!(l.lines.len(), 2);
        assert!(!l.has_actions);
        assert_eq!(l.lines[1].cover, None, "a note in column two is not a cover file");
        assert_eq!(l.root, PathBuf::from("/m"));
        assert_eq!(Scope::list(&l, Some("square")).roots.len(), 2);
        assert_eq!(l.left_out(Some("square")), "");
        // One folder still mirrors to <release>/cover.jpg, not a bare name.
        assert_eq!(parse_cover_list("/m/x/Release\n").root, PathBuf::from("/m/x"));
    }

    #[test]
    fn the_framing_picks_the_lines_and_counts_the_rest() {
        let l = parse_cover_list(EXPORT);
        let sq = Scope::list(&l, Framing::Square.job());
        assert_eq!(sq.roots, [PathBuf::from("/lib/A/One")]);
        assert_eq!(sq.names[&PathBuf::from("/lib/A/One")], "A \u{2014} One");
        assert_eq!(Scope::list(&l, Framing::Cap.job()).roots, [PathBuf::from("/lib/B/Two")]);
        assert_eq!(Scope::list(&l, Framing::Cover.job()).roots.len(), 4, "Cover is no job: every line");
        assert_eq!(
            l.left_out(Framing::Square.job()),
            "1 need a new source \u{b7} 1 to review \u{b7} 1 to cap"
        );
    }

    #[test]
    fn a_list_takes_only_the_named_cover_and_the_count_matches_the_run() {
        let root = tmpdir("list-scope");
        let rel = root.join("Artist/Release");
        fs::create_dir_all(&rel).unwrap();
        solid(600, 535, false, 0xffffffff).savev(rel.join("cover.jpg"), "jpeg", &[]).unwrap();
        solid(600, 535, false, 0xffffffff).savev(rel.join("back-cover.jpg"), "jpeg", &[]).unwrap();
        let gone = root.join("Artist/Gone");
        let text = format!(
            "{r}\t{r}/cover.jpg\tArtist \u{2014} Release\t-\t-\t-\t-\tncover:square\n\
             {g}\t{g}/cover.jpg\tArtist \u{2014} Gone\t-\t-\t-\t-\tncover:square\n",
            r = rel.display(),
            g = gone.display()
        );
        let scope = Scope::list(&parse_cover_list(&text), Some("square"));
        let s = survey_batch(&scope, "cover");
        assert_eq!(s.total, 1, "the back cover is never taken");
        assert_eq!(s.files, [rel.join("cover.jpg")]);
        assert_eq!(s.unreadable, 1, "a named file that is missing is counted, not hidden");
        assert_eq!(s.total, find_in_scope(&scope, "cover").0.len());
        // Both folders sit under Artist, so that is the mirror prefix.
        assert_eq!(s.groups[0].files, ["Release/cover.jpg"], "mirrored under the common parent");
    }

    #[test]
    fn many_lines_naming_files_in_one_folder_take_every_one_of_them() {
        // Label art: one folder of logos, one line per label. Keyed by
        // folder, the list kept only the last line's file.
        let base = tmpdir("list-shared");
        let logos = base.join("labels");
        fs::create_dir_all(&logos).unwrap();
        for f in ["Ultra.Records.label.jpg", "Warp.label.png", "Mute.label.jpg", "unlisted.jpg"] {
            let fmt = if f.ends_with("png") { "png" } else { "jpeg" };
            solid(400, 300, false, 0x808080ff).savev(logos.join(f), fmt, &[]).unwrap();
        }
        let line = |f: &str, n: &str| {
            format!("{d}\t{d}/{f}\t{n}\t-\t-\t-\t-\tncover:square\n", d = logos.display())
        };
        let text = [
            line("Ultra.Records.label.jpg", "Ultra Records"),
            line("Warp.label.png", "Warp"),
            line("Mute.label.jpg", "Mute"),
            line("Gone.label.jpg", "Gone"),
        ]
        .concat();
        let scope = Scope::list(&parse_cover_list(&text), Some("square"));
        assert_eq!(scope.roots, [logos.clone()], "one folder is one root");
        assert!(scope.names.is_empty(), "a shared folder carries no one line's name");

        let s = survey_batch(&scope, "");
        let mut got: Vec<_> = s.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, ["Mute.label.jpg", "Ultra.Records.label.jpg", "Warp.label.png"], "every named file, and only those");
        assert_eq!(s.unreadable, 1, "the missing one is counted, by itself");
        assert_eq!(s.empty_roots, 0);
        assert_eq!(s.total, find_in_scope(&scope, "").0.len(), "the count is the run");
        for (f, n) in s.files.iter().zip(&s.names) {
            let want = match f.file_name().unwrap().to_str().unwrap() {
                "Ultra.Records.label.jpg" => "Ultra Records",
                "Warp.label.png" => "Warp",
                _ => "Mute",
            };
            assert_eq!(n, want, "each file is named by its own line");
        }

        // A line with no cover file for that folder asks for all of it.
        let whole = format!("{text}{}\n", logos.display());
        let all = survey_batch(&Scope::list(&parse_cover_list(&whole), None), "");
        assert_eq!(all.total, 4, "the walk takes the unlisted file too");
    }

    #[test]
    fn the_review_lists_only_what_changes_most_cropped_first() {
        let base = tmpdir("review");
        solid(600, 535, false, 0x808080ff).savev(base.join("insert.png"), "png", &[]).unwrap();
        solid(500, 500, false, 0x808080ff).savev(base.join("square.png"), "png", &[]).unwrap();
        solid(300, 600, false, 0x808080ff).savev(base.join("tall.png"), "png", &[]).unwrap();
        let hits: Vec<(usize, PathBuf)> =
            ["insert.png", "square.png", "tall.png"].iter().map(|f| (0, base.join(f))).collect();
        let r = Recipe { canvas: 600, framing: Framing::Square, disc: None };
        let order = review_order(hits.clone(), r);
        let names: Vec<_> = order.iter().map(|(_, f)| f.file_name().unwrap().to_owned()).collect();
        assert_eq!(names, ["tall.png", "insert.png"], "square files are left out; the worst crop leads");
        let files: Vec<PathBuf> = hits.iter().map(|h| h.1.clone()).collect();
        assert_eq!(order.len(), change_survey(&files, r, 0).0);

        // The grid's list is that order, named, with the header sizes; and
        // the size it promises is the one the run writes.
        let names = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let items = plan_items(&files, &names, r);
        let got: Vec<_> = items.iter().map(|p| (p.name.as_str(), p.dims)).collect();
        assert_eq!(got, [("C", Some((300, 600))), ("A", Some((600, 535)))]);
        for p in &items {
            let real = batch_render(&p.path, r).unwrap();
            assert_eq!(out_dims(p.dims.unwrap(), r), (real.width(), real.height()));
        }
        let big = Recipe { canvas: 400, ..r };
        assert_eq!(out_dims((1000, 800), big), (400, 400));
        let cap = Recipe { framing: Framing::Cap, ..big };
        assert_eq!(out_dims((1000, 800), cap), cap_size(1000, 800, 400).unwrap());
        // Cover / Fit take every file, in run order.
        let all = plan_items(&files, &names, Recipe { framing: Framing::Cover, ..r });
        assert_eq!(all.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), files);

        // The sizes it shows are the ones the run writes.
        let rv = review_one(&base.join("insert.png"), r, 64);
        assert_eq!(rv.before.unwrap().1, (600, 535));
        assert_eq!(rv.after.unwrap().1, (535, 535));
        assert_eq!(change_words((600, 535), (535, 535)), "600 \u{d7} 535 \u{2192} 535 \u{d7} 535 \u{b7} 65 px off the sides (11%)");
        assert_eq!(change_words((300, 600), (300, 300)), "300 \u{d7} 600 \u{2192} 300 \u{d7} 300 \u{b7} 300 px off top and bottom (50%)");
        assert_eq!(change_words((1200, 1000), (600, 500)), "1200 \u{d7} 1000 \u{2192} 600 \u{d7} 500 \u{b7} shrunk to 50%");
    }

    #[test]
    fn square_crops_the_centre_keeps_the_format_and_skips_square_files() {
        let base = tmpdir("square");
        let out = tmpdir("square-out");
        // 30×10: the centre square is x 10..20. Mark its corners.
        let wide = solid(30, 10, false, 0x000000ff);
        set(&wide, 10, 0, [255, 0, 0, 255]);
        set(&wide, 19, 9, [0, 255, 0, 255]);
        wide.savev(base.join("wide.png"), "png", &[]).unwrap();
        solid(600, 535, false, 0x808080ff).savev(base.join("insert.jpg"), "jpeg", &[]).unwrap();
        solid(1000, 800, false, 0x808080ff).savev(base.join("big.png"), "png", &[]).unwrap();
        solid(500, 500, false, 0x808080ff).savev(base.join("square.jpg"), "jpeg", &[]).unwrap();
        let r = Recipe { canvas: 600, framing: Framing::Square, disc: None };

        let w = batch_one(&base.join("wide.png"), &out.join("wide.png"), r, false).unwrap().unwrap();
        let pb = gdk_pixbuf::Pixbuf::from_file(&w).unwrap();
        assert_eq!((pb.width(), pb.height()), (10, 10));
        let px = |x: i32, y: i32| {
            let n = pb.n_channels() as usize;
            let i = y as usize * pb.rowstride() as usize + x as usize * n;
            let b = unsafe { pb.pixels() };
            [b[i], b[i + 1], b[i + 2]]
        };
        assert_eq!(px(0, 0), [255, 0, 0], "the crop starts at the centre, not the edge");
        assert_eq!(px(9, 9), [0, 255, 0]);

        let j = batch_one(&base.join("insert.jpg"), &out.join("insert.jpg"), r, false).unwrap().unwrap();
        assert_eq!(image_dims(&j), Some((535, 535)), "smaller than the size: cropped, not enlarged");
        assert_eq!(fs::read(&j).unwrap()[..2], [0xFF, 0xD8], "JPEG stays JPEG");

        let b = batch_one(&base.join("big.png"), &out.join("big.png"), r, false).unwrap().unwrap();
        assert_eq!(image_dims(&b), Some((600, 600)), "cropped to 800, then held to the size");

        assert_eq!(batch_one(&base.join("square.jpg"), &out.join("square.jpg"), r, false).unwrap(), None);
        assert!(!out.join("square.jpg").exists(), "a square file is not written at all");

        // The survey's count is what the run writes.
        let files: Vec<PathBuf> =
            ["big.png", "insert.jpg", "square.jpg", "wide.png"].iter().map(|f| base.join(f)).collect();
        assert_eq!(change_survey(&files, r, 4).0, 3);
        assert_eq!(plan_for(&[PathBuf::from("a/cover.jpg")], r), [Ok(PathBuf::from("a/cover.jpg"))]);
    }

    #[test]
    #[ignore] // reads a real cover list: NCOVER_LIST=<ndisc export> cargo test -p ncover real_list -- --ignored --nocapture
    fn real_list_counts_agree_with_the_run() {
        let path = PathBuf::from(std::env::var("NCOVER_LIST").expect("set NCOVER_LIST"));
        let l = load_cover_list(&path).unwrap();
        println!("{} lines under {} ({} skipped)", l.lines.len(), l.root.display(), l.skipped.len());
        for f in [Framing::Square, Framing::Cap] {
            let r = Recipe { canvas: 600, framing: f, disc: None };
            let scope = Scope::list(&l, f.job());
            let s = survey_batch(&scope, "cover");
            let (change, _) = change_survey(&s.files, r, 4);
            println!(
                "{f:?}: {} lines, {} files, {} unreadable, {change} would change; not in this run: {}",
                scope.roots.len(),
                s.total,
                s.unreadable,
                l.left_out(f.job())
            );
            // Dry-run every file: the count the button shows is what a run writes.
            let rels: Vec<PathBuf> =
                s.files.iter().map(|x| x.strip_prefix(&scope.strip).unwrap().to_path_buf()).collect();
            let out = std::env::temp_dir().join("ncover-real-list-dry");
            let mut written = 0;
            for ((src, rel), dest) in s.files.iter().zip(&rels).zip(plan_for(&rels, r)) {
                let _ = rel;
                if let Ok(Some(_)) = batch_one(src, &out.join(dest.unwrap()), r, true) {
                    written += 1;
                }
            }
            assert_eq!(written, change, "{f:?}: the survey and the dry run disagree");
            assert_eq!(plan_items(&s.files, &s.names, r).len(), change, "{f:?}: the grid lists another set");
        }
    }
}
