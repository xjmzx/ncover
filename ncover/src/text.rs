use super::*;
use gtk::{cairo, pango};

// ---------- type ----------
//
// Words set again in a real font, as an operation. A low-resolution raster of
// lettering cannot be improved; setting the words in the font gets you the
// lettering at any size, and as vector.
//
// **One layout, two backends** (the macOS sibling's TextRender, ported as
// behaviour through Pango / Cairo rather than Core Text). `layout_text` is the
// only place that decides where a line goes; the raster backend draws those
// lines and the vector backend turns the same ones into paths. They agree by
// construction, and `type_svg_matches_the_raster_side` holds them to it.

/// A block of type on the canvas.
///
/// Sizes and positions are **fractions of the canvas**, not points, so type
/// scales with the output the way the framing does. Points would strand the
/// same lettering in the corner of a 1000 px square.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextSpec {
    pub(crate) text: String,
    /// A Pango font description without a size: "Sans Bold", "DejaVu Serif
    /// Italic". Any size in it is ignored; `size` owns that.
    pub(crate) font: String,
    /// The em, as a fraction of the canvas side.
    pub(crate) size: f64,
    pub(crate) colour: Rgb,
    /// Centre of the block, as fractions of the canvas.
    pub(crate) x: f64,
    pub(crate) y: f64,
    /// Letter spacing, as a fraction of the font size.
    pub(crate) tracking: f64,
    /// Line height, as a multiple of the font size.
    pub(crate) line_spacing: f64,
}

impl Default for TextSpec {
    fn default() -> TextSpec {
        TextSpec {
            text: String::new(),
            font: "Sans Bold".into(),
            size: 0.2,
            colour: Rgb { r: 0, g: 0, b: 0 },
            x: 0.5,
            y: 0.5,
            tracking: 0.0,
            line_spacing: 1.2,
        }
    }
}

impl TextSpec {
    /// A short form for a step list or an error.
    pub(crate) fn label(&self) -> String {
        let flat = self.text.replace('\n', " ");
        let flat = flat.trim();
        let shown: String = if flat.chars().count() > 14 {
            flat.chars().take(13).collect::<String>() + "\u{2026}"
        } else {
            flat.to_string()
        };
        if shown.is_empty() { "Text".into() } else { format!("Text \u{201c}{shown}\u{201d}") }
    }
}

// ---------- the size control ----------
//
// The useful range is a hundredfold — a 2% credit line to a letterform at 200%
// bleeding off every edge — and on a linear slider the first tenth of the
// travel covers everything from invisible to a caption. So: logarithmic.

/// Smallest and largest type, as fractions of the canvas side. The top end is
/// past 1.0 on purpose: a letter larger than the square, cropped by it, is a
/// deliberate thing to draw.
pub(crate) const TEXT_SIZE_MIN: f64 = 0.02;
pub(crate) const TEXT_SIZE_MAX: f64 = 2.0;

/// Where a size sits on the slider, 0…1. Equal travel is equal ratio, and the
/// 0.2 default lands halfway — the check that the range is centred on the
/// sizes people set.
pub(crate) fn text_size_slider_position(size: f64) -> f64 {
    let v = size.clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX);
    (v / TEXT_SIZE_MIN).ln() / (TEXT_SIZE_MAX / TEXT_SIZE_MIN).ln()
}

pub(crate) fn text_size_from_slider(t: f64) -> f64 {
    TEXT_SIZE_MIN * (TEXT_SIZE_MAX / TEXT_SIZE_MIN).powf(t.clamp(0.0, 1.0))
}

/// Tracking's range: tight enough for letters to touch, as a monogram does.
pub(crate) const TEXT_TRACKING_MIN: f64 = -0.25;
pub(crate) const TEXT_TRACKING_MAX: f64 = 0.75;

// ---------- the one layout ----------

/// One line, placed: its Pango line and the origin of its baseline, in canvas
/// coordinates (y down).
pub(crate) struct PlacedLine {
    pub(crate) line: pango::LayoutLine,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

pub(crate) struct TextLayout {
    pub(crate) lines: Vec<PlacedLine>,
    /// The block's bounds (x, y, w, h). Read by the tests, which hold the
    /// layout to "centred" and "scales with the canvas".
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) bounds: (f64, f64, f64, f64),
    // The Layouts own their lines; keep them alive with the lines.
    _layouts: Vec<pango::Layout>,
}

