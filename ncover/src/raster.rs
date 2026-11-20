use super::*;

// ---------- canvas + placement ----------
//
// The output is a SQUARE canvas of a chosen size. A source image of any
// dimensions is PLACED on it — offset and scale — rather than being cropped to
// fit. So the question stops being "how do we squeeze this in" and becomes
// "where on the canvas does this go", which is the one the user is actually
// asking.

/// The offered output sizes. A short list of round numbers, not a free number
/// entry: an arbitrary 437x437 is not a decision anyone means to make, and every
/// size here has to look right at a glance in the picker.
pub(crate) const CANVAS_SIZES: [i32; 5] = [200, 400, 600, 800, 1000];

/// The one you get until you say otherwise.
pub(crate) const CANVAS_DEFAULT: i32 = 400;

/// A stored size that is not one we offer is not honoured — it would be a canvas
/// with no way to get back to it in the UI.
pub(crate) fn sane_canvas(n: i32) -> i32 {
    if CANVAS_SIZES.contains(&n) {
        n
    } else {
        CANVAS_DEFAULT
    }
}

/// How close (in canvas pixels) an edge or centre must come before it snaps.
/// "Medium": firm enough to catch you, loose enough that you can sit 12px off
/// centre on purpose.
pub(crate) const SNAP: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// Top-left of the scaled image, in canvas coordinates.
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) scale: f64,
}

impl Placement {
    /// Scale to COVER the canvas, centred — the sane opening position: no gaps,
    /// nothing arbitrary cropped off one side.
    pub(crate) fn cover(sw: i32, sh: i32, c: i32) -> Placement {
        let scale = (c as f64 / sw as f64).max(c as f64 / sh as f64);
        Placement {
            dx: (c as f64 - sw as f64 * scale) / 2.0,
            dy: (c as f64 - sh as f64 * scale) / 2.0,
            scale,
        }
    }

    /// Whole image inside the canvas, centred.
    pub(crate) fn fit(sw: i32, sh: i32, c: i32) -> Placement {
        let scale = (c as f64 / sw as f64).min(c as f64 / sh as f64);
        Placement {
            dx: (c as f64 - sw as f64 * scale) / 2.0,
            dy: (c as f64 - sh as f64 * scale) / 2.0,
            scale,
        }
    }

    /// `fit`, shrunk to `k` of its size and still centred: the whole image
    /// at 75% or 50% of the canvas, with a margin round it for a background.
    pub(crate) fn fit_at(sw: i32, sh: i32, c: i32, k: f64) -> Placement {
        let scale = (c as f64 / sw as f64).min(c as f64 / sh as f64) * k;
        Placement {
            dx: (c as f64 - sw as f64 * scale) / 2.0,
            dy: (c as f64 - sh as f64 * scale) / 2.0,
            scale,
        }
    }

    /// Rescale a framing from one canvas to another. Changing the output size
    /// must not re-crop what you already framed — the picture stays where you
    /// put it, the square around it just gets bigger.
    pub(crate) fn rescaled(&self, from: i32, to: i32) -> Placement {
        let k = to as f64 / from as f64;
        Placement {
            dx: self.dx * k,
            dy: self.dy * k,
            scale: self.scale * k,
        }
    }
    pub(crate) fn w(&self, sw: i32) -> f64 {
        sw as f64 * self.scale
    }
    pub(crate) fn h(&self, sh: i32) -> f64 {
        sh as f64 * self.scale
    }
}

