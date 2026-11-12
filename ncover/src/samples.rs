use super::*;

// ---------- samples ----------
//
// Four demo files, each one there to show a DIFFERENT feature — not decoration.
// Generated at runtime rather than shipped as binary assets: the SVGs are
// strings, the PNGs are a loop, and nothing has to be vendored or kept in step
// with the code.
//
// Seeded ONCE (see AppData::seeded). Delete one and it stays deleted — a demo
// file that keeps coming back is not a demo, it is litter. Only a fresh install
// (no data.json) seeds again, and "Restore samples" is there for a deliberate
// second chance.


pub(crate) fn samples_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ncover/samples")
}

/// Simple vector shapes with NAMED, declared fills — the file to open to see
/// what SVG introspection actually reads: exact colours with use counts, not a
/// guess from pixels.
pub(crate) const SVG_SHAPES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400" height="400">
  <rect width="400" height="400" fill="#131D2A"/>
  <circle cx="120" cy="120" r="70" fill="#7AF0CD"/>
  <rect x="220" y="50" width="140" height="140" rx="12" fill="#A78BFA"/>
  <polygon points="120,360 50,240 190,240" fill="#7DD3FC"/>
  <path d="M290 240 l60 0 l-30 120 z" fill="#FBBF24" stroke="#F87171" stroke-width="4"/>
  <circle cx="200" cy="200" r="26" fill="none" stroke="#4ADE80" stroke-width="8"/>
</svg>
"##;

/// A disc label: the SVG whose FONTS and TEXT the Inspect panel pulls out. For
/// label art that is most of what you want to know before touching anything.
pub(crate) const SVG_LABEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400" height="400">
  <defs>
    <radialGradient id="face" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#E0BBFF"/>
      <stop offset="100%" stop-color="#6F00CA"/>
    </radialGradient>
  </defs>
  <circle cx="200" cy="200" r="196" fill="url(#face)"/>
  <circle cx="200" cy="200" r="26" fill="#130023"/>
  <text x="200" y="120" font-family="Helvetica" font-size="34" font-weight="bold"
        text-anchor="middle" fill="#130023">SIDE A</text>
  <text x="200" y="300" font-family="Helvetica" font-size="18"
        text-anchor="middle" fill="#130023">45 RPM  ·  STEREO</text>
</svg>
"##;

/// A raster of flat blocks — open this and hit "Palette from image" to see the
/// quantiser return exactly the colours that are in it.
pub(crate) fn png_swatches() -> Result<gdk_pixbuf::Pixbuf> {
    let cols: [(u8, u8, u8); 8] = [
        (122, 240, 205),
        (125, 211, 252),
        (167, 139, 250),
        (74, 222, 128),
        (251, 191, 36),
        (248, 113, 113),
        (178, 96, 58),
        (19, 29, 42),
    ];
    let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 400, 200)
        .context("alloc")?;
    let row = pb.rowstride() as usize;
    let b = unsafe { pb.pixels() };
    for y in 0..200usize {
        for x in 0..400usize {
            let (r, g, bl) = cols[(x * 8 / 400).min(7)];
            let i = y * row + x * 4;
            b[i] = r;
            b[i + 1] = g;
            b[i + 2] = bl;
            b[i + 3] = 255;
        }
    }
    Ok(pb)
}

/// A deliberately NON-SQUARE, larger-than-canvas raster — the file that makes
/// the point of the 400x400 canvas: it has to be placed, and the disc cuts what
/// you framed rather than what the file happens to contain.
pub(crate) fn png_artwork() -> Result<gdk_pixbuf::Pixbuf> {
    let (w, h) = (900usize, 600usize);
    let pb =
        gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, w as i32, h as i32)
            .context("alloc")?;
    let row = pb.rowstride() as usize;
    let b = unsafe { pb.pixels() };
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 / w as f64;
            let fy = y as f64 / h as f64;
            // A diagonal sweep with a bright off-centre bloom, so panning and
            // zooming visibly change what lands inside the disc.
            let d = ((fx - 0.32).powi(2) + (fy - 0.4).powi(2)).sqrt();
            let bloom = (1.0 - (d * 2.4)).clamp(0.0, 1.0).powi(2);
            let i = y * row + x * 4;
            b[i] = (30.0 + 200.0 * fx + 25.0 * bloom).min(255.0) as u8;
            b[i + 1] = (20.0 + 90.0 * (1.0 - fy) + 200.0 * bloom).min(255.0) as u8;
            b[i + 2] = (60.0 + 170.0 * fy + 60.0 * bloom).min(255.0) as u8;
            b[i + 3] = 255;
        }
    }
    Ok(pb)
}

