PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
MANDIR = $(PREFIX)/share/man/man1

all: build

build:
	cargo build --release

install: build
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 target/release/xtabbie $(DESTDIR)$(BINDIR)/xtabbie
	install -d $(DESTDIR)$(MANDIR)
	install -m 644 xtabbie.1 $(DESTDIR)$(MANDIR)/xtabbie.1
	gzip -f $(DESTDIR)$(MANDIR)/xtabbie.1

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/xtabbie
	rm -f $(DESTDIR)$(MANDIR)/xtabbie.1.gz

clean:
	cargo clean

.PHONY: all build install uninstall clean
