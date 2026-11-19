# Changelog

All notable changes to **n.cover**. Versions follow [semver](https://semver.org).

The version here is n.cover's. The `xcolor` CLI in this repository keeps its
own, upstream-derived version (0.6.x) — see `make version`.

There is a sibling at [`macos-node/ncover`](https://github.com/macos-node/ncover):
a native SwiftUI app with the same name and purpose, sharing no code and free
to diverge. Its version numbers are independent of these.

## [Unreleased]

### Removed
- **Preview, its twelve-thumbnail sheet and the four automatic samples.** The
  grid replaces all three: it is always current and shows every file.

### Changed
- Builds against gtk4's `v4_12` feature (was `v4_10`): GTK 4.12 or newer.
- **Save and Overwrite shrink smoothly.** When the framing makes the source
  smaller, the written file is area-averaged (as Cap and Square are) instead
  of keeping the nearest pixel, so fine type and halftones survive. The
  on-screen preview is unchanged. Save is now also on for a framed image
  with no build steps.

### Added
- **Batch scope "From a list…"**: a text file with one release folder per
  line, such as ndisc's Export list from Local covers vs Blossom. Where a
  line names its cover file, only that file is taken, so a release's back
  or inlay is never touched. Its "Artist — Title" names the release in the
  Files list, and the output mirrors the folders under their common parent.
  A hand-made list of folders works too.
- **The framing picks the lines.** When the list carries ndisc's actions
  column, Cap takes only the lines marked `ncover:cap` and Square only
  those marked `ncover:square`. The survey line counts the rest without
  touching them ("not in this run: 112 need a new source · 24 to review …").
- **Batch shows every file it would touch, as a grid.** The right side of
  the Batch page is a live grid that follows the scope and the framing, with
  no button to press: under Cover and Fit every file, in run order, as the
  run would write it; under Cap and Square only the files the run would
  change, the most cropped (Square) or largest (Cap) first. Thumbnails render
  through the run's own function, and only for the cells on screen, so a
  whole library scrolls. **Before** flips the grid to the files as they are
  now; **Open folder** shows the selected cover in the file manager.
- **The Files list is numbered and is the same list.** It sits under the
  controls on the left and holds exactly the grid's files, in the grid's
  order, under the grid's numbers, each with its size change in words
  ("600 × 533 → 533 × 533 · 67 px off the sides (11%)"). Selecting a file
  in either scrolls the other to it.
- **Overwrite works on JPEG covers**, keeping them JPEG (quality 92) under
  their own name, and it no longer needs a build step: framing alone is
  enough, so a cover Batch flagged can be squared by hand from the grid.
  The confirm says what it will write (the original's size, and "600 ×
  600, JPEG at quality 92"). A result with transparent areas is refused for a JPEG
  rather than flattened onto a colour nobody chose. The file is written
  beside the original and renamed over it, so a failed write leaves the
  original whole.
- **Batch recounts after an Overwrite.** Going back to Batch rereads the
  files: the grid, the counts and the Run button follow, so a cover you
  have squared in Picker drops off Square's list.
- **Double-click a file in the grid or the Files list to open it in Picker.**
  The grid's files become Picker's browse list, so ← and → step through
  them in the grid's order ("41 / 241 · cover.jpg"). Enter does the same.
- **Batch is a page of its own**, beside Picker and Palettes. Its controls
  and the Files list sit in a column on the left, and the grid or the last
  run's results fill the rest of the window. The colours a Colour or
  Gradient disc will use show beside the recipe.
- **The Picker page's side columns can be hidden** from the header: the
  colours column (left) and the Source / Result rail (right). Each remembers
  its state. Turning the tips on brings the colours column back, since the
  tips sit at its top.
- **Square**, a fourth batch framing. It crops a non-square image to a
  square of its shorter side, taken from the centre, then shrinks it to the
  size if it is still larger (area averaging, as Cap). It keeps the file's
  name and format (JPEG stays JPEG at quality 92) and writes nothing for an
  image that is already square. The Run button, the samples and the dry run
  say how many it would change ("Run (247 of 247 to square)").

## [0.3.0] — 2026-11-13

### Added
- **Batch says what it will touch before it touches anything.** As soon as
  there is a scope, a line reads e.g. "37 images in 35 releases · 9 releases
  matched nothing", with a Files list underneath naming each release from
  ndisc's manifest ("Artist — Title") and the files it contributes. It
  follows the name filter as you type. The Run button reads "Run (37
  files)", from the same walk the run itself does.
- **Batch samples itself.** The first four outputs render automatically
  under the controls and re-render when the scope, filter, size, framing,
  disc or picked colours change, through the same renderer a run writes
  from. Preview still renders twelve.

- **Cap**, a third batch framing for tidying a library's covers. It shrinks
  only images larger than the chosen size, so their longer side becomes that
  size. It keeps each image's shape (no crop, no padding) and its format
  (JPEG stays JPEG at quality 92, PNG stays PNG), and writes nothing at all
  for an image already within the cap. The survey line and the Run button
  say how many it would change ("Run (245 of 2377 to shrink)"), the samples
  show the first ones that change, and the Dry run lists exactly those.
  Shrinking averages every source pixel by how much of it each output pixel
  covers, with alpha premultiplied, instead of keeping the nearest one, so
  halftones and fine type do not break up. WebP and GIF over the cap are
  refused with the reason rather than converted.

- Batch has its own size picker, bound to the canvas size above the image.
  Batch used to read only that one, which is hidden until an image is open,
  so with nothing open the size (and Cap's limit) was stuck at 400.

### Fixed
- **Two covers in one release overwrote each other.** Every batch output is
  a PNG named after its source, so a release holding both `cover.jpg` and
  `cover.png` wrote two files to one `cover.png`, and the second replaced
  the first. A dry run could not show it, since each file renders fine on
  its own. Output names are now planned for the whole run: sources that
  would share a name keep their extension in it (`cover-jpg.png`,
  `cover-png.png`), and everything else keeps the plain name it always had.
  The survey line says when this happens. 14 releases in the published
  catalogue were affected. Present since batch was added.

### Changed
- A recipe that cannot work yet (Colour with no colour picked) is now said
  in place, under the controls, rather than only as a dialog when you press
  a button.
- The image column scrolls, so an open Batch section no longer pushes the
  window past the bottom of the screen.
- Preview and sample captions show the release folder and the file name on
  two lines. Across a library every file is `cover.png`, and the old single
  line cut off the part that told them apart.

## [0.2.0] — 2026-11-13

### Added
- **Undo and redo** for the artwork: header buttons, Ctrl+Z, and Ctrl+Shift+Z
  or Ctrl+Y. Every edit is a step: build steps, Reset, Fit / Cover / Centre /
  1:1, and a canvas size change (undoing it puts the size picker back too).
  A drag is one step however long it is, and a run of wheel zooms is one
  step until you pause. Opening another image starts a new history. Ctrl+Z
  in a text field still undoes the typing, not the canvas.
- **Preview backdrop**: Checker, Grey, Black, White, or the current colour,
  shown behind the canvas and the Result thumbnail so transparency can be
  judged against what the artwork will sit on. It is never written to the
  file, and undo leaves it alone, because it is a viewing condition rather
  than part of the picture.

### Fixed
- **Changing the canvas size with an image open crashed the app.** The size
  handler borrowed the placement mutably and immutably in one statement
  (`RefCell already borrowed`), and the panic aborted the process from inside
  the GTK callback. Present since the GUI was renamed to n.cover, so in 0.1.0
  and 0.1.1.

### Changed
- The editor's document is now a value: a source plus an ordered list of
  steps (`composition.rs`), rendered by one function. The preview, Save and
  Overwrite all read that render, and every edit goes through one `mutate`.
  Nothing about the app's behaviour or pixels changes. This is the groundwork
  for undo and SVG export.
- `main.rs` split into modules: `color`, `palette`, `samples`, `raster`,
  `batch`, `svg`, `composition`. The UI stays in `main.rs`.

## [0.1.1] — 2026-10-01

### Fixed
- Screen pick on Wayland. The `xcolor` CLI reads pixels from the X root
  window, which a Wayland session's XWayland refuses, so *Pick* did nothing
  useful there. On Wayland the app now asks the desktop to do the pick
  (the Screenshot portal's `PickColor`); X11 sessions still use `xcolor`.

## [0.1.0] — 2026-09-23

First tagged release. The app has existed for some time; this is the point it
became installable from a release rather than only from source.

### Added
- Pick any colour on screen via the bundled `xcolor` CLI — HEX / RGB / HSL,
  copied to the clipboard — with recent colours collecting into a History
  palette.
- Palettes: import `.gpl` / `.json`, export to GPL / CSS / JSON, named
  swatches, and up to three pinned to the Picker.
- Open PNG / SVG / JPEG / WebP, or browse a folder with the arrow keys. Click
  any pixel to pick it; build a palette from an image (quantised) or from an
  SVG's *declared* colours.
- Place a source on a square canvas (200–1000 px) with drag, zoom, snapping
  and a grid.
- Build: blank canvas, Invert, centred squares, and a disc / label mask with
  alpha, white, colour or gradient corners — the steps stack.
- Batch the same recipe over a folder or an ndisc-published discography, with
  a Preview contact sheet and a dry run. It never writes over the source.
- Save as PNG, or overwrite the original PNG in place.

[Unreleased]: https://github.com/xjmzx/ncover/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/xjmzx/ncover/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/xjmzx/ncover/releases/tag/v0.2.0
[0.1.1]: https://github.com/xjmzx/ncover/releases/tag/v0.1.1
[0.1.0]: https://github.com/xjmzx/ncover/releases/tag/v0.1.0