/// A Pango context set up the same way for every render: greyscale
/// antialiasing (subpixel AA bakes a background colour into the edges, and
/// this layer's background is nothing), and no hinting of metrics or
/// positions — hinting snaps advances to whole pixels, so a size would stop
/// scaling linearly with the canvas and the outlines in the SVG would sit
/// fractionally off the pixels on screen.
fn text_context() -> pango::Context {
    let fm = pangocairo::FontMap::default();
    let ctx = fm.create_context();
    if let Ok(mut opts) = cairo::FontOptions::new() {
        opts.set_antialias(cairo::Antialias::Gray);
        opts.set_hint_style(cairo::HintStyle::None);
        opts.set_hint_metrics(cairo::HintMetrics::Off);
        pangocairo::functions::context_set_font_options(&ctx, Some(&opts));
    }
    ctx.set_round_glyph_positions(false);
    ctx
}

fn description(spec: &TextSpec, px: f64) -> pango::FontDescription {
    let mut desc = pango::FontDescription::from_string(&spec.font);
    desc.set_absolute_size(px * pango::SCALE as f64);
    desc
}

/// Generic names fontconfig resolves to a real face on purpose; asking for
/// one and getting a face is not a substitution.
const GENERIC_FAMILIES: [&str; 6] = ["sans", "sans-serif", "serif", "monospace", "system-ui", "cursive"];

/// Whether the asked-for family is the one that will be used, or fontconfig is
/// quietly substituting another. Pango never fails to give you *a* font, so
/// asking is the only way to know. It matters less than elsewhere — the glyphs
/// become paths on the way out — but "that is not the font I asked for" is
/// worth being able to answer.
pub(crate) fn font_is_available(font: &str) -> bool {
    let want = pango::FontDescription::from_string(font);
    let Some(family) = want.family() else { return true };
    let family = family.to_string();
    if family.split(',').all(|f| GENERIC_FAMILIES.contains(&f.trim().to_ascii_lowercase().as_str())) {
        return true;
    }
    let ctx = text_context();
    let Some(fm) = ctx.font_map() else { return false };
    let Some(got) = fm.load_font(&ctx, &want) else { return false };
    let got = got.describe().family().map(|f| f.to_string()).unwrap_or_default();
    family.split(',').any(|f| f.trim().eq_ignore_ascii_case(&got))
}

/// Where every line goes. None for empty text or a zero canvas — nothing to
/// place, which is not an error.
pub(crate) fn layout_text(spec: &TextSpec, canvas: i32) -> Option<TextLayout> {
    let side = canvas as f64;
    if side <= 0.0 || spec.text.trim().is_empty() {
        return None;
    }
    let px = (spec.size * side).max(1.0);
    let ctx = text_context();
    let desc = description(spec, px);
    let attrs = pango::AttrList::new();
    if spec.tracking != 0.0 {
        attrs.insert(pango::AttrInt::new_letter_spacing((spec.tracking * px * pango::SCALE as f64).round() as i32));
    }

    // Split on newlines rather than wrapping: label art is a line or two set
    // deliberately, and a box that reflows is a different tool.
    struct Measured {
        layout: pango::Layout,
        x: f64,
        width: f64,
        ascent: f64,
        descent: f64,
    }
    let s = pango::SCALE as f64;
    let measured: Vec<Measured> = spec
        .text
        .split('\n')
        .map(|t| {
            let layout = pango::Layout::new(&ctx);
            layout.set_font_description(Some(&desc));
            layout.set_attributes(Some(&attrs));
            layout.set_text(t);
            // One line per layout, so the layout's logical box is the line's:
            // its top is the line's ascent above the baseline.
            let (_, logical) = layout.extents();
            let ascent = layout.baseline() as f64 / s - logical.y() as f64 / s;
            Measured {
                layout,
                x: logical.x() as f64 / s,
                width: logical.width() as f64 / s,
                ascent,
                descent: logical.height() as f64 / s - ascent,
            }
        })
        .collect();

    let line_height = px * spec.line_spacing;
    // First ascent to last descent: measured any other way, a single line
    // sits off-centre by the font's own leading.
    let first_ascent = measured.first().map_or(0.0, |m| m.ascent);
    let block_h = first_ascent + line_height * (measured.len() - 1) as f64 + measured.last().map_or(0.0, |m| m.descent);
    let top = spec.y * side - block_h / 2.0;
    let centre_x = spec.x * side;

    let mut lines = Vec::new();
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    for (i, m) in measured.iter().enumerate() {
        let Some(line) = m.layout.line(0) else { continue };
        if m.width <= 0.0 {
            continue; // an empty line still takes its height, and draws nothing
        }
        let left = centre_x - m.width / 2.0;
        min_x = min_x.min(left);
        max_x = max_x.max(left + m.width);
        lines.push(PlacedLine { line, x: left - m.x, y: top + first_ascent + line_height * i as f64 });
    }
    if lines.is_empty() {
        return None;
    }
    Some(TextLayout {
        lines,
        bounds: (min_x, top, max_x - min_x, block_h),
        _layouts: measured.into_iter().map(|m| m.layout).collect(),
    })
}

