# ncover — notes for Claude

GTK4 desktop app for picking colours and building cover / label artwork. Rust ·
GTK4. The bundled `xcolor` CLI is X11-only; the app itself is not. See
[`README.md`](README.md) for the feature tour.

## There is a macOS sibling, and it is a different app

[`macos-node/ncover`](https://github.com/macos-node/ncover) is a native SwiftUI
app with the same name and the same purpose. It is **not a port of this one and
shares no code**, by decision: the two are expected to develop differently, and
a shared core is exactly the coupling that would prevent that.

What that means here: **do not grow a macOS story in this repo.** No `.app`
bundle target, no Cocoa branch, no `cfg(target_os = "macos")` UI path. Work for
the Mac belongs over there.

That this app *does* build and run on macOS (see the traps below) is still
useful, because it makes this repo the **oracle**. The Swift side ports tests
from here keeping their original names and expected pixel values, so wherever
both still claim to do the same thing, both must agree — and when the macOS app
diverges on purpose, it deletes the test rather than weakening it.

## Read SUITE.md first

[`../ndisc/SUITE.md`](https://github.com/xjmzx/ndisc/blob/main/SUITE.md) is
authoritative for anything shared across the suite — but **read it here knowing
that most of it does not apply**. This is the one app that is not Tauri, not
React, has no webview, no Nostr surface, no keyring and no database. What it
shares with the suite is the brand and colour language, and being a consumer of
what `ndisc` publishes.

That makes the usual reflex backwards here. A pattern proven in `ndisc` or
`ntree` — `tauri build`, `make dev`, design tokens as CSS variables, the
`cfg(debug_assertions)` dev/install split — has no equivalent in this repo, and
reaching for one will waste time.

## Build and verify

```
make gui                              # build target/release/ncover (the GTK4 app)
make install-gui PREFIX=$HOME/.local  # user install: ~/.local/bin + .desktop
make install-all                      # also installs the xcolor CLI, man page, icons
```

`make install-all` defaults to `PREFIX=/usr/local` and therefore wants root.
Pass `PREFIX=$HOME/.local` for a user install, as the rest of the suite does.

`make gui` and `cargo test -p ncover` are the cross-platform pair. The
`install-*` targets are not: they ship hicolor PNGs, a scalable SVG and a
`.desktop` file, none of which mean anything off Linux. There is no `.app`
bundle target, so a macOS build is run from `target/release/ncover`.

## The shape: a composition, not a pipeline

The open document is a **`Composition`** (`ncover/src/composition.rs`): a
source plus an ordered list of **`Operation`**s. `render_raster` folds the
list into a canvas, and that one render is what the preview draws and what
Save / Overwrite write. Adding a capability means adding an `Operation` case
and its arm in `render_raster`, not threading new state through the UI.

- **Every edit goes through `mutate`** in `build_image_view`: clone the
  current value, edit the copy, hand it over. It renders, and only a render
  that succeeded becomes the document. Anything that writes `doc` directly is
  a bug: the preview will not follow it, and once undo exists (a stack of
  these values) it will not be undoable.
- **`result` is a cache, not state.** It holds `render_raster(doc)` while the
  document has build steps, and only `mutate` writes it. With no steps the
  canvas draws the placed source live through Cairo instead, which keeps drag
  and zoom cheap.
- **The framing freezes once a step is added** (`has_build`), as it always
  has: the steps were judged against that framing. Reset (`clear_build`)
  unfreezes it. A size change rescales the framing and drops the steps
  (`set_canvas`).
- **Undo is `History`, two stacks of `Composition` values** (in
  `composition.rs`, with its own tests). `mutate(next, group)` records the
  document it replaces; `group` names a continuous gesture (`"drag"`,
  `"zoom"`), and edits in the same group less than `COALESCE` apart are one
  step. Drag-begin calls `break_group`, so two drags are two steps. A no-op
  edit (`prev == next`) records nothing. Opening an image calls
  `show_doc` and clears the history: a new document, not an edit.
- **`show_doc` puts a document on screen without recording it.** Undo and
  redo use it. It also moves the size picker and `canvas` to the document's
  size, setting `canvas` FIRST so the picker's handler sees no change and
  does not record a second edit.
- **Ctrl+Z is a bubble-phase `ShortcutController` on the window, not
  `set_accels_for_action`.** Application accels fire before the focused
  widget, so Ctrl+Z typed into the batch filter undid a canvas step instead
  of the typing. Bubbling lets a text field handle its own undo first.
- **The backdrop is preview-only and must stay that way.** `paint_backdrop`
  is called by the canvas and the Result thumbnail, never by a renderer, and
  it is not part of `Composition`, so undo leaves it alone. Anything that
  composites it into `render_raster` is a data bug: the backdrop exists to
  show what the transparent pixels will sit on, which only works if they stay
  transparent in the file.
- **Do not hold a `RefCell` borrow across `mutate`.** Read the value into a
  local first (`let next = doc.borrow().clone();`), then call. The size
  handler used to write `*place.borrow_mut() = place.borrow()…` in one
  statement, and the right-hand side's borrow lives to the end of the
  statement: a panic that aborted the app on every size change.

The macOS sibling has the same shape (`Composition.swift`). That is a shared
idea, not shared code, and `docs/macos-parity-plan.md` is the plan for
porting further features across.

## Batch: the count is a promise

- **There is one walk, `walk_scope`** (`batch.rs`). `find_in_scope` (run,
  dry run, preview) and `survey_batch` (the "37 images in 35 releases" line
  and "Run (N files)") both read it. Do not walk a scope anywhere else: the
  number on the Run button is only true because the survey and the run
  cannot select different files. A test ties the two counts together.
- **The survey and the sample are driven by a 300 ms ticker, not by signal
  handlers.** Every input goes into a key (`scope_key`, and the sample key =
  scope key + `recipe_now()`), and a key that has held for one tick is
  applied. Adding an input to the recipe means making it part of the key;
  wiring a `connect_*` handler instead will miss the picked colours, which
  change in another part of the window and fire nothing here. Stale results
  are dropped by generation counters, so a slow walk never overwrites a
  newer one.
- **`recipe_now` / `scope_now` say why not as a string; `resolve_*` turn
  that into a dialog.** The ticker uses the silent pair. Never let the
  ticker show a dialog: it runs every 300 ms.
- The ticker does nothing while Batch is collapsed or a run is in flight.

## Traps specific to this repo

- **The root crate is `xcolor`, not `ncover`.** This repo is a fork of the
  `xcolor` X11 picker, and the GUI is a workspace member under `ncover/`. A bare
  `cargo build --release` at the root builds the **CLI**, not the app; the app
  is `make gui`. The `[workspace] members = ["ncover"]` line in the root
  `Cargo.toml` is the thing to read before running any cargo command here.
- **X11 is the CLI, not the app — do not generalise one to the other.** The
  root `xcolor` crate links `xlib` and `xcb`, so *it* is X11 and therefore
  Linux only, with no Wayland path. The GTK4 app is neither: `make gui` is
  `cargo build -p ncover`, which never compiles the X11 crates at all. The app
  builds, tests (45 passing) and runs on macOS **unmodified** against Homebrew
  `gtk4`, `gdk-pixbuf` and `librsvg` — verified 2026-09-22. This file used to
  claim the opposite; it was reasoning from the root crate.
- **The screen pick has two backends (2026-10-01).** On Wayland
  `pick_color()` goes through the Screenshot portal's `PickColor` (ashpd,
  `async-io`, driven by `glib::spawn_future_local`; a Linux-only target
  dependency so macOS still builds). `xcolor` cannot work there: rootless
  XWayland answers its root-window `GetImage` with BadMatch. On an X11
  session it still shells out to `xcolor`.
- **What macOS does lack is the screen pick.** Both backends are Linux
  only, so pick-from-anywhere is Linux only and any change near it
  has to be settled on the Linux box. The *in-app* eyedropper — click a pixel
  on a loaded image, feeding history and palettes identically — is plain
  gdk-pixbuf and works everywhere, so colour-from-artwork is fully functional
  off Linux.
- **Two writing modes with different safety.** *Batch* never writes over its
  source, but *Overwrite* replaces the original PNG in place. They are one click
  apart in the UI. Treat any change near the write path as touching user data,
  and keep the Preview contact sheet and dry run working — they are the guard.
- **It consumes `ndisc` output.** Batch can run a recipe over an ndisc-published
  discography, which makes it a downstream reader of the publish manifest. It
  publishes nothing itself and holds no keys.
- **Output is always PNG.** Not a preference — the disc / label mask needs an
  alpha channel.
- **Upstream lineage is real.** Forked from `xcolor` and MIT-licensed;
  CLI-side changes may have an upstream counterpart worth checking rather than
  reinventing.

## Releasing

`make version V=x.y.z`, update `CHANGELOG.md`, commit, tag `vx.y.z`, push the
tag. The workflow builds on **ubuntu-24.04** — 22.04 ships GTK 4.6 and the GUI
asks for gtk4's `v4_10` feature, so it fails there at compile time.

**The build needs `libx11-xcb-dev`**, which is easy to miss because it is not
part of `libx11-dev`. The `xcb` crate's `xlib_xcb` feature links `-lX11-xcb`,
and without it the CLI fails at link time with `unable to find library
-lX11-xcb` after compiling cleanly. Its runtime counterpart `libx11-xcb1`
belongs in the package's `Depends`.

The `.deb` is staged with the Makefile's own `install-all DESTDIR=… PREFIX=/usr`
rather than a reimplementation, which is also why those targets skip the desktop
and icon cache updates when `DESTDIR` is set.

**`make version` bumps n.cover, not the root crate.** The root is `xcolor`
0.6.x, and its version means "which upstream xcolor this descends from" —
bumping it because the GUI changed would misstate the lineage.

## Not here

Machine-local paths, server addresses, credentials and per-box ops belong in a
machine-local `CLAUDE.md`, never in this file. **This repo is public.**
