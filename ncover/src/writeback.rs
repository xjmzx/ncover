use super::*;

// ---------- writing a run back into the library ----------
//
// Step 3 of the cover review loop: Cap, Square or Pad replaces each cover
// where it lives, and keeps the original. Before this the run went to an
// output folder and a desktop script (`swap-covers.py`, 2026-11-13) swapped
// the results in.
//
// The rules, each of which is a test below:
//
// - **Only the keep-format jobs.** Cap, Square and Pad write the file's own
//   name and format, so `cover.jpg` is still `cover.jpg` and ndisc's
//   `cover_art_path` still points at it. Cover and Fit write a PNG, often
//   under another name, which would orphan that path.
// - **The library never loses a file.** Per file: copy the original into the
//   originals folder (to a temp name, synced, length checked, then renamed
//   into its slot), write the result beside the source and rename it over it,
//   then log. A copy, not a move, so the originals folder may be on another
//   disk; a crash at any point leaves the old file or the new one in the
//   library, never neither.
// - **The original's slot decides.** A file whose slot in the originals
//   folder is already taken is refused: a second run must never overwrite
//   the first original.
// - **The path is the source's own.** The library file written is the walked
//   source itself, never a path rebuilt from names, and the kept slot is the
//   source's FULL path under the scope's root. So `Autechre/Amber/cover.jpg`
//   and `Boards/Amber/cover.jpg` keep their originals in two different
//   places, and a file that is not under the root is refused rather than
//   given a made-up place.
// - **Every swap is logged** in `ncover-log.tsv` in the originals folder,
//   plain TSV, and the last run can be put back from it.

/// The log, in the root of the originals folder.
pub(crate) const WRITEBACK_LOG: &str = "ncover-log.tsv";

const LOG_HEAD: &str = "# n.cover write-back log. One `swap` line per replaced file, one `undo` line per run put back.\n\
# swap\trun\ttime\tjob\tlibrary file\toriginal kept\tbefore\tafter\twritten bytes\twritten mtime (ns)\n\
# undo\trun\ttime\trestored\tskipped\n";

/// The originals folder may not be the library, inside it, or above it.
/// Inside: the next run would walk the kept originals as covers. Above: the
/// library is then inside the originals folder, and a kept slot can be a
/// library path.
pub(crate) fn guard_originals(library: &Path, kept: &Path) -> Result<()> {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (l, k) = (real(library), real(kept));
    if k == l {
        anyhow::bail!("The originals folder is the library itself. Choose a folder outside it.");
    }
    if k.starts_with(&l) {
        anyhow::bail!(
            "The originals folder is inside the library.\n\nThe next run would find the kept originals and treat them as covers. Choose a folder outside it."
        );
    }
    if l.starts_with(&k) {
        anyhow::bail!("The library is inside the originals folder. Choose a folder beside the library, not above it.");
    }
    Ok(())
}

/// A source's path under the scope's root, or the reason it has none. Only
/// plain names: a `..` or a root would let a kept slot escape the originals
/// folder, so they are refused rather than cleaned up.
pub(crate) fn rel_under(f: &Path, root: &Path) -> std::result::Result<PathBuf, String> {
    let rel = f
        .strip_prefix(root)
        .map_err(|_| format!("not under {}; refused rather than given a place of its own", root.display()))?;
    let plain = rel.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if rel.as_os_str().is_empty() || !plain {
        return Err(format!("{} is not a plain path under {}", rel.display(), root.display()));
    }
    Ok(rel.to_path_buf())
}

fn job_word(f: Framing) -> Option<&'static str> {
    match f {
        Framing::Cap => Some("cap"),
        Framing::Square => Some("square"),
        Framing::Pad(_) => Some("pad"),
        _ => None,
    }
}

fn mtime_ns(m: &fs::Metadata) -> u128 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// `cover.jpg` → `.cover.jpg.ncover-tmp`, beside it.
fn tmp_beside(p: &Path) -> PathBuf {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("cover");
    p.with_file_name(format!(".{name}.ncover-tmp"))
}

