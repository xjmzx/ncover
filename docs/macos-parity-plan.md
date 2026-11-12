# Porting plan: bringing the GTK app up to the macOS sibling

Drafted 2026-11-12. Linux is at **0.1.1**; `macos-node/ncover` is at **0.5.1**
(plus an unreleased install change). The two share no code, by decision, and
that stays true. This plan ports **behaviour and design decisions**, not Swift.
Where the Mac side wrote down *why* something is the way it is (its `CLAUDE.md`
"Traps" sections), that reasoning comes across with the feature.

A reversal to note: until now **this repo was the oracle**, and the Swift tests
kept the Rust names and pixel values. For everything below, the Mac
`Tests/NCoverKitTests` (`CompositionTests`, `BuildOpTests`, `TextTests`,
`BatchTests`) is the spec. Port the test first, then make it pass.

## What is already at parity (no work)

Screen pick (portal + `xcolor`), HEX/RGB/HSL, history, palettes (`.gpl`/`.json`
in; GPL/CSS/JSON out; pin three), pick from the artwork, palette from a raster
(quantised) or from an SVG's declared colours, folder browse, square canvas
200–1000 with drag/zoom/snap, grid, Cover/Fit, blank/invert/square, disc mask
with alpha/white/colour/gradient corners, batch over a folder or
`published.json` with a contact sheet and dry run, the `guard_batch` rule
(all three refusals), PNG save, Overwrite guarded to PNG. Batch already
defaults to **No disc**, which is the bug macOS fixed in 0.5.1.

## Out of scope (Mac-only, no Linux meaning)

`.clr` import/export, `~/Library/Colors`, `NSColorPanel` palette attachment,
`NSColor` on the pasteboard, `.dmg`, `/Applications`. The closest Linux idea,
exporting a palette somewhere GIMP/Inkscape will find it, is a separate
question and not part of parity.

---

## Phase 0: the composition model (prerequisite, no visible change)

Everything after this depends on it. Today `build_image_view` holds the
document as a loose set of `Rc<RefCell<…>>`s (`pixbuf`, `place`, `out`,
`svg`, …), and each build step **overwrites `out`**. That is a pipeline, and
the Mac side's central lesson is that it should be a *value*.

- Split `ncover/src/main.rs` (5.7k lines) into modules before adding to it:
  `composition.rs`, `raster.rs` (compose / invert / square / mask), `svg.rs`
  (inspect + the coming vector backend), `batch.rs`, `palette.rs`. The UI stays
  in `main.rs` for now.
- Introduce `Composition { source: Source, canvas: i32, ops: Vec<Operation> }`
  with `Source { raster: Pixbuf, vector: Option<VectorSource> }` and
  `enum Operation { Place(Placement), Mask{..}, Invert, Square{..} }`.
  Further cases arrive in later phases.
- `render_raster(&Composition) -> Pixbuf` folds the ops. The preview draws
  this, so *the screen is what PNG export writes* stays true.
- Every edit goes through one `mutate(|c| …)` function. Anything that bypasses
  it is a bug, because it will not be undoable.
- The existing ~40 tests must pass unchanged. They pin the pixels, and a
  refactor that moves the disc by one pixel is caught there.

**Done when:** the app looks and behaves identically, `cargo test -p ncover`
is green, and `out` no longer exists as separate state.

**Status (2026-11-13): done** on branch `phase0-composition`. 51 tests (45 +
6 composition). Verified against the untouched `main` in a nested Xephyr,
driven through AT-SPI and XTEST: build steps, drag, zoom, Centre/Fit/1:1,
Reset and the frozen drag were pixel-identical. The size change was not,
because **`main` crashes there** (a `RefCell` double borrow) and the new
build does not. Deliberately left for later phases:
- `svg` (the `SvgInfo`) and the file name stay outside `Source`. Phase 3
  moves the SVG text into `Source` as its vector form.
- Drag goes through `mutate` once per motion event. That is fine with no
  undo; phase 1 must commit one undo entry per drag, not per event.
- A size change still drops the build steps, to keep behaviour identical.
  Steps are canvas-relative now (square fraction, disc, invert), so keeping
  them across a size change costs one line in `set_canvas` if wanted.

## Phase 1: undo/redo and the preview backdrop (small, high value)

- **Undo/redo** = two `Vec<Composition>` stacks around `mutate`. No operation
  needs an inverse. Ctrl+Z / Ctrl+Shift+Z, plus header buttons. A drag
  commits **one** undo entry on release, not one per motion event.
