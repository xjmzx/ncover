# Changelog

All notable changes to **n.cover**. Versions follow [semver](https://semver.org).

The version here is n.cover's. The `xcolor` CLI in this repository keeps its
own, upstream-derived version (0.6.x) — see `make version`.

There is a sibling at [`macos-node/ncover`](https://github.com/macos-node/ncover):
a native SwiftUI app with the same name and purpose, sharing no code and free
to diverge. Its version numbers are independent of these.

## [Unreleased]

## [0.5.2] — 2026-11-20

### Added
- **75% and 50%** beside Fit in the editor: the whole image at three
  quarters or half of the canvas, centred, with a margin round it. Then
  + Background fills the margin and Overwrite writes it at the canvas size
  (area-averaged, so a shrunk logo stays crisp). For label art that runs
  to the edge.

## [0.5.1] — 2026-11-20

### Added
- **Type a colour.** The HEX / RGB / HSL readout under the swatch is now a
  field: type `#FFFFFF`, `#fff`, `rgb(255, 255, 255)`, `255 255 255`,
  `hsl(0, 0%, 100%)` or a basic name (white, black, grey, red, green,
  blue, yellow, cyan, magenta, orange) and press Enter. It becomes the
  current colour and joins History, as a pick does; anything else turns
  the field red. The app's own HEX and RGB text reads back exactly.
- **Basics**, a built-in strip after History: white, three greys, black,
  and the RGB primaries and secondaries. Always there, one click each, and
  never exported with your palettes. White for a label's background, say.

## [0.5.0] — 2026-11-20

### Added
- **A Label art scope**, a fourth button (a tag) beside folder · list ·
  disc: ndisc's `_labels` folder under the library root, read from
  published.json, one image per record label. The name filter steps aside
  there (label files are not `cover.*`) and comes back when you leave.
  Until ndisc has created `_labels` the scope says so and Run stays off.
  Every other walk now skips a folder named `_labels`, reserved for label
  art suite-wide (SUITE.md), so a library-wide Folder run never takes the
  logos for covers.

- **Batch can write into the library, keeping the originals.** A second
  Output, *Into the library*, beside *To a folder*, for Cap, Square and
  Pad: each cover is replaced where it is, in its own name and format, so
  ndisc still finds it. Its original is first copied whole into an
  originals folder you choose (remembered), at the same path as in the
  library, and every swap is logged there in `ncover-log.tsv`. The run asks
  first, and the Run button turns red while this output is on.
  - A cover whose original is already kept is refused, so a second run
    can never overwrite the first original.
  - The library always holds either the old file or the new one: the
    result is written beside it and renamed over it, and only once the
    original's copy is safe.
  - **Put back last run** restores the newest run's originals. A cover
    changed since that run is left as it is and named, with its original
    still kept.
  - The originals folder may not be the library, inside it, or above it.
  - Cover and Fit cannot go into the library: they write a PNG under
    another name.
  - Batch recounts afterwards, so the covers just fixed drop off the grid,
    and the run's results stay on screen.

  This is step 3 of the cover review loop, done until now by a desktop
  script.

### Changed
- **New app icon**, from the 2026-11-20 master.
- **Batch's column has the macOS sibling's four section titles**: What to
  run it over, Where the results go, The recipe, What it will touch.
- **Run says why it cannot go, in place.** Run and Dry run grey out, with
  the reason under them ("Choose an output folder.", an originals folder
  inside the library, a recipe that needs a colour), instead of a dialog
  when you press them.
- **Files are named by their own folder.** Under a folder scope every file
  was captioned with the scope folder's name; it now reads its folder
  under the scope ("Autechre/Amber"), and a cover in a disc subfolder
  keeps its release's name. The rule the macOS sibling already follows.

### Fixed
- **The name filter dropped every file a list names that does not contain
  "cover".** The filter (default "cover") narrows a walk of a folder, and
  it was applied to a list's named files too. Release covers are all
  `cover.*`, which hid it; ndisc's label list names files like
  `tectonic.label.jpg`, and every line was dropped. A named file is now
  taken whatever the filter says. Checked on ndisc 0.8.0's real label
  export: Square takes 5 logos and Cap 14, and the dry run agrees.
- **A list naming several files in one folder took only the last.** The
  list scope kept one named file per folder, so a folder of label logos
  with a line per label ran over one logo. Every file a line names is
  taken now, each under its own line's name, and a missing one is counted
  by itself. A folder named by several lines is one root, called by its
  own name; a line naming the folder alone still takes all of it. Present
  since the list scope (0.4.0).
- **A Batch file outside the scope's folder could be written over itself.**
  A file found outside the folder the output mirrors (possible with a
  published manifest whose release folders sit outside its library root)
  fell back to its absolute path, and an absolute path joined onto the
  output folder is the source itself. Such a file is now refused with the
  reason. Every file's place is its whole path under that folder, so two
  releases with the same title under two artists never share one.
- The `.deb` declared GTK 4.10 or newer, but since 0.4.0 the app needs 4.12
  (the Batch grid). It now requires `libgtk-4-1 (>= 4.12)`, so apt refuses
  the install on 4.10 or 4.11 instead of the app failing to start.

## [0.4.0] — 2026-11-19

### Removed
- **Preview, its twelve-thumbnail sheet and the four automatic samples.** The
  grid replaces all three: it is always current and shows every file.

### Changed
- **The Quick start is a page of its own**, behind the ⓘ button that now
  sits with Edit · Batch · Palettes. It was a small card at the top of the
  colours column; it is now a centred page of four cards (Edit, Batch,
  Colours & palettes, Formats & limits), rewritten for what the app does
  today, with the samples buttons and a "Show this page when n.cover
  starts" box under them.
- **Batch's Scope is three icon buttons** (folder · list · disc), of equal
  width, instead of a dropdown. Each says what it takes in its tooltip. The
  disc only appears when ndisc has exported a published manifest.
- **Monochrome accents.** The chosen button in a switch (Edit · Batch ·
  Palettes · ⓘ, HEX / RGB / HSL, Batch's scope) is inverted, light with
  dark text, instead of a shade darker than the rest. The green buttons
  (Copy, Pick Color, Run, Start editing, New palette) are now light with
  dark text too. Colours come from the theme, so a light theme inverts the
  other way. Overwrite stays red.
- **The Picker page is now called Edit** (Edit · Batch · Palettes). It is
  the image editor; the colour picker is a column at its side.
- Builds against gtk4's `v4_12` feature (was `v4_10`): GTK 4.12 or newer.
- **Save and Overwrite shrink smoothly.** When the framing makes the source
  smaller, the written file is area-averaged (as Cap and Square are) instead
  of keeping the nearest pixel, so fine type and halftones survive. The
  on-screen preview is unchanged. Save is now also on for a framed image
  with no build steps.

### Added
- **Pad**, a fifth batch framing: square without cropping. A non-square
  image sits in the middle of a square of its longer side, and the bars are
  filled with the **edge colour** (each bar the average of the image edge
  beside it, transparency included, so a transparent PNG logo gets
  transparent bars), white, black or your picked colour. Then as Square:
  shrunk if larger than the size, the file's format kept (JPEG stays JPEG),
  square images left alone, and the same `ncover:square` list lines, since
  cropping or padding is your call per run. For covers with art or type to
  the edge, and for label logos. The grid lists the least square first
  ("600 × 538 → 600 × 600 · 62 px added top and bottom").
- **+ Background**, a build step that puts the current colour BEHIND the
  picture: the bars Fit leaves, or a disc's transparent corners, become that
  colour, and soft edges blend into it. The canvas comes out opaque, so an
  off-square JPEG cover can be made square without cropping (Fit, pick a
  colour from the cover, + Background, Overwrite). Undo takes it back like
  any step.
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
  have squared in Edit drops off Square's list.
- **Double-click a file in the grid or the Files list to open it in Edit.**
  The grid's files become Edit's browse list, so ← and → step through
  them in the grid's order ("41 / 241 · cover.jpg"). Enter does the same.
- **Batch is a page of its own**, beside Edit and Palettes. Its controls
  and the Files list sit in a column on the left, and the grid or the last
  run's results fill the rest of the window. The colours a Colour or
  Gradient disc will use show beside the recipe.
- **The Edit page's side columns can be hidden** from the header: the
  colours column (left) and the Source / Result rail (right). Each remembers
  its state.
- **Square**, a fourth batch framing. It crops a non-square image to a
  square of its shorter side, taken from the centre, then shrinks it to the
  size if it is still larger (area averaging, as Cap). It keeps the file's
  name and format (JPEG stays JPEG at quality 92) and writes nothing for an
  image that is already square. The Run button, the grid and the dry run
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

[Unreleased]: https://github.com/xjmzx/ncover/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/xjmzx/ncover/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/xjmzx/ncover/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/xjmzx/ncover/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/xjmzx/ncover/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/xjmzx/ncover/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/xjmzx/ncover/releases/tag/v0.2.0
[0.1.1]: https://github.com/xjmzx/ncover/releases/tag/v0.1.1
[0.1.0]: https://github.com/xjmzx/ncover/releases/tag/v0.1.0