/// Copy `from` to `to` without ever leaving a partial file at `to`: a temp
/// name beside it, synced, its length checked, its modified time kept, then
/// renamed into place.
fn copy_whole(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    }
    let tmp = tmp_beside(to);
    let done = (|| -> Result<()> {
        let n = fs::copy(from, &tmp)?;
        let meta = fs::metadata(from)?;
        if n != meta.len() {
            anyhow::bail!("copied {n} of {} bytes", meta.len());
        }
        let f = fs::OpenOptions::new().write(true).open(&tmp)?;
        if let Ok(t) = meta.modified() {
            let _ = f.set_modified(t);
        }
        f.sync_all()?;
        fs::rename(&tmp, to)?;
        Ok(())
    })();
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done.with_context(|| format!("could not copy {} to {}", from.display(), to.display()))
}

/// Remove `dir` and its parents while they are empty, stopping at `root`.
fn prune_empty(mut dir: PathBuf, root: &Path) {
    while dir != root && dir.starts_with(root) && fs::remove_dir(&dir).is_ok() {
        if !dir.pop() {
            break;
        }
    }
}

/// One run into the library: where the originals go, and what the log calls
/// this run.
pub(crate) struct WriteBack {
    pub(crate) kept_root: PathBuf,
    pub(crate) run: String,
    time: String,
}

impl WriteBack {
    /// Make the originals folder and the log if needed, and name the run
    /// (its local start time, made unique against the log).
    pub(crate) fn start(kept_root: &Path) -> Result<WriteBack> {
        fs::create_dir_all(kept_root).with_context(|| format!("could not create {}", kept_root.display()))?;
        let now = glib::DateTime::now_local().map_err(|e| anyhow::Error::msg(e.to_string()))?;
        let time = now.format("%Y-%m-%d %H:%M:%S").map(|s| s.to_string()).unwrap_or_default();
        let base = now.format("%Y%m%d-%H%M%S").map(|s| s.to_string()).unwrap_or_default();
        let taken: std::collections::HashSet<String> = read_log(kept_root)?.into_iter().map(|r| r.run).collect();
        let mut run = base.clone();
        let mut n = 2;
        while taken.contains(&run) {
            run = format!("{base}-{n}");
            n += 1;
        }
        Ok(WriteBack { kept_root: kept_root.to_path_buf(), run, time })
    }

    /// Replace `src` (at `rel` under the scope's root) with the recipe's
    /// result, keeping the original at `kept_root/rel`. `Ok(None)`: the job
    /// leaves this file alone. `Ok(Some(kept))`: replaced (or, `dry`, would
    /// be), with the original at `kept`.
    pub(crate) fn one(&self, src: &Path, rel: &Path, r: Recipe, dry: bool) -> Result<Option<PathBuf>> {
        let Some(job) = job_word(r.framing) else {
            anyhow::bail!("only Cap, Square and Pad write into the library: Cover and Fit write a PNG under another name");
        };
        let kept = self.kept_root.join(rel);
        if !kept.starts_with(&self.kept_root) || rel.is_absolute() {
            anyhow::bail!("{} would land outside the originals folder", rel.display());
        }
        for p in [src, kept.as_path()] {
            if p.to_string_lossy().contains(['\t', '\n']) {
                anyhow::bail!("its path holds a tab or a line break, which the log cannot record");
            }
        }
        if !would_change(src, r) {
            return Ok(None);
        }
        if kept.symlink_metadata().is_ok() {
            anyhow::bail!(
                "an original is already kept for this file ({}); move it out of the originals folder to run this file again",
                kept.display()
            );
        }
        cap_format(src)?;
        let pb = batch_render(src, r)?;
        refuse_alpha_for(&pb, src)?;
        if dry {
            return Ok(Some(kept));
        }
        let before = image_dims(src).unwrap_or((0, 0));

        copy_whole(src, &kept)?;
        let forget_kept = || {
            let _ = fs::remove_file(&kept);
            if let Some(p) = kept.parent() {
                prune_empty(p.to_path_buf(), &self.kept_root);
            }
        };
        if let Err(e) = overwrite_in_place(&pb, src) {
            // The library still holds the original (the rename never
            // happened), so the copy is not needed and must not block the
            // slot for a later run.
            forget_kept();
            return Err(e.context(format!("could not write {}", src.display())));
        }
        let meta = fs::metadata(src)?;
        let line = format!(
            "swap\t{}\t{}\t{job}\t{}\t{}\t{}x{}\t{}x{}\t{}\t{}\n",
            self.run,
            self.time,
            src.display(),
            kept.display(),
            before.0,
            before.1,
            pb.width(),
            pb.height(),
            meta.len(),
            mtime_ns(&meta)
        );
        if let Err(e) = append_log(&self.kept_root, &line) {
            // A swap the log does not know about cannot be undone, so it is
            // not left in place: the original goes straight back.
            let back = restore(&kept, src);
            if back.is_ok() {
                forget_kept();
            }
            return Err(e.context(match back {
                Ok(()) => "the log could not be written, so the original was put back".to_string(),
                Err(b) => format!("the log could not be written, and putting the original back failed too ({b:#}); it is at {}", kept.display()),
            }));
        }
        Ok(Some(kept))
    }
}

