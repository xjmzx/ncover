use super::*;

// ---------- the vector backend ----------
//
// The same operation list as the raster fold, written as SVG instead of
// pixels (the macOS sibling's VectorRender, ported). Rasterising an SVG in
// order to write an SVG is pure loss, so where the source is vector (an SVG
// file, or a Blank canvas) and every step has a vector form, the output is
// too. Where a step has none, `vector_refusal` says which, for a person, and
// Save as SVG stays off rather than writing a picture that does not match the
// screen.

/// A number as SVG wants it: at most three decimals, no trailing zeros.
fn f(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

impl Mask {
    /// The clip shape, placed in its region (which is what lets a mask be
    /// fitted to the artwork).
    fn svg_clip_shape(self, reg: MaskRegion) -> String {
        let s = reg.side;
        let (ox, oy) = (reg.cx - s / 2.0, reg.cy - s / 2.0);
        match self {
            Mask::Disc => format!(r#"<circle cx="{}" cy="{}" r="{}"/>"#, f(reg.cx), f(reg.cy), f(s / 2.0)),
            Mask::Rounded(frac) => {
                let r = frac.clamp(0.0, 1.0) * s / 2.0;
                format!(
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" ry="{}"/>"#,
                    f(ox), f(oy), f(s), f(s), f(r), f(r)
                )
            }
            Mask::Chamfer(frac) => {
                let i = frac.clamp(0.0, 1.0) * s / 2.0;
                let pts = [(i, 0.0), (s - i, 0.0), (s, i), (s, s - i), (s - i, s), (i, s), (0.0, s - i), (0.0, i)]
                    .iter()
                    .map(|(x, y)| format!("{},{}", f(ox + x), f(oy + y)))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(r#"<polygon points="{pts}"/>"#)
            }
        }
    }

    /// The INVERSE of the clip shape, as an even-odd path: the canvas with the
    /// shape cut out. The outer fill must appear only outside the shape; a
    /// plain rect behind the artwork would also show through every transparent
    /// pixel inside it, which is a different picture.
    fn svg_inverse_clip(self, c: f64, reg: MaskRegion) -> String {
        let s = reg.side;
        let (ox, oy) = (reg.cx - s / 2.0, reg.cy - s / 2.0);
        let outer = format!("M0,0 H{} V{} H0 Z ", f(c), f(c));
        let inner = match self {
            Mask::Disc => {
                let r = s / 2.0;
                format!(
                    "M{},{} A{},{} 0 1,0 {},{} A{},{} 0 1,0 {},{} Z",
                    f(reg.cx), f(reg.cy - r), f(r), f(r), f(reg.cx), f(reg.cy + r),
                    f(r), f(r), f(reg.cx), f(reg.cy - r)
                )
            }
            Mask::Rounded(frac) => {
                let r = frac.clamp(0.0, 1.0) * s / 2.0;
                format!(
                    "M{},{} H{} A{r},{r} 0 0,1 {},{} V{} A{r},{r} 0 0,1 {},{} H{} A{r},{r} 0 0,1 {},{} V{} A{r},{r} 0 0,1 {},{} Z",
                    f(ox + r), f(oy), f(ox + s - r), f(ox + s), f(oy + r), f(oy + s - r),
                    f(ox + s - r), f(oy + s), f(ox + r), f(ox), f(oy + s - r), f(oy + r), f(ox + r), f(oy),
                    r = f(r)
                )
            }
            Mask::Chamfer(frac) => {
                let i = frac.clamp(0.0, 1.0) * s / 2.0;
                format!(
                    "M{},{} H{} L{},{} V{} L{},{} H{} L{},{} V{} Z",
                    f(ox + i), f(oy), f(ox + s - i), f(ox + s), f(oy + i), f(oy + s - i),
                    f(ox + s - i), f(oy + s), f(ox + i), f(ox), f(oy + s - i), f(oy + i)
                )
            }
        };
        format!(r#"<path clip-rule="evenodd" d="{outer}{inner}"/>"#)
    }
}

/// Distance from the region's centre to the furthest canvas pixel centre: the
/// same quantity the raster side normalises its gradient by, so both ramps
/// reach the outer stop in the same place.
fn gradient_outer_radius(c: f64, reg: MaskRegion) -> f64 {
    [(0.5, 0.5), (c - 0.5, 0.5), (0.5, c - 0.5), (c - 0.5, c - 0.5)]
        .iter()
        .map(|(x, y)| ((x - reg.cx).powi(2) + (y - reg.cy).powi(2)).sqrt())
        .fold(c / 2.0, f64::max)
}

impl Operation {
    /// Whether this step can be written as SVG — honestly. The one refusal:
    /// a gradient fill on a rounded or chamfered mask. An SVG radial gradient
    /// follows a circle; the raster ramp follows the shape's distance field,
    /// and the two would not match.
    pub(crate) fn has_vector_form(&self) -> bool {
        !matches!(self, Operation::Mask { shape, fill: OuterFill::Gradient { .. }, .. } if *shape != Mask::Disc)
    }
}

impl Composition {
    /// Why this cannot be saved as SVG, for a person; None when it can. A
    /// Template source is never drawn, so it never blocks the export, whatever
    /// its format.
    pub(crate) fn vector_refusal(&self) -> Option<String> {
        if !self.is_template() && self.source.svg.is_none() && !self.source.blank {
            return Some("the source is a bitmap (PNG / JPEG), not an SVG. Use it as a Template to trace over".into());
        }
        self.ops.iter().enumerate().find(|(_, op)| !op.has_vector_form()).map(|(i, op)| {
            format!("step {}, {} with a gradient, has no vector form: a gradient only follows a disc", i + 1, op.label())
        })
    }
}

/// An SVG file's root, re-headed to sit at (x, y, w, h) inside another SVG.
/// Nested `<svg>` is valid and leaves the artwork's own paths untouched. The
/// root's width / height / x / y are replaced; a root with no viewBox gets one
/// from its own width and height, or it would not scale.
pub(crate) fn nested_svg(text: &str, x: f64, y: f64, w: f64, h: f64) -> Result<String> {
    let start = find_root(text).context("no <svg> element in the file")?;
    let tag_end = tag_end(text, start).context("the <svg> tag never closes")?;
    let tag = &text[start + 4..tag_end]; // between "<svg" and ">"
    let (tag, self_closing) = match tag.strip_suffix('/') {
        Some(t) => (t, true),
        None => (tag, false),
    };
    let attrs = attributes(tag);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let mut head = format!(r#"<svg x="{}" y="{}" width="{}" height="{}""#, f(x), f(y), f(w), f(h));
    if get("viewBox").is_none() {
        let num = |v: Option<&str>| v.and_then(|s| s.trim().trim_end_matches("px").parse::<f64>().ok());
        if let (Some(vw), Some(vh)) = (num(get("width")), num(get("height"))) {
            head.push_str(&format!(r#" viewBox="0 0 {} {}""#, f(vw), f(vh)));
        }
    }
    for (n, v) in &attrs {
        if !matches!(n.as_str(), "width" | "height" | "x" | "y") {
            head.push_str(&format!(r#" {n}="{}""#, v.replace('"', "&quot;")));
        }
    }
    head.push_str(if self_closing { "/>" } else { ">" });
    Ok(head + &text[tag_end + 1..])
}

/// Index of the root `<svg`, past the XML declaration, doctype and comments.
fn find_root(text: &str) -> Option<usize> {
    let mut i = 0;
    while let Some(off) = text[i..].find('<') {
        let at = i + off;
        let rest = &text[at..];
        if rest.starts_with("<svg") && rest[4..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            return Some(at);
        }
        i = at + 1;
        if rest.starts_with("<!--") {
            i = at + rest.find("-->")? + 3;
        }
    }
    None
}

/// The `>` that ends the tag starting at `start`, skipping quoted values.
fn tag_end(text: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, ch) in text[start..].char_indices() {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(start + i),
            _ => {}
        }
    }
    None
}

/// `name="value"` pairs of a tag body, in order.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ns = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = tag[ns..i].to_string();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            if !name.is_empty() {
                out.push((name, String::new()));
            }
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
            let q = b[i];
            let vs = i + 1;
            i = vs;
            while i < b.len() && b[i] != q {
                i += 1;
            }
            out.push((name, tag[vs..i.min(b.len())].to_string()));
            i += 1;
        }
    }
    out
}

/// The document as SVG, or why it cannot be (`vector_refusal`).
pub(crate) fn render_vector(doc: &Composition) -> Result<String> {
    if let Some(why) = doc.vector_refusal() {
        anyhow::bail!("{}", why);
    }
    let c = doc.canvas as f64;
    let (sw, sh) = (doc.source.raster.width(), doc.source.raster.height());
    let svg_text = match &doc.source.svg {
        Some(p) if !doc.is_template() => {
            Some(fs::read_to_string(p).with_context(|| format!("could not read {}", p.display()))?)
        }
        _ => None,
    };
    let mut defs: Vec<String> = Vec::new();
    // Built inside-out: each mask, invert or background wraps what came
    // before, as the raster fold applies each step to the canvas so far.
    let mut body = String::new();
    for (i, op) in doc.ops.iter().enumerate() {
        match *op {
            Operation::Place(p) => {
                let (w, h) = (p.w(sw), p.h(sh));
                body = match &svg_text {
                    Some(t) => nested_svg(t, p.dx, p.dy, w, h)?,
                    // Blank: flat white where the source is placed.
                    None => format!(
                        r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#ffffff"/>"##,
                        f(p.dx), f(p.dy), f(w), f(h)
                    ),
                };
            }
            Operation::Template(_) => {} // a guide, never a mark
            Operation::Mask { shape, fill, fit } => {
                // Fitting to the artwork needs to know what is drawn, which
                // only the raster fold can answer: ask it, for the steps
                // before this one. Export is not a hot path.
                let reg = match fit {
                    MaskFit::Canvas => MaskRegion::canvas(doc.canvas),
                    MaskFit::Artwork => {
                        let mut prefix = doc.clone();
                        prefix.ops.truncate(i);
                        drawn_region(&render_raster(&prefix)?).unwrap_or(MaskRegion::canvas(doc.canvas))
                    }
                };
                let clip = format!("mask{i}");
                defs.push(format!(r#"<clipPath id="{clip}">{}</clipPath>"#, shape.svg_clip_shape(reg)));
                let outside = format!("outside{i}");
                let paint = match fill {
                    OuterFill::Alpha => None,
                    OuterFill::White => Some("#ffffff".to_string()),
                    OuterFill::Solid(col) => Some(hex(col)),
                    OuterFill::Gradient { inner, outer } => {
                        let r = gradient_outer_radius(c, reg);
                        let id = format!("grad{i}");
                        defs.push(format!(
                            r#"<radialGradient id="{id}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}"><stop offset="{}" stop-color="{}"/><stop offset="1" stop-color="{}"/></radialGradient>"#,
                            f(reg.cx), f(reg.cy), f(r), f((reg.side / 2.0) / r), hex(inner), hex(outer)
                        ));
                        Some(format!("url(#{id})"))
                    }
                };
                let layer = match paint {
                    None => String::new(),
                    Some(p) => {
                        defs.push(format!(r#"<clipPath id="{outside}">{}</clipPath>"#, shape.svg_inverse_clip(c, reg)));
                        format!(
                            r#"<g clip-path="url(#{outside})"><rect width="{}" height="{}" fill="{p}"/></g>"#,
                            f(c), f(c)
                        )
                    }
                };
                body = format!(r#"{layer}<g clip-path="url(#{clip})">{body}</g>"#);
            }
            Operation::Invert => {
                // RGB inverted, alpha untouched, in sRGB (the filter default is
                // linearRGB, which would invert to different values).
                let id = format!("invert{i}");
                defs.push(format!(
                    r#"<filter id="{id}" color-interpolation-filters="sRGB"><feColorMatrix type="matrix" values="-1 0 0 0 1  0 -1 0 0 1  0 0 -1 0 1  0 0 0 1 0"/></filter>"#
                ));
                body = format!(r#"<g filter="url(#{id})">{body}</g>"#);
            }
            Operation::Square { frac, colour } => {
                // Drawn ON what came before, with the raster stamp's rounding.
                let side = (frac.clamp(0.0, 1.0) * c).round();
                let o = ((c - side) / 2.0).floor();
                body.push_str(&format!(
                    r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                    f(o), f(o), f(side), f(side), hex(colour)
                ));
            }
            Operation::Background(col) => {
                // UNDER what is drawn: earlier in document order.
                body = format!(r#"<rect width="{}" height="{}" fill="{}"/>{body}"#, f(c), f(c), hex(col));
            }
        }
    }
    let defs = if defs.is_empty() { String::new() } else { format!("\n  <defs>{}</defs>", defs.concat()) };
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{c}\" height=\"{c}\" viewBox=\"0 0 {c} {c}\">{defs}\n  {body}\n</svg>\n",
        c = f(c)
    ))
}

#[cfg(test)]
mod vector_tests {
    use super::*;

    /// A fresh folder per call: the tests run in parallel.
    fn tmp(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("ncover-vector-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const LOGO: &str = r##"<?xml version="1.0"?>
<!-- a test logo -->
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="10" width="80" height="80" fill="#2a6fdb"/><circle cx="50" cy="50" r="20" fill="#f0c020"/></svg>"##;

    fn doc_from(text: &str, canvas: i32) -> Composition {
        let dir = tmp(&format!("src-{canvas}-{}", text.len()));
        let path = dir.join("logo.svg");
        fs::write(&path, text).unwrap();
        let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 1024, 1024, true).unwrap();
        let mut d = Composition::new(pb, canvas);
        d.source.svg = Some(path);
        d
    }

    /// Rasterise our SVG with librsvg, the renderer that reads it back, and
    /// compare it with the raster fold: mean channel difference, and the
    /// share of pixels further apart than a rim's antialiasing.
    fn agreement(doc: &Composition) -> (f64, f64) {
        let svg = render_vector(doc).unwrap();
        let dir = tmp(&format!("agree-{}", svg.len()));
        let path = dir.join("out.svg");
        fs::write(&path, &svg).unwrap();
        let c = doc.canvas;
        let back = gdk_pixbuf::Pixbuf::from_file_at_scale(&path, c, c, false).unwrap();
        let back = if back.has_alpha() { back } else { back.add_alpha(false, 0, 0, 0).unwrap() };
        let mine = render_raster(doc).unwrap();
        let (a, b) = (unsafe { back.pixels() }, unsafe { mine.pixels() });
        let (ra, rb) = (back.rowstride() as usize, mine.rowstride() as usize);
        let (mut sum, mut far, mut n) = (0f64, 0usize, 0usize);
        for y in 0..c as usize {
            for x in 0..c as usize {
                let mut worst = 0i32;
                for k in 0..4 {
                    let d = (a[y * ra + x * 4 + k] as i32 - b[y * rb + x * 4 + k] as i32).abs();
                    sum += d as f64;
                    worst = worst.max(d);
                }
                if worst > 48 {
                    far += 1;
                }
                n += 1;
            }
        }
        (sum / (n * 4) as f64, far as f64 / n as f64)
    }

    #[test]
    fn the_root_is_reheaded_and_the_artwork_left_alone() {
        let out = nested_svg(LOGO, 5.0, 6.5, 200.0, 200.0).unwrap();
        assert!(out.starts_with(r#"<svg x="5" y="6.5" width="200" height="200" viewBox="0 0 100 100" xmlns="#), "{out}");
        assert!(out.contains(r##"<rect x="10" y="10" width="80" height="80" fill="#2a6fdb"/>"##));
        assert!(!out.contains("<?xml"));
    }

    #[test]
    fn a_vector_source_with_every_kind_of_step_matches_the_raster_side() {
        let mut d = doc_from(LOGO, 200);
        d.set_placement(Placement::fit_at(1024, 1024, 200, 0.75));
        d.ops.push(Operation::Square { frac: 0.2, colour: Rgb { r: 0, g: 0, b: 0 } });
        d.ops.push(Operation::Invert);
        d.ops.push(Operation::Mask {
            shape: Mask::Disc,
            fill: OuterFill::Gradient { inner: Rgb { r: 200, g: 20, b: 20 }, outer: Rgb { r: 20, g: 20, b: 200 } },
            fit: MaskFit::Canvas,
        });
        let (mean, far) = agreement(&d);
        assert!(mean < 3.0 && far < 0.02, "mean {mean:.2}, far {:.2}%", far * 100.0);
    }

    #[test]
    fn every_shape_and_fit_agrees_and_a_background_goes_under() {
        for (shape, fit) in [
            (Mask::Rounded(0.4), MaskFit::Canvas),
            (Mask::Chamfer(0.5), MaskFit::Canvas),
            (Mask::Disc, MaskFit::Artwork),
        ] {
            let mut d = doc_from(LOGO, 200);
            d.set_placement(Placement::fit_at(1024, 1024, 200, 0.5));
            d.ops.push(Operation::Mask { shape, fill: OuterFill::Solid(Rgb { r: 30, g: 160, b: 90 }), fit });
            d.ops.push(Operation::Background(Rgb { r: 255, g: 255, b: 255 }));
            let (mean, far) = agreement(&d);
            assert!(mean < 3.0 && far < 0.02, "{shape:?} {fit:?}: mean {mean:.2}, far {:.2}%", far * 100.0);
        }
    }

    #[test]
    fn a_blank_canvas_is_a_vector_source() {
        let mut d = Composition::new(blank_canvas(1000).unwrap(), 200);
        d.source.blank = true;
        d.ops.push(Operation::Square { frac: 0.5, colour: Rgb { r: 0, g: 0, b: 0 } });
        d.ops.push(Operation::Mask { shape: Mask::Disc, fill: OuterFill::Alpha, fit: MaskFit::Canvas });
        assert_eq!(d.vector_refusal(), None);
        let (mean, far) = agreement(&d);
        assert!(mean < 3.0 && far < 0.02, "mean {mean:.2}, far {:.2}%", far * 100.0);
    }

    #[test]
    fn refusals_say_which_step_and_why() {
        let bitmap = Composition::new(blank_canvas(100).unwrap(), 100);
        assert!(bitmap.vector_refusal().unwrap().contains("bitmap"));
        let mut traced = bitmap.clone();
        traced.set_template(true);
        traced.ops.push(Operation::Square { frac: 0.5, colour: Rgb { r: 0, g: 0, b: 0 } });
        assert_eq!(traced.vector_refusal(), None, "a template is never drawn, so its format does not matter");
        let svg = render_vector(&traced).unwrap();
        assert!(!svg.contains("<svg x="), "the template is not in the file");
        assert!(svg.contains("<rect x=\"25\" y=\"25\" width=\"50\" height=\"50\""));

        let mut d = doc_from(LOGO, 100);
        d.ops.push(Operation::Mask {
            shape: Mask::Rounded(0.3),
            fill: OuterFill::Gradient { inner: Rgb { r: 0, g: 0, b: 0 }, outer: Rgb { r: 255, g: 255, b: 255 } },
            fit: MaskFit::Canvas,
        });
        assert_eq!(
            d.vector_refusal().as_deref(),
            Some("step 2, Rounded with a gradient, has no vector form: a gradient only follows a disc")
        );
        assert!(render_vector(&d).is_err());
    }
}
