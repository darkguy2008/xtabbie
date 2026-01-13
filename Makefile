PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin

all: build

build:
	cargo build --release

install: build
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 target/release/xtabbie $(DESTDIR)$(BINDIR)/xtabbie

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/xtabbie

clean:
	cargo clean

.PHONY: all build install uninstall clean
