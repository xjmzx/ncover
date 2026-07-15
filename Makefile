DESTDIR =
PREFIX = /usr/local
CARGO_FLAGS =

.PHONY: all gui install install-gui install-all uninstall help FORCE

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

install-gui: target/release/ncover
	install -s -D -m755 -- target/release/ncover "$(DESTDIR)$(PREFIX)/bin/ncover"
	install -D -m644 -- extra/io.github.xjmzx.NCover.desktop "$(DESTDIR)$(PREFIX)/share/applications/io.github.xjmzx.NCover.desktop"
	install -D -m644 -- extra/icons/ncover-16.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-24.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-32.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-48.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-256.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/ncover.png"
	install -D -m644 -- extra/icons/ncover-512.png "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/ncover.png"

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

help:
	@echo "Available make targets:"
	@echo "  all           - Build xcolor CLI (default)"
	@echo "  gui           - Build n.cover (ncover)"
	@echo "  install       - Install xcolor CLI + man + .desktop + icons"
	@echo "  install-gui   - Install n.cover binary + .desktop"
	@echo "  install-all   - install + install-gui"
	@echo "  uninstall     - Remove all installed files"
	@echo "  help          - Print this help"
