DESTDIR =
PREFIX = /usr/local
CARGO_FLAGS =

.PHONY: all gui install install-gui install-all uninstall help FORCE

all: target/release/xcolor

gui: target/release/xcolor-gui

# FORCE, because these rules have no prerequisites: once the binary exists make
# considers it up to date and `make install` silently ships a STALE build. Cargo
# does its own up-to-date check, so running it every time costs nothing.
target/release/xcolor: FORCE
	cargo build --release $(CARGO_FLAGS)

target/release/xcolor-gui: FORCE
	cargo build --release -p xcolor-gui $(CARGO_FLAGS)

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

install-gui: target/release/xcolor-gui
	install -s -D -m755 -- target/release/xcolor-gui "$(DESTDIR)$(PREFIX)/bin/xcolor-gui"
	install -D -m644 -- extra/io.github.xjmzx.XColorGui.desktop "$(DESTDIR)$(PREFIX)/share/applications/io.github.xjmzx.XColorGui.desktop"

install-all: install install-gui

uninstall:
	rm -f -- "$(DESTDIR)$(PREFIX)/bin/xcolor"
	rm -f -- "$(DESTDIR)$(PREFIX)/bin/xcolor-gui"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/man/man1/xcolor.1"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/applications/xcolor.desktop"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/applications/io.github.xjmzx.XColorGui.desktop"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/16x16/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/24x24/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/48x48/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/xcolor.png"
	rm -f -- "$(DESTDIR)$(PREFIX)/share/icons/hicolor/512x512/apps/xcolor.png"

help:
	@echo "Available make targets:"
	@echo "  all           - Build xcolor CLI (default)"
	@echo "  gui           - Build xcolor-gui"
	@echo "  install       - Install xcolor CLI + man + .desktop + icons"
	@echo "  install-gui   - Install xcolor-gui binary + .desktop"
	@echo "  install-all   - install + install-gui"
	@echo "  uninstall     - Remove all installed files"
	@echo "  help          - Print this help"