// ---------- the raster backend ----------

/// Draw type onto a copy of `base`.
///
/// Glyphs go through Cairo: type must be antialiased, and a text renderer is
/// what does that. What comes back is **coverage only** — the lines are drawn
/// opaque onto transparency, and the alpha of that surface is the coverage —
/// so the colour is the spec's exactly, never a premultiplied value divided
/// back out, and it is composited over the base in straight alpha. An edge that
/// passes through a disc mask afterwards does not darken.
pub(crate) fn draw_text(base: &gdk_pixbuf::Pixbuf, spec: &TextSpec) -> Result<gdk_pixbuf::Pixbuf> {
    let canvas = base.width().min(base.height());
    let pb = base.copy().context("could not copy the canvas")?;
    let Some(layout) = layout_text(spec, canvas) else { return Ok(pb) };
    let (w, h) = (base.width(), base.height());
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).context("could not allocate the type layer")?;
    {
        let cr = cairo::Context::new(&surface).context("could not draw the type")?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        for l in &layout.lines {
            cr.move_to(l.x, l.y);
            pangocairo::functions::show_layout_line(&cr, &l.line);
        }
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().context("could not read the type layer")?;
    let (n, rs) = (pb.n_channels() as usize, pb.rowstride() as usize);
    let bytes = unsafe { pb.pixels() };
    let col = [spec.colour.r, spec.colour.g, spec.colour.b];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let o = y * stride + x * 4;
            // ARGB32 is a native-endian u32 with alpha in the top byte.
            let ta = u32::from_ne_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) >> 24;
            if ta == 0 {
                continue;
            }
            let i = y * rs + x * n;
            let ba = if n == 4 { bytes[i + 3] as u32 } else { 255 };
            if ta == 255 || ba == 0 {
                bytes[i..i + 3].copy_from_slice(&col);
                if n == 4 {
                    bytes[i + 3] = ta as u8;
                }
                continue;
            }
            // Source-over in straight alpha, in units of 255 × 255.
            let a = ta * 255 + ba * (255 - ta);
            for k in 0..3 {
                let v = (col[k] as u32 * ta * 255 + bytes[i + k] as u32 * ba * (255 - ta) + a / 2) / a;
                bytes[i + k] = v.min(255) as u8;
            }
            if n == 4 {
                bytes[i + 3] = ((a + 127) / 255).min(255) as u8;
            }
        }
    }
    Ok(pb)
}

// ---------- the vector backend ----------

/// The type as one SVG `<path>` of outlines, never `<text font-family>`.
///
/// A font reference is smaller and stays editable, and it renders as a
/// different picture — or not at all — on a machine without the font. Paths
/// always match what was on screen. Non-zero winding, because that is what a
/// glyph's counters are wound for; even-odd would also punch holes wherever
/// two letters overlap.
pub(crate) fn text_svg_path(spec: &TextSpec, canvas: i32) -> Result<Option<String>> {
    let Some(layout) = layout_text(spec, canvas) else { return Ok(None) };
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).context("could not allocate")?;
    let cr = cairo::Context::new(&surface).context("could not trace the type")?;
    for l in &layout.lines {
        cr.move_to(l.x, l.y);
        pangocairo::functions::layout_line_path(&cr, &l.line);
    }
    let path = cr.copy_path().context("could not read the outlines")?;
    let mut d = String::new();
    let n = |v: f64| {
        let s = format!("{v:.3}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" { "0".to_string() } else { s.to_string() }
    };
    for seg in path.iter() {
        match seg {
            cairo::PathSegment::MoveTo((x, y)) => d += &format!("M{},{}", n(x), n(y)),
            cairo::PathSegment::LineTo((x, y)) => d += &format!("L{},{}", n(x), n(y)),
            cairo::PathSegment::CurveTo((a, b), (c, e), (x, y)) => {
                d += &format!("C{},{} {},{} {},{}", n(a), n(b), n(c), n(e), n(x), n(y))
            }
            cairo::PathSegment::ClosePath => d.push('Z'),
        }
    }
    if d.is_empty() {
        return Ok(None);
    }
    let c = spec.colour;
    Ok(Some(format!(
        r##"<path fill-rule="nonzero" fill="#{:02x}{:02x}{:02x}" d="{d}"/>"##,
        c.r, c.g, c.b
    )))
}

