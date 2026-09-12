DESTDIR =
PREFIX = /usr/local
CARGO_FLAGS =

.PHONY: all gui icons install install-gui install-all uninstall help FORCE

# hicolor raster sizes generated from the source SVGs.
ICON_SIZES = 16 24 32 48 256 512

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
	@for s in $(ICON_SIZES); do \
	  out="extra/icons/ncover-$$s.png"; \
	  if command -v rsvg-convert >/dev/null 2>&1; then \
	    rsvg-convert -w $$s -h $$s icon.svg -o "$$out"; \
	  elif command -v convert >/dev/null 2>&1; then \
	    convert -background none -resize $${s}x$${s} icon.svg "$$out"; \
	  else \
	    echo "need rsvg-convert (librsvg2-bin) or imagemagick"; exit 1; \
	  fi; \
	done; \
	echo "regenerated extra/icons/ncover-*.png from icon.svg"
	@for s in $(ICON_SIZES); do \
	  out="extra/icons/xcolor-$$s.png"; \
	  if command -v rsvg-convert >/dev/null 2>&1; then \
	    rsvg-convert -w $$s -h $$s icon-xcolor.svg -o "$$out"; \
	  elif command -v convert >/dev/null 2>&1; then \
	    convert icon-xcolor.png -resize $${s}x$${s} "$$out"; \
	  else \
	    echo "need rsvg-convert (librsvg2-bin) or imagemagick"; exit 1; \
	  fi; \
	done; \
	echo "regenerated extra/icons/xcolor-*.png from icon-xcolor.svg"

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
	install -D -m644 -- icon.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/ncover.svg"
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
	@echo "  install       - Install xcolor CLI + man + .desktop + icons"
	@echo "  install-gui   - Install n.cover binary + .desktop"
	@echo "  install-all   - install + install-gui"
	@echo "  uninstall     - Remove all installed files"
	@echo "  help          - Print this help"
