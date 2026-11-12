use super::*;
use std::time::{Duration, Instant};

// ---------- composition ----------
//
// The document is a VALUE: a source plus an ordered list of operations. The
// picture on screen and the file on disk are both rendered from that list —
// nothing holds a built pixbuf of its own that could drift from it.
//
// That is what makes the next things cheap. Undo is a stack of these values,
// so no operation needs a hand-written inverse. Adding a capability is adding
// an `Operation` case and its arm in `render_raster`, not threading another
// piece of state through the UI. And a second backend (SVG) can read the same
// list.
//
// The macOS sibling (macos-node/ncover, Composition.swift) has the same shape.
// The two share no code, only the idea.

/// What was opened, or the synthetic white of a Blank canvas.
///
/// Equality is identity of the pixbuf (a GObject compares by pointer), which
/// is exactly the question undo asks: is this the same source as before?
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Source {
    pub(crate) raster: gdk_pixbuf::Pixbuf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Operation {
    /// Draw the source onto the canvas at this placement.
    Place(Placement),
    /// The disc mask, with its corner treatment. Colours are resolved when the
    /// step is added, so the step means the same thing later.
    Disc(OuterFill),
    /// Invert RGB, leaving alpha alone.
    Invert,
    /// An opaque, centred square; `frac` is its side as a fraction of the canvas.
    Square { frac: f64, colour: Rgb },
}

impl Operation {
    /// A name for a person, for error messages now and refusals later.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Operation::Place(_) => "Place",
            Operation::Disc(_) => "Disc",
            Operation::Invert => "Invert",
            Operation::Square { .. } => "Square",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Composition {
    pub(crate) source: Source,
    pub(crate) canvas: i32,
    pub(crate) ops: Vec<Operation>,
}

impl Composition {
    /// A freshly loaded source, covering the canvas — what Open, Blank and the
    /// folder browser all start from.
    pub(crate) fn new(raster: gdk_pixbuf::Pixbuf, canvas: i32) -> Composition {
        let p = Placement::cover(raster.width(), raster.height(), canvas);
        Composition {
            source: Source { raster },
            canvas,
            ops: vec![Operation::Place(p)],
        }
    }

    /// Where the source sits. Identity when nothing has placed it.
    pub(crate) fn placement(&self) -> Placement {
        self.ops
            .iter()
            .find_map(|op| match op {
                Operation::Place(p) => Some(*p),
                _ => None,
            })
            .unwrap_or(Placement { dx: 0.0, dy: 0.0, scale: 1.0 })
    }

    /// Replace the placement, inserting one at the front if there is none.
    pub(crate) fn set_placement(&mut self, p: Placement) {
        match self.ops.iter_mut().find(|op| matches!(op, Operation::Place(_))) {
            Some(op) => *op = Operation::Place(p),
            None => self.ops.insert(0, Operation::Place(p)),
        }
    }

    /// True once anything beyond the placement has been added. The UI freezes
    /// the framing while this holds — the steps were judged against it.
    pub(crate) fn has_build(&self) -> bool {
        self.ops.iter().any(|op| !matches!(op, Operation::Place(_)))
    }

    /// Back to just the placed source (the Reset button).
    pub(crate) fn clear_build(&mut self) {
        self.ops.retain(|op| matches!(op, Operation::Place(_)));
    }

    /// Change the output size. The framing is rescaled, not reset. The build
    /// is dropped, which is what the app has always done on a size change.
    pub(crate) fn set_canvas(&mut self, to: i32) {
        let p = self.placement().rescaled(self.canvas, to);
        self.canvas = to;
        self.set_placement(p);
        self.clear_build();
    }
}

/// A canvas with nothing on it: every pixel transparent.
fn empty_canvas(size: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size)
        .context("could not allocate the canvas")?;
    pb.fill(0x00000000);
    Ok(pb)
}

/// Fold the operations over a canvas. This is the ONE raster renderer: the
/// preview shows what it returns, and Save / Overwrite write it.
pub(crate) fn render_raster(doc: &Composition) -> Result<gdk_pixbuf::Pixbuf> {
    let mut cur = empty_canvas(doc.canvas)?;
    for op in &doc.ops {
        cur = match *op {
            Operation::Place(p) => compose(&doc.source.raster, p, doc.canvas)?,
            Operation::Disc(fill) => disc_template(&cur, fill)?,
            Operation::Invert => invert_rgb(&cur)?,
            Operation::Square { frac, colour } => stamp_square(&cur, frac, colour)?,
        };
    }
    Ok(cur)
}

// ---------- undo ----------
//
// Undo is two stacks of whole documents. No operation needs an inverse,
// because nothing is ever undone — an earlier value is simply put back.
// Compositions are cheap to keep: the source pixbuf is shared by reference,
// and the rest is a short list of small values.

/// How many steps back you can go. Generous; each one is a few hundred bytes.
pub(crate) const UNDO_DEPTH: usize = 200;

/// Edits in the same group, this close together, are one undo step. A drag
/// is a stream of motion events and a zoom is a run of wheel clicks; undoing
/// them one event at a time would be undoing nothing anyone did.
pub(crate) const COALESCE: Duration = Duration::from_millis(800);

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Composition>,
    redo: Vec<Composition>,
    /// The group the last recorded edit belonged to, and when it happened.
    open: Option<(&'static str, Instant)>,
}

impl History {
    /// Record that the document is about to stop being `prev`. `group` names
    /// a continuous gesture ("drag", "zoom"); `None` is a discrete edit that
    /// is always its own step.
    pub(crate) fn record(&mut self, prev: Composition, group: Option<&'static str>, now: Instant) {
        let continues = match (group, self.open) {
            (Some(g), Some((open, t))) => g == open && now.duration_since(t) < COALESCE,
            _ => false,
        };
        self.open = group.map(|g| (g, now));
        if continues {
            return; // the first edit of the gesture already saved where it started
        }
        self.undo.push(prev);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// End the current gesture, so the next edit starts a new step even if
    /// it is the same kind — two drags are two steps.
    pub(crate) fn break_group(&mut self) {
        self.open = None;
    }

    /// Step back from `cur`. Returns the document to show, or None.
    pub(crate) fn undo(&mut self, cur: Composition) -> Option<Composition> {
        let prev = self.undo.pop()?;
        self.redo.push(cur);
        self.open = None;
        Some(prev)
    }

    /// Step forward again from `cur`. Returns the document to show, or None.
    pub(crate) fn redo(&mut self, cur: Composition) -> Option<Composition> {
        let next = self.redo.pop()?;
        self.undo.push(cur);
        self.open = None;
        Some(next)
    }

    /// A new document (Open, Blank, the folder browser) starts a new history.
    pub(crate) fn clear(&mut self) {
        *self = History::default();
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    fn doc(tag: f64) -> Composition {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 4, 4).unwrap();
        let mut d = Composition::new(pb, 100);
        d.set_placement(Placement { dx: tag, dy: 0.0, scale: 1.0 });
        d
    }

    fn dx(d: &Composition) -> f64 {
        d.placement().dx
    }

    #[test]
    fn undo_puts_the_earlier_document_back_and_redo_returns() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(doc(0.0), None, t); // 0 -> 1
        h.record(doc(1.0), None, t); // 1 -> 2
        let back = h.undo(doc(2.0)).unwrap();
        assert_eq!(dx(&back), 1.0);
        let back = h.undo(back).unwrap();
        assert_eq!(dx(&back), 0.0);
        assert!(h.undo(back.clone()).is_none(), "nothing before the first edit");
        let fwd = h.redo(back).unwrap();
        assert_eq!(dx(&fwd), 1.0);
        assert_eq!(dx(&h.redo(fwd).unwrap()), 2.0);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_new_edit_after_undo_drops_the_redo_branch() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(doc(0.0), None, t);
        let back = h.undo(doc(1.0)).unwrap();
        assert!(h.can_redo());
        h.record(back, None, t);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_drag_is_one_step_however_many_events_it_has() {
        let mut h = History::default();
        let t = Instant::now();
        for i in 0..30 {
            // each motion event records the document it is replacing
            h.record(doc(i as f64), Some("drag"), t + Duration::from_millis(i * 16));
        }
        let back = h.undo(doc(30.0)).unwrap();
        assert_eq!(dx(&back), 0.0, "undo goes to where the drag STARTED");
        assert!(!h.can_undo());
    }

    #[test]
    fn two_drags_are_two_steps() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(doc(0.0), Some("drag"), t);
        h.record(doc(1.0), Some("drag"), t);
        h.break_group(); // drag-begin of the second drag
        h.record(doc(2.0), Some("drag"), t);
        assert_eq!(dx(&h.undo(doc(3.0)).unwrap()), 2.0);
        assert_eq!(dx(&h.undo(doc(2.0)).unwrap()), 0.0);
    }

    #[test]
    fn a_pause_ends_a_zoom_and_a_different_gesture_never_merges() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(doc(0.0), Some("zoom"), t);
        h.record(doc(1.0), Some("zoom"), t + COALESCE + Duration::from_millis(1));
        h.record(doc(2.0), Some("drag"), t + COALESCE + Duration::from_millis(2));
        assert_eq!(dx(&h.undo(doc(3.0)).unwrap()), 2.0);
        assert_eq!(dx(&h.undo(doc(2.0)).unwrap()), 1.0);
        assert_eq!(dx(&h.undo(doc(1.0)).unwrap()), 0.0);
    }

    #[test]
    fn discrete_edits_never_merge_even_back_to_back() {
        let mut h = History::default();
        let t = Instant::now();
        h.record(doc(0.0), None, t);
        h.record(doc(1.0), None, t);
        assert_eq!(dx(&h.undo(doc(2.0)).unwrap()), 1.0);
    }

    #[test]
    fn the_depth_is_capped_by_dropping_the_oldest() {
        let mut h = History::default();
        let t = Instant::now();
        for i in 0..(UNDO_DEPTH + 5) {
            h.record(doc(i as f64), None, t);
        }
        let mut cur = doc(-1.0);
        let mut n = 0;
        while let Some(prev) = h.undo(cur.clone()) {
            cur = prev;
            n += 1;
        }
        assert_eq!(n, UNDO_DEPTH);
        assert_eq!(dx(&cur), 5.0, "the five oldest fell off");
    }

    #[test]
    fn documents_compare_by_source_identity_and_steps() {
        let a = doc(0.0);
        let mut b = a.clone();
        assert_eq!(a, b, "a clone is the same document");
        b.ops.push(Operation::Invert);
        assert_ne!(a, b);
        assert_ne!(doc(0.0), doc(0.0), "a different pixbuf is a different source");
    }
}