#[cfg(test)]
mod text_tests {
    use super::*;

    // Ported from the macOS sibling's TextTests, keeping their intent and
    // names. Their oracle is Core Text; ours is Pango, so these assert
    // behaviour (centred, scales, widens, stacks), not glyph positions.

    fn spec(s: &str, size: f64) -> TextSpec {
        TextSpec { text: s.into(), font: "DejaVu Sans".into(), size, ..TextSpec::default() }
    }

    fn white(size: i32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size).unwrap();
        pb.fill(0xffffffff);
        pb
    }

    fn clear(size: i32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size).unwrap();
        pb.fill(0);
        pb
    }

    fn px(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32) -> (u8, u8, u8, u8) {
        sample(pb, &pb.read_pixel_bytes(), x, y)
    }

    fn mid(l: &TextLayout) -> (f64, f64) {
        let (x, y, w, h) = l.bounds;
        (x + w / 2.0, y + h / 2.0)
    }

    #[test]
    fn layout_centres_the_block_on_the_given_point() {
        let l = layout_text(&spec("EG", 0.4), 400).unwrap();
        let (cx, cy) = mid(&l);
        assert!((cx - 200.0).abs() < 0.001, "x centres on 0.5: {cx}");
        assert!((cy - 200.0).abs() < 0.001, "y centres on 0.5: {cy}");
        assert_eq!(l.lines.len(), 1);
        assert!(l.lines[0].y > l.bounds.1, "the baseline is below the block's top");
    }

    #[test]
    fn size_is_a_fraction_of_the_canvas_so_type_scales_with_it() {
        let small = layout_text(&spec("EG", 0.4), 200).unwrap();
        let large = layout_text(&spec("EG", 0.4), 400).unwrap();
        let r = large.bounds.2 / small.bounds.2;
        assert!((r - 2.0).abs() < 0.01, "twice the canvas, twice the type: {r}");
    }

    #[test]
    fn tracking_widens_the_line_about_its_centre() {
        let tight = layout_text(&spec("EG", 0.4), 400).unwrap();
        let mut wide = spec("EG", 0.4);
        wide.tracking = 0.3;
        let loose = layout_text(&wide, 400).unwrap();
        assert!(loose.bounds.2 > tight.bounds.2);
        assert!((mid(&loose).0 - mid(&tight).0).abs() < 0.001);
    }

    #[test]
    fn a_second_line_stacks_below_the_first() {
        let one = layout_text(&spec("E", 0.4), 400).unwrap();
        let two = layout_text(&spec("E\nG", 0.4), 400).unwrap();
        assert!(two.bounds.3 > one.bounds.3);
        assert!((mid(&two).1 - 200.0).abs() < 0.001, "the block, not the first line, is centred");
        assert_eq!(two.lines.len(), 2);
        assert!(two.lines[1].y > two.lines[0].y);
    }

    #[test]
    fn nothing_to_set_is_not_an_error() {
        assert!(layout_text(&spec("", 0.4), 400).is_none());
        assert!(layout_text(&spec("  \n ", 0.4), 400).is_none());
        assert!(layout_text(&spec("EG", 0.4), 0).is_none());
        let base = white(10);
        let out = draw_text(&base, &spec("", 0.4)).unwrap();
        assert_eq!(out.read_pixel_bytes().to_vec(), base.read_pixel_bytes().to_vec());
        assert!(text_svg_path(&spec("", 0.4), 10).unwrap().is_none());
    }

    #[test]
    fn type_actually_marks_the_canvas() {
        let mut s = spec("\u{2588}\u{2588}\u{2588}\u{2588}", 0.5);
        s.colour = Rgb { r: 255, g: 0, b: 0 };
        let out = draw_text(&white(200), &s).unwrap();
        assert_ne!(px(&out, 100, 100), (255, 255, 255, 255), "something was drawn");
        assert_eq!(px(&out, 2, 2), (255, 255, 255, 255), "the corner is untouched");
    }

    #[test]
    fn type_over_transparency_keeps_its_alpha_rather_than_going_opaque() {
        let out = draw_text(&clear(200), &spec("\u{2588}\u{2588}\u{2588}\u{2588}", 0.5)).unwrap();
        assert_eq!(px(&out, 2, 2).3, 0, "outside the type is still nothing");
        assert!(px(&out, 100, 100).3 > 0, "inside it is not");
    }

    #[test]
    fn the_colour_is_the_colour_asked() {
        let mut s = spec("\u{2588}\u{2588}\u{2588}\u{2588}", 0.5);
        s.colour = Rgb { r: 10, g: 200, b: 30 };
        assert_eq!(px(&draw_text(&white(200), &s).unwrap(), 100, 100), (10, 200, 30, 255));
        // And an antialiased edge over transparency keeps the colour too —
        // straight alpha, not a darkened premultiplied value.
        let out = draw_text(&clear(200), &s).unwrap();
        let bytes = out.read_pixel_bytes();
        let partial: Vec<_> = (0..200)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .map(|(x, y)| sample(&out, &bytes, x, y))
            .filter(|p| p.3 > 0 && p.3 < 255)
            .collect();
        assert!(!partial.is_empty(), "there are antialiased edges");
        assert!(partial.iter().all(|p| (p.0, p.1, p.2) == (10, 200, 30)), "every edge pixel is the colour");
    }

    #[test]
    fn the_vector_form_is_paths_not_a_font_reference() {
        let svg = text_svg_path(&spec("EG", 0.4), 400).unwrap().unwrap();
        assert!(!svg.contains("<text") && !svg.contains("font-family"));
        assert!(svg.starts_with("<path fill-rule=\"nonzero\" fill=\"#000000\" d=\"M"));
    }

    #[test]
    fn the_step_names_the_words() {
        assert_eq!(spec("EG", 0.2).label(), "Text \u{201c}EG\u{201d}");
        assert_eq!(spec("two\nlines", 0.2).label(), "Text \u{201c}two lines\u{201d}");
        assert!(spec("an extremely long title indeed", 0.2).label().chars().count() <= 22);
        assert_eq!(spec("", 0.2).label(), "Text");
    }

    #[test]
    fn a_missing_font_is_reported_rather_than_substituted_silently() {
        assert!(font_is_available("DejaVu Sans Bold"));
        assert!(font_is_available("Sans Bold"), "a generic family is not a substitution");
        assert!(!font_is_available("NoSuchFontExistsHere"));
    }

    #[test]
    fn the_size_slider_is_logarithmic_and_round_trips() {
        for v in [0.02, 0.05, 0.2, 0.75, 1.0, 2.0] {
            assert!((text_size_from_slider(text_size_slider_position(v)) - v).abs() < 1e-9);
        }
        assert_eq!(text_size_slider_position(TEXT_SIZE_MIN), 0.0);
        assert!((text_size_slider_position(TEXT_SIZE_MAX) - 1.0).abs() < 1e-12);
        assert!((text_size_slider_position(0.2) - 0.5).abs() < 0.001, "the default lands halfway");
        let a = text_size_from_slider(0.25) / text_size_from_slider(0.0);
        let b = text_size_from_slider(0.75) / text_size_from_slider(0.5);
        assert!((a - b).abs() < 1e-9, "equal travel is equal ratio");
    }

    #[test]
    fn the_slider_is_clamped_at_both_ends() {
        assert_eq!(text_size_from_slider(-5.0), TEXT_SIZE_MIN);
        assert_eq!(text_size_from_slider(5.0), TEXT_SIZE_MAX);
        assert_eq!(text_size_slider_position(0.0001), 0.0);
        assert!((text_size_slider_position(99.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn type_larger_than_the_canvas_is_cropped_rather_than_refused() {
        let inked = |size: f64| {
            let out = draw_text(&white(200), &spec("E", size)).unwrap();
            assert_eq!((out.width(), out.height()), (200, 200));
            let bytes = out.read_pixel_bytes();
            (0..200).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| sample(&out, &bytes, x, y).0 < 200).count()
        };
        let (big, small) = (inked(TEXT_SIZE_MAX), inked(0.2));
        assert!(small > 0);
        assert!(big > small * 8, "big {big}, small {small}");
        assert!(big < 200 * 200, "cropped by the canvas, not filling it");
    }
}