- **Backdrop**: checker / grey / black / white / picked colour, drawn by the
  canvas draw handler only. It must **never** reach `render_raster` or SVG
  export. It is a viewing condition, the same category as the grid. The label
  and tooltip say "not exported".
- **Remember the canvas size** between launches. Check first:
  `AppData.canvas` exists, so this may already be done.

Release as **0.2.0**.

**Status (2026-11-13): done** on branch `phase0-composition`. 59 tests (+8
for `History`). Verified in Xephyr: undo and redo restore drag, zoom, steps,
Reset and size exactly (pixel-equal canvas, size picker and stored size
back to 400). The size preference was already remembered (`AppData.canvas`).
Two things the plan did not foresee:
- Application accels stole Ctrl+Z from text fields, so the shortcut is a
  bubble-phase controller instead.
- Opening another image clears the history rather than being undoable.
  The macOS side may differ. That is a design choice to revisit, not a gap.

## Phase 2: masks and resampling quality

- **Mask as a signed distance function** (`Mask::{Disc, RoundedRect(r),
  Chamfer(i)}`). Port `signedDistance` directly: it is ~20 lines, and the
  coverage ramp, the corner fill and the gradient all derive from it. Keep
  `disc_template` as a thin wrapper over `apply_mask(Disc, …)` so the existing
  disc tests still point at the path they were written for.
- **`MaskFit::{Canvas, Artwork}`**: *Artwork* inscribes the mask in the square
  that bounds what is drawn **at that point in the op list**
  (`drawn_region()`, alpha floor 8). With an off-centre region, normalise the
  gradient to the *largest* corner distance.
- Gradient normalisation stays at the **furthest drawn pixel centre**, not the
  abstract corner.
- **Two resamplers.** `compose` should area-average (in *premultiplied*
  space) when shrinking and stay nearest-neighbour when enlarging. The Mac
  measured 4.04% → 1.63% RMSE on a 2048→400 cover. "A colour tool must not
  invent colours" is right for sampling and enlarging, wrong for shrinking.
- **Re-render SVG at the drawn size.** Today `load_image_file` rasterises SVG
  once at `from_file_at_scale(…, 1024, 1024)`, then resamples. Instead,
  re-rasterise through librsvg (`rsvg` crate, or the pixbuf loader at the
  target size) on canvas-size change, framing change and zoom end. **Never on
  drag**, because dragging cannot change the scale.

UI: three shape buttons (disc / rounded / chamfer) with an amount slider shown
for the latter two, and a Canvas | Artwork toggle.

Release as **0.3.0**.

## Phase 3: vector export, template and background

The largest structural payoff. librsvg is already the loader here, so SVG
output can be verified against the same renderer that reads it.

- `Operation::has_vector_form()` with an **honest** answer per case. Gradient
  fill on a non-disc mask → `false` (an SVG `radialGradient` only follows a
  circular rim). Claiming a form you have not written makes SVG export drop
  the step silently, which is the one way to corrupt output quietly.
- `Composition::vector_refusal() -> Option<String>`, phrased for a person
  ("step 3, Invert, has no vector form"). It is shown beside a disabled
  *Save as SVG*.
- `render_vector(&Composition) -> Option<String>`:
  - Source as a **nested `<svg>`**, re-headed (`x/y/width/height`), never
    rewriting the artwork's own paths.
  - Mask = `clipPath`. The outer fill is clipped to the **inverse** of the
    mask (even-odd canvas-minus-shape), never a plain rect behind the artwork.
  - Invert = `feColorMatrix`; square = `<rect>`.
  - For `MaskFit::Artwork`, ask the raster fold for the region by rendering
    the prefix of the list. Correctness beats inferring bounds.
- **Blank canvas carries a vector form** (a white rect), so a composition
  built from nothing can be an SVG.
- **`Operation::Template(Placement)`**: the source is placed and **not
  drawn**. The canvas draws a faint ghost (draw handler, like the grid). A
  raster template does **not** block SVG output: `vector_refusal` asks whether
  the source is *drawn* before it asks its format. Toggling converts
  `Place` ↔ `Template` in place, so the placement survives.
- **`Operation::Background(Rgb)`**: composites **under** whatever is drawn
  wherever it sits in the list, so a later mask clips it. In SVG the `<rect>`
  is **prepended**. Keep the labels opposite to the backdrop's: this one *is*
  written to the file.
