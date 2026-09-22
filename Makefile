DESTDIR =
PREFIX = /usr/local
CARGO_FLAGS =

.PHONY: all gui icons version install install-gui install-all uninstall help FORCE

# hicolor raster sizes generated from the source SVGs.
ICON_SIZES = 16 24 32 48 256 512

# Linux icons crop the grid margin. The masters carry the art in an 824 square
# on a 1024 canvas (Apple's grid, ICONS.md), which fills 80.5% of the tile --
# visibly smaller in the dock than Yaru's own icons, which fill 89%. Cropping to
# this viewBox gets the same 89% out of the master with no re-export. The .icns
# and the .ico keep the full canvas.
LINUX_VIEWBOX ?= 49 49 926 926

all: target/release/xcolor

gui: target/release/ncover

# FORCE, because these rules have no prerequisites: once the binary exists make
# considers it up to date and `make install` silently ships a STALE build. Cargo
# does its own up-to-date check, so running it every time costs nothing.
target/release/xcolor: FORCE
	cargo build --release $(CARGO_FLAGS)

target/release/ncover: FORCE
	cargo build --release -p ncover $(CARGO_FLAGS)

FORCE:

# Regenerate the hicolor PNG raster sets (run once per icon change). Suite icon
# convention (2026-09-11): the rounded Figma exports. cover-x2 -> icon.svg
# (ncover), color-x2 -> icon-xcolor.png (xcolor). Two different sources on purpose:
#   ncover — icon.svg is a path SVG with outlined lettering. rsvg-convert
#            (librsvg2-bin) renders it faithfully. ImageMagick's `convert`
#            fallback does not render its Figma masks reliably; without
#            rsvg-convert, downscale the 2048px cover-x2.png export instead.
#   xcolor — now the same story as ncover, and the reason it once wasn't is
#            worth keeping: the old colour-wheel design was a Figma
#            *angular/conic-gradient* (foreignObject + CSS conic-gradient),
#            which neither librsvg nor ImageMagick renders — so its rasters had
#            to be downscaled from the Figma-rendered master PNG, and GTK could
#            not have used the SVG as a scalable icon either. The 2026-09-12
#            re-export replaced the wheel with a flat fill, which is ordinary
#            SVG shapes, so icon-xcolor.svg is a real source again and rsvg
#            renders it. icon-xcolor.png is kept as the Figma master and the
#            `convert` fallback below still downscales from it.
icons:
	@# ncover is Linux-only, so every raster is cropped to LINUX_VIEWBOX (89%
	@# fill, like Yaru's icons) rather than left on Apple's 80.5% grid. The
	@# source SVGs keep the grid — they are the masters.
	sed '1s|viewBox="[^"]*"|viewBox="$(LINUX_VIEWBOX)"|' icon.svg > icon-linux.svg
	sed '1s|viewBox="[^"]*"|viewBox="$(LINUX_VIEWBOX)"|' icon-xcolor.svg > icon-xcolor-linux.svg
	@for s in $(ICON_SIZES); do \
	  out="extra/icons/ncover-$$s.png"; \
	  if command -v rsvg-convert >/dev/null 2>&1; then \
	    rsvg-convert -w $$s -h $$s icon-linux.svg -o "$$out"; \
	  elif command -v convert >/dev/null 2>&1; then \
	    convert -background none -resize $${s}x$${s} icon-linux.svg "$$out"; \
	  else \
	    echo "need rsvg-convert (librsvg2-bin) or imagemagick"; exit 1; \
	  fi; \
	done; \
	echo "regenerated extra/icons/ncover-*.png from icon.svg"
	@for s in $(ICON_SIZES); do \
	  out="extra/icons/xcolor-$$s.png"; \
	  if command -v rsvg-convert >/dev/null 2>&1; then \
	    rsvg-convert -w $$s -h $$s icon-xcolor-linux.svg -o "$$out"; \
	  elif command -v convert >/dev/null 2>&1; then \
	    convert icon-xcolor.png -resize $${s}x$${s} "$$out"; \
	  else \
	    echo "need rsvg-convert (librsvg2-bin) or imagemagick"; exit 1; \
	  fi; \
	done; \
	echo "regenerated extra/icons/xcolor-*.png from icon-xcolor.svg"
	@rm -f icon-linux.svg icon-xcolor-linux.svg