/// Snap `p` to the canvas's centre and edges. Each axis is considered
/// independently — you can be snapped horizontally while still free vertically,
/// which is what makes it feel like a guide rather than a magnet.
///
/// Returns the snapped placement plus which guides fired, so the view can SHOW
/// why the image stopped moving. A snap you cannot see is just a bug.
pub(crate) fn snap(p: Placement, sw: i32, sh: i32, canvas: i32) -> (Placement, bool, bool) {
    let (mut p, c) = (p, canvas as f64);
    let (iw, ih) = (p.w(sw), p.h(sh));

    // x: left edge, right edge, centre-to-centre.
    let cands_x = [(0.0, 0.0), (c - iw, c), ((c - iw) / 2.0, c / 2.0)];
    let mut sx = None;
    for (target, _) in cands_x {
        if (p.dx - target).abs() <= SNAP {
            p.dx = target;
            sx = Some(target);
            break;
        }
    }
    let cands_y = [(0.0, 0.0), (c - ih, c), ((c - ih) / 2.0, c / 2.0)];
    let mut sy = None;
    for (target, _) in cands_y {
        if (p.dy - target).abs() <= SNAP {
            p.dy = target;
            sy = Some(target);
            break;
        }
    }
    (p, sx.is_some(), sy.is_some())
}

/// Render the placed source onto the fixed 400x400 canvas. Outside the image is
/// transparent — the disc's corner fill decides what happens there, not this.
pub(crate) fn compose(src: &gdk_pixbuf::Pixbuf, p: Placement, canvas: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let dst = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, canvas, canvas)
        .context("could not allocate the canvas")?;
    dst.fill(0x00000000);

    let (sw, sh) = (src.width(), src.height());
    let sbytes = unsafe { src.pixels() };
    let drow = dst.rowstride() as usize;
    let dbytes = unsafe { dst.pixels() };

    for y in 0..canvas {
        for x in 0..canvas {
            // Canvas pixel -> source pixel (nearest: a colour tool must not
            // invent colours that are in neither neighbour).
            let sx = ((x as f64 + 0.5 - p.dx) / p.scale).floor() as i32;
            let sy = ((y as f64 + 0.5 - p.dy) / p.scale).floor() as i32;
            if sx < 0 || sy < 0 || sx >= sw || sy >= sh {
                continue; // left transparent
            }
            let (r, g, b, a) = sample(src, sbytes, sx, sy);
            let i = y as usize * drow + x as usize * 4;
            dbytes[i] = r;
            dbytes[i + 1] = g;
            dbytes[i + 2] = b;
            dbytes[i + 3] = a;
        }
    }
    Ok(dst)
}

// ---------- build primitives ----------
//
// Two operations for BUILDING an image rather than framing one: invert, and a
// centred solid square. Together with a blank canvas they are a minimal kit for
// label artwork — a black square on white, or (invert) a white one on black —
// and they COMPOSE, each one working on what is already on the canvas.

/// A blank opaque-white square, the ground you build a label on. White because a
/// record label is white far more often than not, and Invert reaches black in
/// one click from here.
pub(crate) fn blank_canvas(size: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size)
        .context("could not allocate the canvas")?;
    pb.fill(0xFFFF_FFFF); // 0xRRGGBBAA — white, fully opaque
    Ok(pb)
}

/// Invert RGB on a COPY, leaving alpha alone. A colour tool inverts the COLOUR,
/// not the shape: a transparent pixel stays transparent, it does not become an
/// opaque black one.
pub(crate) fn invert_rgb(src: &gdk_pixbuf::Pixbuf) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = src.copy().context("could not copy the image")?;
    let (w, h) = (pb.width(), pb.height());
    let n = pb.n_channels() as usize;
    let stride = pb.rowstride() as usize;
    let bytes = unsafe { pb.pixels() };
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * stride + x as usize * n;
            bytes[i] = 255 - bytes[i];
            bytes[i + 1] = 255 - bytes[i + 1];
            bytes[i + 2] = 255 - bytes[i + 2];
            // channel 3 (alpha), if present, is deliberately untouched
        }
    }
    Ok(pb)
}