- Test the raster and vector backends against each other: rasterise the
  exported SVG with librsvg and compare it to `render_raster` (the Mac side's
  gradient-agreement test is the model).

Release as **0.4.0**.

## Phase 4: batch says what it will touch

Mostly independent of phases 0–3. It could go first if batch is what you are
using most.

- **Survey** on every scope/filter change: image count, release/folder count,
  per-release file lists, releases that matched nothing. It decodes nothing
  (a directory walk only), runs off the main thread, and debounces typing.
- The survey and `find_images` must **de-duplicate identically**, and a test
  ties the two counts together. The Run button reads "Run (N files)", which
  is a promise.
- **Name releases from the manifest** ("Artist — Title"), not the folder.
  Extend `ManifestRelease` to keep artist/title.
- **Auto-preview**: four samples re-render on every settled change, through
  the *same* `batch_render` a run uses. Gather every input into one key so
  nothing can leave a stale preview. The Preview button still renders twelve.
- **Failures carry their reason.** Today the run does `Err(_) => failed += 1`
  (`main.rs` ~1339) and drops the reason. Keep `(path, error)` and list them.
  Previews show failing files too, rather than omitting them.

Release as **0.5.0**.

## Phase 5: type

Needs phase 3 for the vector half. This is the part that differs most from
the Mac implementation, because it uses Pango/Cairo instead of Core Text/CG.

- `Operation::Text(TextSpec)`: text, font description, size, x/y, tracking
  and line height **as fractions of the canvas** (so type scales with the
  output), colour, alignment, and a nine-way placement grid.
- **One layout function** decides glyph positions, and both backends read
  from it:
  - Raster: `pangocairo::functions::show_layout` into an ARGB32
    `ImageSurface`. That surface is **premultiplied**, so un-premultiply before
    compositing in straight alpha, or every letter edge darkens through the
    disc mask. Turn off subpixel AA (`cairo::Antialias::Gray` via font
    options) to avoid coloured fringes on transparency.
  - Vector: `pangocairo::functions::layout_path` then `cr.copy_path()` →
    SVG `d`, with `fill-rule="nonzero"` (counters are wound the other way,
    and even-odd would punch holes where letters overlap). **Outlines, never
    `<text font-family>`.**
  - Agreement check: rasterise the exported SVG with librsvg and compare
    pixels. The Mac side got 98.8% byte-identical; aim for a test with a
    stated threshold.
- Measure the block from **first ascent to last descent**, not n × line
  height, or a single line sits off-centre by the font's leading.
- Font choice: `gtk::FontDialog` (GTK 4.10; the crate already enables
  `v4_10`). Pango substitutes silently, the same as Core Text, so check that
  the resolved family matches what was asked for and say so when it doesn't.
- Size slider is **logarithmic**, 2%–200% of the canvas, readout in % and
  pt; tracking −0.25…0.75. The range runs past 100% on purpose.
- New dependency: `pangocairo` (cairo-rs is already in the lock via gtk4).

Alongside it, **colour-well popovers**: every colour button (square, mask,
both gradient stops, background, batch, text) gets a popover with the current
colour, recent history and pinned palettes. This is cheap once `build_swatch_flow`
can be reused inside a `Popover`.

Release as **0.6.0**.

---

## Order and sizing

| Phase | What | Size | Depends on |
|---|---|---|---|
| 0 | Composition model + module split | M–L | — |
| 1 | Undo/redo, backdrop | S | 0 |
| 2 | SDF masks, mask fit, resamplers, SVG re-render | M | 0 |
| 3 | SVG export, template, background | L | 0, 2 |
| 4 | Batch survey, auto-preview, failure reasons | M | — |
| 5 | Type + colour-well popovers | L | 3 |

Recommended: **0 → 1 → 4 → 2 → 3 → 5.** Phase 0 de-risks everything, and
phase 1 is the cheapest visible win. Phase 4 is independent and the most
useful day to day. Type comes last because it is the largest and needs vector
export to be worth having.

## Things to settle before starting

- **Is parity the goal, or a selection?** The two apps are meant to diverge.
  Type and vector export are the expensive half; undo, masks and batch are the
  cheap half.
- **Wayland verification.** Each phase gets checked in the running app on a
  Wayland session, not only by tests. XTEST does not reach Wayland-native
  windows, so scripted checks run the app with `GDK_BACKEND=x11`.
- **Docs.** Each phase adds its own "Traps" section to this repo's
  `CLAUDE.md`, in the same spirit as the Mac file, and a CHANGELOG entry.
