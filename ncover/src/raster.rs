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

/// Mask `src` into a disc inscribed in a square canvas, filling the corners
/// per `fill`.
///
/// The source is scaled to COVER the square (never letterboxed — a disc with
/// bars through it is not a disc), and the rim is antialiased over one pixel so
/// the edge does not read as a staircase.
pub(crate) fn disc_template(src: &gdk_pixbuf::Pixbuf, fill: OuterFill) -> Result<gdk_pixbuf::Pixbuf> {
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

    let cx = size as f64 / 2.0;
    let cy = size as f64 / 2.0;
    let radius = size as f64 / 2.0;
    // Cover: the smaller source axis must reach across the square.
    let scale = size as f64 / sw.min(sh) as f64;
    // Gradient normalisation: distance to the furthest PIXEL CENTRE, not to the
    // abstract corner. A pixel's centre is half a pixel inside the corner, so
    // normalising to the corner leaves the corner pixel short of the outer stop
    // (94.7% on a 64px canvas — ask for black->white and the corner comes out
    // #F1F1F1). The furthest thing that actually gets drawn should reach the
    // end of the ramp.
    let corner = ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt();

    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // Coverage: 1 inside, 0 outside, ramped across the last pixel.
            let cov = ((radius - dist) + 0.5).clamp(0.0, 1.0);

            // Source pixel under this destination pixel (cover-scaled, centred).
            let sx = ((x as f64 - cx) / scale + sw as f64 / 2.0).floor() as i32;
            let sy = ((y as f64 - cy) / scale + sh as f64 / 2.0).floor() as i32;
            let (ir, ig, ib, ia) = sample(src, sbytes, sx, sy);

            let (or_, og, ob, oa) = match fill {
                OuterFill::Alpha => (0, 0, 0, 0),
                OuterFill::White => (255, 255, 255, 255),
                OuterFill::Solid(c) => (c.r, c.g, c.b, 255),
                OuterFill::Gradient { inner, outer } => {
                    let t = ((dist - radius) / (corner - radius)).clamp(0.0, 1.0);
                    (
                        lerp(inner.r, outer.r, t),
                        lerp(inner.g, outer.g, t),
                        lerp(inner.b, outer.b, t),
                        255,
                    )
                }
            };

            // Composite the disc over the corner fill by coverage.
            let i = y as usize * drow + x as usize * 4;
            let a_in = ia as f64 / 255.0 * cov;
            let a_out = oa as f64 / 255.0 * (1.0 - cov);
            let a = a_in + a_out;
            if a <= 0.0 {
                dbytes[i] = 0;
                dbytes[i + 1] = 0;
                dbytes[i + 2] = 0;
                dbytes[i + 3] = 0;
                continue;
            }
            let mix = |inside: u8, outside: u8| -> u8 {
                (((inside as f64 * a_in) + (outside as f64 * a_out)) / a)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            dbytes[i] = mix(ir, or_);
            dbytes[i + 1] = mix(ig, og);
            dbytes[i + 2] = mix(ib, ob);
            dbytes[i + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(dst)
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
    use super::*;

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