fn append_log(root: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    let path = root.join(WRITEBACK_LOG);
    let fresh = !path.exists();
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("could not open {}", path.display()))?;
    if fresh {
        f.write_all(LOG_HEAD.as_bytes())?;
    }
    f.write_all(line.as_bytes())?;
    f.sync_data()?;
    Ok(())
}

/// Put `kept` back over `lib`: copied beside it and renamed over it, so the
/// library file is whole at every moment. The original's modified time comes
/// back with it.
fn restore(kept: &Path, lib: &Path) -> Result<()> {
    let tmp = tmp_beside(lib);
    copy_whole(kept, &tmp)?;
    if let Ok(m) = fs::metadata(lib) {
        let _ = fs::set_permissions(&tmp, m.permissions());
    }
    fs::rename(&tmp, lib).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        anyhow::Error::msg(format!("could not put {} back: {e}", lib.display()))
    })
}

/// One line of the log.
#[derive(Debug, Clone)]
pub(crate) struct LogRow {
    pub(crate) undo: bool,
    pub(crate) run: String,
    pub(crate) time: String,
    lib: PathBuf,
    kept: PathBuf,
    bytes: u64,
    mtime: u128,
}

fn read_log(root: &Path) -> Result<Vec<LogRow>> {
    let path = root.join(WRITEBACK_LOG);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(anyhow::Error::new(e).context(format!("could not read {}", path.display()))),
    };
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["swap", run, time, _job, lib, kept, _b, _a, bytes, mtime] => rows.push(LogRow {
                undo: false,
                run: run.to_string(),
                time: time.to_string(),
                lib: PathBuf::from(lib),
                kept: PathBuf::from(kept),
                bytes: bytes.parse().unwrap_or(u64::MAX),
                mtime: mtime.parse().unwrap_or(0),
            }),
            ["undo", run, time, ..] => rows.push(LogRow {
                undo: true,
                run: run.to_string(),
                time: time.to_string(),
                lib: PathBuf::new(),
                kept: PathBuf::new(),
                bytes: 0,
                mtime: 0,
            }),
            _ => {} // the header, or a line a person added
        }
    }
    Ok(rows)
}

/// The most recent run not yet put back: its id, start time and swaps.
pub(crate) fn last_run(root: &Path) -> Result<Option<(String, String, usize)>> {
    let rows = read_log(root)?;
    let undone: std::collections::HashSet<&str> = rows.iter().filter(|r| r.undo).map(|r| r.run.as_str()).collect();
    let Some(last) = rows.iter().rev().find(|r| !r.undo && !undone.contains(r.run.as_str())) else {
        return Ok(None);
    };
    let n = rows.iter().filter(|r| !r.undo && r.run == last.run).count();
    Ok(Some((last.run.clone(), last.time.clone(), n)))
}