/// Composite a filled, centred square onto a COPY of `base`. Side = `frac` of the
/// canvas side (the canvas is square), clamped to [0,1]. Opaque — a label mark is
/// solid, not a tint — so it also stamps over transparent ground, turning it into
/// the square's colour there.
pub(crate) fn stamp_square(base: &gdk_pixbuf::Pixbuf, frac: f64, c: Rgb) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = base.copy().context("could not copy the canvas")?;
    let (w, h) = (pb.width(), pb.height());
    let side = (frac.clamp(0.0, 1.0) * w.min(h) as f64).round() as i32;
    if side <= 0 {
        return Ok(pb); // an empty square is a no-op, not an error
    }
    let (x0, y0) = ((w - side) / 2, (h - side) / 2);
    let n = pb.n_channels() as usize;
    let stride = pb.rowstride() as usize;
    let has_a = pb.has_alpha();
    let bytes = unsafe { pb.pixels() };
    for y in y0..(y0 + side).min(h) {
        for x in x0..(x0 + side).min(w) {
            if x < 0 || y < 0 {
                continue;
            }
            let i = y as usize * stride + x as usize * n;
            bytes[i] = c.r;
            bytes[i + 1] = c.g;
            bytes[i + 2] = c.b;
            if has_a {
                bytes[i + 3] = 255;
            }
        }
    }
    Ok(pb)
}

/// Put a solid colour BEHIND everything on the canvas: each pixel is laid
/// over `c` by its own alpha, and comes out opaque. Transparent ground (the
/// bars Fit leaves, a disc's Alpha corners) becomes `c`; opaque pixels are
/// untouched; a soft edge blends into `c` rather than into black.
pub(crate) fn fill_behind(base: &gdk_pixbuf::Pixbuf, c: Rgb) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = base.copy().context("could not copy the canvas")?;
    if !pb.has_alpha() {
        return Ok(pb); // nothing is see-through, so nothing is behind
    }
    let (w, h, n, rs) = (pb.width() as usize, pb.height() as usize, pb.n_channels() as usize, pb.rowstride() as usize);
    let bytes = unsafe { pb.pixels() };
    let bg = [c.r, c.g, c.b];
    for y in 0..h {
        for x in 0..w {
            let i = y * rs + x * n;
            let a = bytes[i + 3] as u32;
            if a == 255 {
                continue;
            }
            for k in 0..3 {
                bytes[i + k] = ((bytes[i + k] as u32 * a + bg[k] as u32 * (255 - a) + 127) / 255) as u8;
            }
            bytes[i + 3] = 255;
        }
    }
    Ok(pb)
}

// ---------- disc template ----------
//
// Mask artwork into a disc — a record label, a CD face — with the corners left
// as one of four things. Done as plain pixel work rather than a cairo round
// trip: the alpha handling is the whole point, and it is easier to be exact
// about it than to un-premultiply someone else's surface.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OuterFill {
    /// The corners are simply not there. The honest default for artwork that
    /// will sit on an unknown background.
    Alpha,
    White,
    Solid(Rgb),
    /// Radial, from the disc's edge outward: `inner` at the rim, `outer` at the
    /// corners.
    Gradient { inner: Rgb, outer: Rgb },
}

pub(crate) fn lerp(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8
}

/// Nearest-neighbour sample. A colour tool must not invent colours, and bilinear
/// would blend two swatches into a third that is in neither.
pub(crate) fn sample(pb: &gdk_pixbuf::Pixbuf, bytes: &[u8], x: i32, y: i32) -> (u8, u8, u8, u8) {
    let x = x.clamp(0, pb.width() - 1) as usize;
    let y = y.clamp(0, pb.height() - 1) as usize;
    let nch = pb.n_channels() as usize;
    let i = y * pb.rowstride() as usize + x * nch;
    if i + nch > bytes.len() {
        return (0, 0, 0, 0);
    }
    (
        bytes[i],
        bytes[i + 1],
        bytes[i + 2],
        if pb.has_alpha() { bytes[i + 3] } else { 255 },
    )
}

/// A mask is a signed distance function, not a shape special case: negative
/// inside, positive outside, in pixels. Coverage and the corner gradient both
/// derive from it, so a new shape is a new distance and nothing else. Ported
/// from the macOS sibling's `Mask.signedDistance`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mask {
    Disc,
    /// Corner radius as a fraction of half the canvas. At 1.0 it IS the disc,
    /// so the slider runs continuously from square to circle.
    Rounded(f64),
    /// Corner cut as a fraction of half the canvas. At 1.0 the octagon has
    /// closed into a diamond.
    Chamfer(f64),
}

