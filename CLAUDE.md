# ncover — notes for Claude

GTK4 desktop app for picking colours and building cover / label artwork. Rust ·
GTK4. The bundled `xcolor` CLI is X11-only; the app itself is not. See
[`README.md`](README.md) for the feature tour.

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
- **What macOS does lack is the screen pick.** `pick_color()` shells out to the
  `xcolor` binary, so pick-from-anywhere is Linux only and any change near it
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

## Not here

Machine-local paths, server addresses, credentials and per-box ops belong in a
machine-local `CLAUDE.md`, never in this file. **This repo is public.**
