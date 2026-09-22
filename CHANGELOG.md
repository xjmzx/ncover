# Changelog

All notable changes to **n.cover**. Versions follow [semver](https://semver.org).

The version here is n.cover's. The `xcolor` CLI in this repository keeps its
own, upstream-derived version (0.6.x) — see `make version`.

There is a sibling at [`macos-node/ncover`](https://github.com/macos-node/ncover):
a native SwiftUI app with the same name and purpose, sharing no code and free
to diverge. Its version numbers are independent of these.

## [Unreleased]

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

[Unreleased]: https://github.com/xjmzx/ncover/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/xjmzx/ncover/releases/tag/v0.1.0