## Bump n.cover's version. Two places, and it prints them.
##
## The ROOT crate is deliberately left alone. It is `xcolor` 0.6.x, a fork of
## upstream, and its version means "which xcolor this descends from" — bumping
## it because the GUI changed would misstate the lineage. Releases are tagged
## on n.cover's version; the xcolor binary rides along.
##
## awk rather than sed -i, which is spelled differently on GNU and BSD and this
## file should work on both.
version:
	@test -n "$(V)" || { echo "usage: make version V=x.y.z"; exit 1; }
	@awk -v v="$(V)" '/^version = /&&!d{print "version = \"" v "\""; d=1; next} {print}' \
	  ncover/Cargo.toml > ncover/Cargo.toml.tmp && mv ncover/Cargo.toml.tmp ncover/Cargo.toml
	@awk -v v="$(V)" '/^name = "ncover"$$/{f=1} f&&/^version = /{print "version = \"" v "\""; f=0; next} {print}' \
	  Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock
	@echo "set $(V) in:"
	@echo "  ncover/Cargo.toml"
	@echo "  Cargo.lock (ncover entry)"
	@echo "root Cargo.toml left at $$(awk '/^version = /{print $$3; exit}' Cargo.toml) — that is xcolor's, not n.cover's"
	@echo "now: update CHANGELOG.md, commit, then 'git tag v$(V) && git push --tags'"

install: target/release/xcolor
	install -s -D -m755 -- target/release/xcolor "$(DESTDIR)$(PREFIX)/bin/xcolor"
	install -D -m644 -- man/xcolor.1 "$(DESTDIR)$(PREFIX)/share/man/man1/xcolor.1"
	install -D -m644 -- extra/xcolor.desktop "$(DESTDIR)$(PREFIX)/share/applications/xcolor.desktop"
	install -D -m644 -- extra/icons/xcolor-16.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/xcolor.png"
	install -D -m644 -- extra/icons/xcolor-24.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/xcolor.png"
	install -D -m644 -- extra/icons/xcolor-32.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/xcolor.png"
	install -D -m644 -- extra/icons/xcolor-48.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/xcolor.png"
	install -D -m644 -- extra/icons/xcolor-256.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/xcolor.png"
	install -D -m644 -- extra/icons/xcolor-512.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/xcolor.png"
	@# No scalable xcolor.svg: its icon is a conic-gradient colour wheel that
	@# librsvg (GTK's SVG loader) can't render, so a scalable icon would show
	@# blank. xcolor stays PNG-only; ncover (a plain path SVG) ships scalable.
	@# Refresh desktop + icon caches on a real user install only — skip when
	@# staging into DESTDIR (packaging), where package triggers own the caches.
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(PREFIX)/share/applications" >/dev/null 2>&1 || true; \
	fi
	@if [ -z "$(DESTDIR)" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then \
		gtk-update-icon-cache -f -t "$(PREFIX)/share/icons/hicolor" >/dev/null 2>&1 || true; \
	fi

install-gui: target/release/ncover
	install -s -D -m755 -- target/release/ncover "$(DESTDIR)$(PREFIX)/bin/ncover"
	install -D -m644 -- extra/io.github.xjmzx.NCover.desktop "$(DESTDIR)$(PREFIX)/share/applications/io.github.xjmzx.NCover.desktop"
	install -D -m644 -- extra/icons/ncover-16.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-24.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-32.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-48.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-256.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-512.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/ncover.png"
	@# Linux fill: crop the grid margin on the way in (see LINUX_VIEWBOX).
	install -d "$(dir $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/ncover.svg)"
	sed '1s|viewBox="[^"]*"|viewBox="$(LINUX_VIEWBOX)"|' icon.svg > "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/ncover.svg"
	chmod 0644 "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/ncover.svg"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(PREFIX)/share/applications" >/dev/null 2>&1 || true; \
	fi
	@if [ -z "$(DESTDIR)" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then \
		gtk-update-icon-cache -f -t "$(PREFIX)/share/icons/hicolor" >/dev/null 2>&1 || true; \
	fi

install-all: install install-gui

uninstall:
	rm -f -- "$(DESTDIR)$(PREFIX)/bin/xcolor"
	rm -f -- "$(DESTDIR)$(PREFIX)/bin/ncover"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/man/man1/xcolor.1"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/applications/xcolor.desktop"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/applications/io.github.xjmzx.NCover.desktop"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/ncover.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/ncover.svg"

help:
	@echo "Available make targets:"
	@echo "  all           - Build xcolor CLI (default)"
	@echo "  gui           - Build n.cover (ncover)"
	@echo "  icons         - Regenerate hicolor PNGs from icon.svg / icon-xcolor.svg"
	@echo "  version       - Bump n.cover's version: make version V=x.y.z"
	@echo "  install       - Install xcolor CLI + man + .desktop + icons"
	@echo "  install-gui   - Install n.cover binary + .desktop"
	@echo "  install-all   - install + install-gui"
	@echo "  uninstall     - Remove all installed files"
	@echo "  help          - Print this help"