impl Mask {
    /// Signed distance from (x, y) to the shape inscribed in a `size` square.
    pub(crate) fn signed_distance(self, x: f64, y: f64, size: f64) -> f64 {
        let c = size / 2.0;
        let (px, py) = (x - c, y - c);
        match self {
            Mask::Disc => (px * px + py * py).sqrt() - c,
            Mask::Rounded(f) => {
                // Shrink the box by the radius, measure to it, subtract the radius.
                let r = f.clamp(0.0, 1.0) * c;
                let qx = px.abs() - (c - r);
                let qy = py.abs() - (c - r);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                outside + qx.max(qy).min(0.0) - r
            }
            Mask::Chamfer(f) => {
                // A box cut by the two diagonals through (c - i, c) and (c, c - i).
                let i = f.clamp(0.0, 1.0) * c;
                let bx = (px.abs() - c).max(py.abs() - c);
                let diagonal = (px.abs() + py.abs() - (2.0 * c - i)) / std::f64::consts::SQRT_2;
                bx.max(diagonal)
            }
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Mask::Disc => "Disc",
            Mask::Rounded(_) => "Rounded",
            Mask::Chamfer(_) => "Chamfer",
        }
    }
}

/// What the mask is sized and centred on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaskFit {
    /// Inscribed in the canvas.
    Canvas,
    /// Inscribed in the square bounding what is drawn at that point: a disc
    /// over an inset logo becomes a disc of the logo.
    Artwork,
}

/// The square a mask is inscribed in: centre and side, in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaskRegion {
    pub(crate) cx: f64,
    pub(crate) cy: f64,
    pub(crate) side: f64,
}

impl MaskRegion {
    pub(crate) fn canvas(size: i32) -> MaskRegion {
        let s = size as f64;
        MaskRegion { cx: s / 2.0, cy: s / 2.0, side: s }
    }
}

/// Alpha at or below this counts as nothing drawn, so an antialiased edge or a
/// faint glow does not stretch "the artwork" to the whole canvas.
pub(crate) const DRAWN_ALPHA_FLOOR: u8 = 8;