#[cfg(test)]
mod composition_tests {
    use super::*;

    fn src(w: i32, h: i32, rgba: u32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, w, h).unwrap();
        pb.fill(rgba);
        pb
    }

    fn bytes(pb: &gdk_pixbuf::Pixbuf) -> Vec<u8> {
        pb.read_pixel_bytes().to_vec()
    }

    #[test]
    fn a_new_composition_is_the_source_covering_the_canvas() {
        let d = Composition::new(src(300, 200, 0xff0000ff), 100);
        assert_eq!(d.ops.len(), 1);
        assert!(!d.has_build());
        let p = Placement::cover(300, 200, 100);
        assert_eq!(d.placement(), p);
        assert_eq!(
            bytes(&render_raster(&d).unwrap()),
            bytes(&compose(&d.source.raster, p, 100).unwrap())
        );
    }

    /// The fold must produce exactly what applying each step to the previous
    /// result did before there was a fold — the app's pixels do not move.
    #[test]
    fn rendering_the_list_equals_applying_the_steps_one_by_one() {
        let mut d = Composition::new(src(256, 256, 0x3366ccff), 200);
        let fill = OuterFill::Gradient {
            inner: Rgb { r: 255, g: 0, b: 0 },
            outer: Rgb { r: 0, g: 0, b: 255 },
        };
        let sq = Operation::Square { frac: 0.4, colour: Rgb { r: 0, g: 0, b: 0 } };
        d.ops.extend([sq, Operation::Invert, Operation::Disc(fill)]);

        let mut by_hand = compose(&d.source.raster, d.placement(), 200).unwrap();
        by_hand = stamp_square(&by_hand, 0.4, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        by_hand = invert_rgb(&by_hand).unwrap();
        by_hand = disc_template(&by_hand, fill).unwrap();

        assert_eq!(bytes(&render_raster(&d).unwrap()), bytes(&by_hand));
    }

    #[test]
    fn set_placement_replaces_rather_than_adding() {
        let mut d = Composition::new(src(10, 10, 0xffffffff), 100);
        d.ops.push(Operation::Invert);
        let p = Placement { dx: 5.0, dy: -3.0, scale: 2.0 };
        d.set_placement(p);
        assert_eq!(d.ops, vec![Operation::Place(p), Operation::Invert]);
    }

    #[test]
    fn clear_build_keeps_only_the_placement() {
        let mut d = Composition::new(src(10, 10, 0xffffffff), 100);
        let p = d.placement();
        d.ops.extend([Operation::Invert, Operation::Disc(OuterFill::Alpha)]);
        assert!(d.has_build());
        d.clear_build();
        assert_eq!(d.ops, vec![Operation::Place(p)]);
    }

    #[test]
    fn a_size_change_rescales_the_framing_and_drops_the_build() {
        let mut d = Composition::new(src(400, 300, 0xffffffff), 400);
        d.set_placement(Placement { dx: 40.0, dy: 20.0, scale: 0.5 });
        d.ops.push(Operation::Invert);
        d.set_canvas(200);
        assert_eq!(d.canvas, 200);
        assert_eq!(d.placement(), Placement { dx: 20.0, dy: 10.0, scale: 0.25 });
        assert!(!d.has_build());
    }

    #[test]
    fn an_empty_list_renders_a_transparent_canvas() {
        let mut d = Composition::new(src(10, 10, 0xffffffff), 50);
        d.ops.clear();
        let pb = render_raster(&d).unwrap();
        assert_eq!((pb.width(), pb.height()), (50, 50));
        assert!(bytes(&pb).iter().all(|b| *b == 0));
    }
}