/// Write any sample that is not already there. Returns how many were written —
/// existing files are never overwritten, so an edited sample survives.
pub(crate) fn write_samples() -> Result<usize> {
    write_samples_into(&samples_dir())
}

pub(crate) fn write_samples_into(dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let mut n = 0;

    for (name, body) in [("shapes.svg", SVG_SHAPES), ("disc-label.svg", SVG_LABEL)] {
        let p = dir.join(name);
        if !p.exists() {
            fs::write(&p, body).with_context(|| format!("write {}", p.display()))?;
            n += 1;
        }
    }
    for (name, make) in [
        ("swatches.png", png_swatches as fn() -> Result<gdk_pixbuf::Pixbuf>),
        ("artwork.png", png_artwork),
    ] {
        let p = dir.join(name);
        if !p.exists() {
            make()?
                .savev(&p, "png", &[])
                .with_context(|| format!("write {}", p.display()))?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod sample_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("xcolor-samples-{name}"));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn seeds_all_four_demos() {
        let d = tmp("all");
        assert_eq!(write_samples_into(&d).unwrap(), 4);
        for f in ["shapes.svg", "disc-label.svg", "swatches.png", "artwork.png"] {
            assert!(d.join(f).exists(), "{f} missing");
        }
        // The shapes SVG must actually parse and declare the colours the tips
        // promise — a demo that does not demo the feature is worse than none.
        let info = inspect_svg(&fs::read_to_string(d.join("shapes.svg")).unwrap()).unwrap();
        assert!(info.colors.len() >= 6);

        // The label SVG must carry the fonts and text the Inspect panel reads.
        let l = inspect_svg(&fs::read_to_string(d.join("disc-label.svg")).unwrap()).unwrap();
        assert_eq!(l.fonts, vec!["Helvetica"]);
        assert!(l.texts.iter().any(|t| t.contains("SIDE A")));
        assert_eq!(l.gradients, 1);
    }

    #[test]
    fn never_overwrites_an_existing_sample() {
        // Edit a sample and it must survive "Restore". Clobbering the user's
        // file to give them a demo back would be an absurd trade.
        let d = tmp("keep");
        write_samples_into(&d).unwrap();
        fs::write(d.join("shapes.svg"), "MINE").unwrap();
        assert_eq!(write_samples_into(&d).unwrap(), 0, "nothing should be rewritten");
        assert_eq!(fs::read_to_string(d.join("shapes.svg")).unwrap(), "MINE");
    }

    #[test]
    fn restore_writes_back_only_what_is_missing() {
        let d = tmp("restore");
        write_samples_into(&d).unwrap();
        fs::remove_file(d.join("artwork.png")).unwrap();
        assert_eq!(write_samples_into(&d).unwrap(), 1);
        assert!(d.join("artwork.png").exists());
    }

    #[test]
    fn the_artwork_demo_is_non_square_and_bigger_than_the_canvas() {
        // Its whole job is to make the point of the canvas: it must NOT fit.
        let d = tmp("shape");
        write_samples_into(&d).unwrap();
        let pb = gdk_pixbuf::Pixbuf::from_file(d.join("artwork.png")).unwrap();
        assert_ne!(pb.width(), pb.height(), "must be non-square");
        assert!(
            pb.width() > CANVAS_DEFAULT && pb.height() > CANVAS_DEFAULT,
            "must overflow the canvas"
        );
    }
}