/// The square bounding what is drawn, centred on it; its side is the larger
/// dimension, so the mask reaches the artwork's widest extent. None when
/// nothing is drawn. An image without alpha is drawn everywhere.
pub(crate) fn drawn_region(pb: &gdk_pixbuf::Pixbuf) -> Option<MaskRegion> {
    let (w, h) = (pb.width(), pb.height());
    if !pb.has_alpha() {
        return Some(MaskRegion { cx: w as f64 / 2.0, cy: h as f64 / 2.0, side: w.max(h) as f64 });
    }
    let (n, rs) = (pb.n_channels() as usize, pb.rowstride() as usize);
    let px = unsafe { pb.pixels() };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1, -1);
    for y in 0..h {
        for x in 0..w {
            if px[y as usize * rs + x as usize * n + 3] > DRAWN_ALPHA_FLOOR {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    let (bw, bh) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
    Some(MaskRegion { cx: x0 as f64 + bw / 2.0, cy: y0 as f64 + bh / 2.0, side: bw.max(bh) })
}

/// Mask `src` to a shape inscribed in `region` (the whole square by default),
/// filling outside it per `fill`.
///
/// The source is scaled to COVER the square (never letterboxed — a disc with
/// bars through it is not a disc), and the rim is antialiased over one pixel so
/// the edge does not read as a staircase. Every shape goes through here; the
/// disc's output is unchanged from the old disc-only routine.
pub(crate) fn apply_mask(
    src: &gdk_pixbuf::Pixbuf,
    mask: Mask,
    fill: OuterFill,
    region: Option<MaskRegion>,
) -> Result<gdk_pixbuf::Pixbuf> {
    let (sw, sh) = (src.width(), src.height());
    if sw == 0 || sh == 0 {
        anyhow::bail!("empty image");
    }
    let size = sw.max(sh);
    let sbytes = unsafe { src.pixels() };

    let dst = gdk_pixbuf::Pixbuf::new(
        gdk_pixbuf::Colorspace::Rgb,
        true, // always RGBA out: Alpha fill needs it, and the others cost nothing
        8,
        size,
        size,
    )
    .context("could not allocate the output image")?;
    let drow = dst.rowstride() as usize;
    let dbytes = unsafe { dst.pixels() };

    let fsize = size as f64;
    let (cx, cy) = (fsize / 2.0, fsize / 2.0);
    // Cover: the smaller source axis must reach across the square.
    let scale = fsize / sw.min(sh) as f64;
    let r = region.unwrap_or(MaskRegion::canvas(size));
    // The shape in its own frame: "inscribed in a square of side r.side".
    let sd = |x: f64, y: f64| mask.signed_distance(x - r.cx + r.side / 2.0, y - r.cy + r.side / 2.0, r.side);
    // Gradient normalisation: the furthest PIXEL CENTRE, not the abstract
    // corner (a corner pixel's centre is half a pixel in, so normalising to
    // the corner leaves it short of the outer stop: #F1F1F1 for black→white
    // on 64 px). With an off-centre region the corners differ: take the most.
    let sd_max = [(0.5, 0.5), (fsize - 0.5, 0.5), (0.5, fsize - 0.5), (fsize - 0.5, fsize - 0.5)]
        .into_iter()
        .map(|(x, y)| sd(x, y))
        .fold(f64::MIN, f64::max)
        .max(1e-9);

    for y in 0..size {
        for x in 0..size {
            let d = sd(x as f64 + 0.5, y as f64 + 0.5);
            // Coverage: 1 inside, 0 outside, ramped across the last pixel.
            let cov = (0.5 - d).clamp(0.0, 1.0);

            // Source pixel under this destination pixel (cover-scaled, centred).
            let sx = ((x as f64 - cx) / scale + sw as f64 / 2.0).floor() as i32;
            let sy = ((y as f64 - cy) / scale + sh as f64 / 2.0).floor() as i32;
            let (ir, ig, ib, ia) = sample(src, sbytes, sx, sy);

            let (or_, og, ob, oa) = match fill {
                OuterFill::Alpha => (0, 0, 0, 0),
                OuterFill::White => (255, 255, 255, 255),
                OuterFill::Solid(c) => (c.r, c.g, c.b, 255),
                OuterFill::Gradient { inner, outer } => {
                    let t = (d / sd_max).clamp(0.0, 1.0);
                    (lerp(inner.r, outer.r, t), lerp(inner.g, outer.g, t), lerp(inner.b, outer.b, t), 255)
                }
            };

            // Composite the masked artwork over the outer fill by coverage.
            let i = y as usize * drow + x as usize * 4;
            let a_in = ia as f64 / 255.0 * cov;
            let a_out = oa as f64 / 255.0 * (1.0 - cov);
            let a = a_in + a_out;
            if a <= 0.0 {
                dbytes[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mix = |inside: u8, outside: u8| -> u8 {
                (((inside as f64 * a_in) + (outside as f64 * a_out)) / a).round().clamp(0.0, 255.0) as u8
            };
            dbytes[i] = mix(ir, or_);
            dbytes[i + 1] = mix(ig, og);
            dbytes[i + 2] = mix(ib, ob);
            dbytes[i + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(dst)
}

/// The disc, by its old name: Batch and the disc tests use it, and if the
/// generalisation changed the disc by a pixel those tests say so.
pub(crate) fn disc_template(src: &gdk_pixbuf::Pixbuf, fill: OuterFill) -> Result<gdk_pixbuf::Pixbuf> {
    apply_mask(src, Mask::Disc, fill, None)
}

/// Alpha below this is treated as "not really there" — a transparent corner is
/// not a colour the artwork uses, and letting it into the palette would fill it
/// with the checkerboard.
pub(crate) const ALPHA_FLOOR: u8 = 128;

/// Quantisation bucket: 4 bits per channel (16 levels), i.e. 4096 bins. Coarse
/// enough that a gradient collapses into the few tones a human would name,
/// fine enough to keep two similar-but-distinct brand colours apart.
pub(crate) const QUANT_SHIFT: u8 = 4;

/// The image's dominant colours, most-used first. Near-transparent pixels are
/// skipped; the representative for each bin is the MEAN of the pixels in it, not
/// the bin centre, so the result is a colour the image actually contains.
pub(crate) fn dominant_colors(pb: &gdk_pixbuf::Pixbuf, want: usize) -> Vec<Rgb> {
    let (w, h) = (pb.width(), pb.height());
    let nch = pb.n_channels() as usize;
    let rowstride = pb.rowstride() as usize;
    let has_alpha = pb.has_alpha();
    let bytes = unsafe { pb.pixels() };

    // (count, sum_r, sum_g, sum_b) per bin.
    let mut bins: HashMap<u16, (u64, u64, u64, u64)> = HashMap::new();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * rowstride + x * nch;
            if i + nch > bytes.len() {
                continue;
            }
            if has_alpha && bytes[i + 3] < ALPHA_FLOOR {
                continue;
            }
            let (r, g, b) = (bytes[i], bytes[i + 1], bytes[i + 2]);
            let key = (((r >> QUANT_SHIFT) as u16) << 8)
                | (((g >> QUANT_SHIFT) as u16) << 4)
                | ((b >> QUANT_SHIFT) as u16);
            let e = bins.entry(key).or_insert((0, 0, 0, 0));
            e.0 += 1;
            e.1 += r as u64;
            e.2 += g as u64;
            e.3 += b as u64;
        }
    }

    let mut v: Vec<(u64, Rgb)> = bins
        .into_values()
        .map(|(n, sr, sg, sb)| {
            (
                n,
                Rgb {
                    r: (sr / n) as u8,
                    g: (sg / n) as u8,
                    b: (sb / n) as u8,
                },
            )
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(want).map(|(_, c)| c).collect()
}

#[cfg(test)]
mod disc_tests {

    #[test]
    fn fit_at_shrinks_fit_and_keeps_it_centred() {
        // A 600×600 logo on a 600 canvas: 75% is 450 px with a 75 px margin.
        let p = Placement::fit_at(600, 600, 600, 0.75);
        assert_eq!((p.scale, p.dx, p.dy), (0.75, 75.0, 75.0));
        // A wide 800×400 on 600: Fit is 600×300; at 50% it is 300×150, centred.
        let p = Placement::fit_at(800, 400, 600, 0.5);
        assert_eq!((p.w(800), p.h(400)), (300.0, 150.0));
        assert_eq!((p.dx, p.dy), (150.0, 225.0));
        assert_eq!(Placement::fit_at(800, 400, 600, 1.0), Placement::fit(800, 400, 600));
    }

    use super::*;

    fn bytes(pb: &gdk_pixbuf::Pixbuf) -> Vec<u8> {
        unsafe { pb.pixels() }.to_vec()
    }

    #[test]
    fn rounded_at_full_is_the_disc_and_at_zero_the_whole_square() {
        let src = red(64);
        let disc = apply_mask(&src, Mask::Disc, OuterFill::Alpha, None).unwrap();
        let round = apply_mask(&src, Mask::Rounded(1.0), OuterFill::Alpha, None).unwrap();
        assert_eq!(bytes(&disc), bytes(&round), "the slider runs continuously from square to circle");
        let square = apply_mask(&src, Mask::Rounded(0.0), OuterFill::Alpha, None).unwrap();
        assert_eq!(px(&square, 0, 0), (255, 0, 0, 255), "no rounding: the corner pixel stays");
        assert_eq!(px(&disc, 0, 0).3, 0, "the disc drops it");
    }

    #[test]
    fn a_chamfer_cuts_the_corners_and_keeps_the_edges() {
        let m = apply_mask(&red(64), Mask::Chamfer(0.5), OuterFill::White, None).unwrap();
        assert_eq!(px(&m, 0, 0), (255, 255, 255, 255), "the corner is cut, filled white");
        assert_eq!(px(&m, 32, 0), (255, 0, 0, 255), "the middle of an edge is kept");
        assert_eq!(px(&m, 32, 32), (255, 0, 0, 255));
        // The cut line: a pixel just inside the 45° cut is red, just outside white.
        assert_eq!(px(&m, 12, 12), (255, 0, 0, 255));
        assert_eq!(px(&m, 6, 6), (255, 255, 255, 255));
    }

    #[test]
    fn fitted_to_the_artwork_the_mask_follows_what_is_drawn() {
        // 200 canvas, opaque only in a 100×100 square in the middle.
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 200, 200).unwrap();
        pb.fill(0x00000000);
        red(100).copy_area(0, 0, 100, 100, &pb, 50, 50);
        let r = drawn_region(&pb).unwrap();
        assert_eq!((r.cx, r.cy, r.side), (100.0, 100.0, 100.0));

        let canvas = apply_mask(&pb, Mask::Disc, OuterFill::Alpha, None).unwrap();
        let art = apply_mask(&pb, Mask::Disc, OuterFill::Alpha, Some(r)).unwrap();
        // The artwork's own corner: inside the canvas disc, outside the logo's disc.
        assert_eq!(px(&canvas, 52, 52).3, 255);
        assert_eq!(px(&art, 52, 52).3, 0);
        // The middle of the artwork's top edge is inside both.
        assert_eq!(px(&art, 100, 51), (255, 0, 0, 255));
        // Nothing drawn: no region.
        let empty = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 10, 10).unwrap();
        empty.fill(0x00000000);
        assert_eq!(drawn_region(&empty), None);
    }

    #[test]
    fn a_gradient_reaches_its_outer_stop_in_the_corner_for_every_shape() {
        let fill = OuterFill::Gradient { inner: Rgb { r: 0, g: 0, b: 0 }, outer: Rgb { r: 255, g: 255, b: 255 } };
        for shape in [Mask::Disc, Mask::Rounded(0.5), Mask::Chamfer(0.5)] {
            let m = apply_mask(&red(64), shape, fill, None).unwrap();
            assert_eq!(px(&m, 0, 0), (255, 255, 255, 255), "{shape:?}");
        }
    }

    /// A solid red square, `n`×`n`, fully opaque.
    fn red(n: i32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, n, n).unwrap();
        pb.fill(0xFF0000FF);
        pb
    }

    fn px(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let b = unsafe { pb.pixels() };
        let i = y as usize * pb.rowstride() as usize + x as usize * 4;
        (b[i], b[i + 1], b[i + 2], b[i + 3])
    }

    #[test]
    fn alpha_fill_leaves_the_corners_actually_transparent() {
        let out = disc_template(&red(64), OuterFill::Alpha).unwrap();
        assert_eq!(px(&out, 0, 0).3, 0, "corner must be fully transparent");
        assert_eq!(px(&out, 32, 32), (255, 0, 0, 255), "centre is the artwork");
    }

    #[test]
    fn white_fill_puts_white_in_the_corners_not_transparency() {
        let out = disc_template(&red(64), OuterFill::White).unwrap();
        assert_eq!(px(&out, 0, 0), (255, 255, 255, 255));
        assert_eq!(px(&out, 32, 32), (255, 0, 0, 255));
    }

    #[test]
    fn solid_fill_uses_the_colour_given() {
        let blue = Rgb { r: 0, g: 0, b: 255 };
        let out = disc_template(&red(64), OuterFill::Solid(blue)).unwrap();
        assert_eq!(px(&out, 0, 0), (0, 0, 255, 255));
    }

    #[test]
    fn gradient_runs_from_the_rim_outward() {
        // inner (at the rim) -> outer (at the corner).
        let out = disc_template(
            &red(64),
            OuterFill::Gradient {
                inner: Rgb { r: 0, g: 0, b: 0 },
                outer: Rgb { r: 255, g: 255, b: 255 },
            },
        )
        .unwrap();
        let corner = px(&out, 0, 0);
        assert_eq!(corner, (255, 255, 255, 255), "corner is the OUTER stop");

        // NB an INSCRIBED disc touches the canvas edges, so there is no "outside"
        // at the mid-edge — pixel (63,32) IS the rim. The fill only exists in the
        // four corners. Sample along the diagonal, just outside the rim: it must
        // still be near the INNER stop.
        let near_rim = px(&out, 6, 6);
        assert!(
            near_rim.0 < 128,
            "just past the rim should be near the inner stop, got {near_rim:?}"
        );
    }

    #[test]
    fn a_non_square_source_is_covered_not_letterboxed() {
        // A disc with bars through it is not a disc. A 128x64 source must fill
        // the square canvas, cropping the long axis rather than padding.
        let wide = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 128, 64).unwrap();
        wide.fill(0x00FF00FF);
        let out = disc_template(&wide, OuterFill::Alpha).unwrap();
        assert_eq!(out.width(), 128);
        assert_eq!(out.height(), 128);
        // Top-centre is inside the disc and must be artwork, not padding.
        assert_eq!(px(&out, 64, 4), (0, 255, 0, 255));
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn cover_centres_and_fills_the_canvas() {
        // A wide source: scaled so the SHORT axis reaches across, centred, with
        // the long axis overhanging equally both sides.
        let p = Placement::cover(800, 400, 400);
        assert_eq!(p.scale, 1.0); // 400/400 on the short axis
        assert_eq!(p.dy, 0.0);
        assert_eq!(p.dx, -200.0); // (400 - 800) / 2
        // Nothing is left uncovered.
        assert!(p.w(800) >= 400.0 && p.h(400) >= 400.0);
    }

    #[test]
    fn snaps_to_centre() {
        let p = Placement { dx: 3.0, dy: -4.0, scale: 1.0 }; // a 400x400 source
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!((s.dx, s.dy), (0.0, 0.0)); // centre == edges here
        assert!(sx && sy);
    }

    #[test]
    fn snaps_each_axis_independently() {
        // Snapped horizontally, free vertically — that is what makes it feel
        // like a guide and not a magnet.
        let p = Placement { dx: 2.0, dy: 100.0, scale: 1.0 };
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!(s.dx, 0.0);
        assert_eq!(s.dy, 100.0, "vertical was far from any guide; leave it alone");
        assert!(sx && !sy);
    }

    #[test]
    fn does_not_snap_beyond_the_threshold() {
        // You must be able to sit deliberately off-centre.
        let p = Placement { dx: 25.0, dy: 25.0, scale: 1.0 };
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!((s.dx, s.dy), (25.0, 25.0));
        assert!(!sx && !sy);
    }

    #[test]
    fn snaps_a_small_image_to_the_canvas_centre() {
        // A 100x100 image: centre-to-centre means dx = (400-100)/2 = 150.
        let p = Placement { dx: 146.0, dy: 150.0, scale: 1.0 };
        let (s, _, _) = snap(p, 100, 100, 400);
        assert_eq!(s.dx, 150.0);
    }

    #[test]
    fn compose_places_the_image_where_you_put_it() {
        let src = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 10, 10).unwrap();
        src.fill(0xFF0000FF);
        let c = compose(&src, Placement { dx: 100.0, dy: 100.0, scale: 1.0 }, 400).unwrap();
        assert_eq!(c.width(), 400);
        let b = unsafe { c.pixels() };
        let at = |x: i32, y: i32| {
            let i = y as usize * c.rowstride() as usize + x as usize * 4;
            (b[i], b[i + 1], b[i + 2], b[i + 3])
        };
        assert_eq!(at(105, 105), (255, 0, 0, 255), "inside the placed image");
        assert_eq!(at(50, 50).3, 0, "outside it is transparent, not black");
    }
}
