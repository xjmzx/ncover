# Changelog

All notable changes to **n.cover**. Versions follow [semver](https://semver.org).

The version here is n.cover's. The `xcolor` CLI in this repository keeps its
own, upstream-derived version (0.6.x) — see `make version`.

There is a sibling at [`macos-node/ncover`](https://github.com/macos-node/ncover):
a native SwiftUI app with the same name and purpose, sharing no code and free
to diverge. Its version numbers are independent of these.

## [Unreleased]

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

[Unreleased]: https://github.com/xjmzx/ncover/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/xjmzx/ncover/releases/tag/v0.2.0
[0.1.1]: https://github.com/xjmzx/ncover/releases/tag/v0.1.1
[0.1.0]: https://github.com/xjmzx/ncover/releases/tag/v0.1.0