/// What putting a run back did.
pub(crate) struct UndoReport {
    pub(crate) run: String,
    pub(crate) restored: usize,
    /// Library files left as they are, each with why. Their originals stay
    /// in the originals folder.
    pub(crate) skipped: Vec<(PathBuf, String)>,
}

/// Put the last run back, newest swap first. A library file that changed
/// since the run (its size or modified time is not what the run wrote) is
/// left alone and named: putting the original back would throw that change
/// away. The run is then marked undone in the log.
pub(crate) fn undo_last(root: &Path) -> Result<Option<UndoReport>> {
    let Some((run, _, _)) = last_run(root)? else { return Ok(None) };
    let rows: Vec<LogRow> = read_log(root)?.into_iter().filter(|r| !r.undo && r.run == run).collect();
    let mut report = UndoReport { run: run.clone(), restored: 0, skipped: Vec::new() };
    for r in rows.iter().rev() {
        let now = match fs::metadata(&r.lib) {
            Ok(m) => m,
            Err(e) => {
                report.skipped.push((r.lib.clone(), format!("cannot be read ({e})")));
                continue;
            }
        };
        if now.len() != r.bytes || mtime_ns(&now) != r.mtime {
            report.skipped.push((r.lib.clone(), "changed since the run; left as it is".into()));
            continue;
        }
        if !r.kept.is_file() {
            report.skipped.push((r.lib.clone(), format!("its original is no longer at {}", r.kept.display())));
            continue;
        }
        match restore(&r.kept, &r.lib) {
            Ok(()) => {
                let _ = fs::remove_file(&r.kept);
                if let Some(p) = r.kept.parent() {
                    prune_empty(p.to_path_buf(), root);
                }
                report.restored += 1;
            }
            Err(e) => report.skipped.push((r.lib.clone(), format!("{e:#}"))),
        }
    }
    let time = glib::DateTime::now_local()
        .ok()
        .and_then(|t| t.format("%Y-%m-%d %H:%M:%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    append_log(root, &format!("undo\t{run}\t{time}\t{}\t{}\n", report.restored, report.skipped.len()))?;
    Ok(Some(report))
}

#[cfg(test)]
mod writeback_tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncover-writeback-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn jpeg(p: &Path, w: i32, h: i32, rgba: u32) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, w, h).unwrap();
        pb.fill(rgba);
        pb.savev(p, "jpeg", &[("quality", "92")]).unwrap();
    }

    fn square() -> Recipe {
        Recipe { canvas: 600, framing: Framing::Square, disc: None }
    }

    #[test]
    fn a_square_run_replaces_the_cover_keeps_the_original_and_logs_it() {
        let base = tmpdir("swap");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let cover = lib.join("Autechre/Amber/cover.jpg");
        jpeg(&cover, 600, 538, 0x336699ff);
        let before = fs::read(&cover).unwrap();

        let wb = WriteBack::start(&kept).unwrap();
        let rel = rel_under(&cover, &lib).unwrap();
        let got = wb.one(&cover, &rel, square(), false).unwrap();

        let k = kept.join("Autechre/Amber/cover.jpg");
        assert_eq!(got.as_deref(), Some(k.as_path()));
        assert_eq!(fs::read(&k).unwrap(), before, "the original is kept byte for byte");
        assert_eq!(image_dims(&cover), Some((538, 538)), "the library file is the result");
        assert!(cover.extension().unwrap() == "jpg" && cap_format(&cover).unwrap().0 == "jpeg");
        let log = fs::read_to_string(kept.join(WRITEBACK_LOG)).unwrap();
        assert_eq!(log.lines().filter(|l| l.starts_with("swap\t")).count(), 1);
        assert!(log.contains("\tsquare\t") && log.contains("600x538\t538x538"));
        assert_eq!(last_run(&kept).unwrap().map(|r| r.2), Some(1));
        // No temp file left in either place.
        for d in [cover.parent().unwrap(), k.parent().unwrap()] {
            assert!(fs::read_dir(d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with("ncover-tmp")));
        }
    }

    #[test]
    fn the_same_title_under_two_artists_keeps_two_originals_in_two_places() {
        // Same release name, same file name, different artist: each library
        // file is written where it is, and each original has its own slot.
        let base = tmpdir("twins");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let a = lib.join("Autechre/Amber/cover.jpg");
        let b = lib.join("Boards of Canada/Amber/cover.jpg");
        jpeg(&a, 600, 538, 0xff0000ff);
        jpeg(&b, 700, 600, 0x0000ffff);
        let (ra, rb) = (fs::read(&a).unwrap(), fs::read(&b).unwrap());

        let wb = WriteBack::start(&kept).unwrap();
        for f in [&a, &b] {
            let rel = rel_under(f, &lib).unwrap();
            assert!(wb.one(f, &rel, square(), false).unwrap().is_some());
        }
        assert_eq!(image_dims(&a), Some((538, 538)));
        assert_eq!(image_dims(&b), Some((600, 600)), "each written where it lives, at its own size");
        assert_eq!(fs::read(kept.join("Autechre/Amber/cover.jpg")).unwrap(), ra);
        assert_eq!(fs::read(kept.join("Boards of Canada/Amber/cover.jpg")).unwrap(), rb);

        let rep = undo_last(&kept).unwrap().unwrap();
        assert_eq!((rep.restored, rep.skipped.len()), (2, 0));
        assert_eq!(fs::read(&a).unwrap(), ra, "each original back in its own folder");
        assert_eq!(fs::read(&b).unwrap(), rb);
    }

    #[test]
    fn a_file_outside_the_root_or_with_dots_has_no_place_and_is_refused() {
        let lib = Path::new("/music");
        assert!(rel_under(Path::new("/elsewhere/Amber/cover.jpg"), lib).is_err());
        assert!(rel_under(Path::new("/music/../etc/cover.jpg"), lib).is_err());
        assert!(rel_under(Path::new("/music"), lib).is_err());
        assert_eq!(rel_under(Path::new("/music/A/B/cover.jpg"), lib).unwrap(), PathBuf::from("A/B/cover.jpg"));
    }

    #[test]
    fn a_taken_slot_is_refused_and_the_library_file_is_untouched() {
        let base = tmpdir("taken");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let cover = lib.join("A/One/cover.jpg");
        jpeg(&cover, 600, 538, 0x336699ff);
        let slot = kept.join("A/One/cover.jpg");
        jpeg(&slot, 10, 10, 0x000000ff); // an earlier run's original
        let (lib_was, slot_was) = (fs::read(&cover).unwrap(), fs::read(&slot).unwrap());

        let wb = WriteBack::start(&kept).unwrap();
        let rel = rel_under(&cover, &lib).unwrap();
        for dry in [true, false] {
            let e = wb.one(&cover, &rel, square(), dry).unwrap_err();
            assert!(format!("{e:#}").contains("already kept"), "{e:#}");
        }
        assert_eq!(fs::read(&cover).unwrap(), lib_was);
        assert_eq!(fs::read(&slot).unwrap(), slot_was, "the first original survives");
    }

    #[test]
    fn a_square_file_is_left_alone_and_a_dry_run_writes_nothing() {
        let base = tmpdir("dry");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let sq = lib.join("A/Sq/cover.jpg");
        let off = lib.join("A/Off/cover.jpg");
        jpeg(&sq, 500, 500, 0x336699ff);
        jpeg(&off, 600, 538, 0x336699ff);
        let was = fs::read(&off).unwrap();
        let wb = WriteBack::start(&kept).unwrap();
        assert!(wb.one(&sq, &rel_under(&sq, &lib).unwrap(), square(), false).unwrap().is_none());
        assert!(wb.one(&off, &rel_under(&off, &lib).unwrap(), square(), true).unwrap().is_some());
        assert_eq!(fs::read(&off).unwrap(), was);
        assert!(!kept.join("A").exists(), "a dry run keeps nothing");
        assert_eq!(last_run(&kept).unwrap(), None);
    }

    #[test]
    fn cover_and_fit_are_refused_for_the_library() {
        let base = tmpdir("cover");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let f = lib.join("A/One/cover.jpg");
        jpeg(&f, 600, 538, 0x336699ff);
        let wb = WriteBack::start(&kept).unwrap();
        let r = Recipe { canvas: 600, framing: Framing::Cover, disc: None };
        assert!(wb.one(&f, &rel_under(&f, &lib).unwrap(), r, true).is_err());
    }

    #[test]
    fn undo_skips_a_file_changed_since_the_run_and_marks_the_run_done() {
        let base = tmpdir("undo");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let a = lib.join("A/One/cover.jpg");
        let b = lib.join("A/Two/cover.jpg");
        jpeg(&a, 600, 538, 0xff0000ff);
        jpeg(&b, 600, 538, 0x00ff00ff);
        let wb = WriteBack::start(&kept).unwrap();
        for f in [&a, &b] {
            wb.one(f, &rel_under(f, &lib).unwrap(), square(), false).unwrap();
        }
        // Edited by hand after the run.
        jpeg(&b, 300, 300, 0x0000ffff);
        let edited = fs::read(&b).unwrap();

        let rep = undo_last(&kept).unwrap().unwrap();
        assert_eq!(rep.restored, 1);
        assert_eq!(rep.skipped.len(), 1);
        assert_eq!(rep.skipped[0].0, b);
        assert_eq!(image_dims(&a), Some((600, 538)), "put back");
        assert_eq!(fs::read(&b).unwrap(), edited, "the later edit is not thrown away");
        assert!(kept.join("A/Two/cover.jpg").is_file(), "its original stays kept");
        assert!(!kept.join("A/One").exists(), "a restored original leaves no empty folder");
        assert_eq!(last_run(&kept).unwrap(), None, "the run is marked undone");
        assert!(undo_last(&kept).unwrap().is_none());
    }

    #[test]
    fn a_second_run_is_undone_before_the_first() {
        let base = tmpdir("two-runs");
        let (lib, kept) = (base.join("library"), base.join("originals"));
        let a = lib.join("A/One/cover.jpg");
        let b = lib.join("A/Two/cover.jpg");
        jpeg(&a, 600, 538, 0xff0000ff);
        jpeg(&b, 600, 538, 0x00ff00ff);
        let first = WriteBack::start(&kept).unwrap();
        first.one(&a, &rel_under(&a, &lib).unwrap(), square(), false).unwrap();
        let second = WriteBack::start(&kept).unwrap();
        assert_ne!(first.run, second.run, "two runs in one second still get two names");
        second.one(&b, &rel_under(&b, &lib).unwrap(), square(), false).unwrap();

        assert_eq!(undo_last(&kept).unwrap().unwrap().run, second.run);
        assert_eq!(image_dims(&b), Some((600, 538)));
        assert_eq!(image_dims(&a), Some((538, 538)), "the first run is still in place");
        assert_eq!(undo_last(&kept).unwrap().unwrap().run, first.run);
        assert_eq!(image_dims(&a), Some((600, 538)));
    }

    #[test]
    fn the_originals_folder_may_not_be_the_library_inside_it_or_above_it() {
        let base = tmpdir("guard");
        let lib = base.join("library");
        fs::create_dir_all(lib.join("kept")).unwrap();
        assert!(guard_originals(&lib, &lib).is_err());
        assert!(guard_originals(&lib, &lib.join("kept")).is_err());
        assert!(guard_originals(&lib, &base).is_err());
        let beside = base.join("originals");
        fs::create_dir_all(&beside).unwrap();
        assert!(guard_originals(&lib, &beside).is_ok());
    }
}
