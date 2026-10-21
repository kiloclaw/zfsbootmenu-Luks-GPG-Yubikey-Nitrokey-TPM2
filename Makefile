DESTDIR=
PREFIX=/usr
CONFDIR=/etc/zfsbootmenu
MODDIR=$(PREFIX)/share
DRACUTDIR=$(PREFIX)/lib/dracut/modules.d
INITCPIODIR=$(PREFIX)/lib/initcpio
MANDIR=$(PREFIX)/share/man
BINDIR=$(PREFIX)/bin
LIBEXECDIR=$(PREFIX)/libexec/zfsbootmenu
EXAMPLES=$(PREFIX)/share/examples/zfsbootmenu

# The Rust programs: zbm, the runtime that boot images include, and the host
# tools generate-zbm and zbm-kcl. Their dependencies are vendored, so the build
# needs no network.
CARGO=cargo
CARGO_TARGET_DIR=target
CARGO_FLAGS=--release --locked
RUST_PROGRAMS=zbm generate-zbm zbm-kcl

# Which host tools "make install" installs: "rust" (the default) or "legacy",
# the Perl generate-zbm and Bash zbm-kcl in legacy/bin, which need no Rust
# toolchain; images built without zbm use the legacy runtime as before
TOOLS=rust

.PHONY: all rust install core tools-rust tools-legacy dracut initcpio zbm-release show-version

all: rust

rust:
	$(CARGO) build $(CARGO_FLAGS) --target-dir "$(CARGO_TARGET_DIR)" $(addprefix --bin ,$(RUST_PROGRAMS))

zbm-release:
	./scripts/version.sh -u

install: core tools-$(TOOLS) dracut initcpio zbm-release

tools-rust: zbm-release rust
	install -m 0755 -D "$(CARGO_TARGET_DIR)/release/generate-zbm" "$(DESTDIR)/$(BINDIR)/generate-zbm"
	install -m 0755 -D "$(CARGO_TARGET_DIR)/release/zbm-kcl" "$(DESTDIR)/$(BINDIR)/zbm-kcl"
	install -m 0755 -D "$(CARGO_TARGET_DIR)/release/zbm" "$(DESTDIR)/$(LIBEXECDIR)/zbm"

tools-legacy: zbm-release
	install -m 0755 -t "$(DESTDIR)/$(BINDIR)" -D legacy/bin/*

core: zbm-release
	./scripts/install-tree.sh legacy/zfsbootmenu "$(DESTDIR)/$(MODDIR)/zfsbootmenu"
	install -m 0644 -t "$(DESTDIR)/$(CONFDIR)" -D config/zfsbootmenu/config.yaml
	install -m 0644 -t "$(DESTDIR)/$(MANDIR)/man5" -D docs/man/dist/man5/*.5
	install -m 0644 -t "$(DESTDIR)/$(MANDIR)/man7" -D docs/man/dist/man7/*.7
	install -m 0644 -t "$(DESTDIR)/$(MANDIR)/man8" -D docs/man/dist/man8/*.8
	install -m 0755 -t "$(DESTDIR)/$(EXAMPLES)/hooks" -D contrib/*
	install -m 0755 -t "$(DESTDIR)/$(EXAMPLES)" -D examples/*
	install -m 0644 -t "$(DESTDIR)/$(EXAMPLES)" -D assets/splash.bmp

dracut:
	./scripts/install-tree.sh packaging/dracut "$(DESTDIR)/$(DRACUTDIR)/90zfsbootmenu"
	install -m 0644 -t \
		"$(DESTDIR)/$(CONFDIR)/dracut.conf.d/" \
		-D config/zfsbootmenu/dracut.conf.d/*

initcpio:
	./scripts/install-tree.sh packaging/initcpio "$(DESTDIR)/$(INITCPIODIR)"
	install -m 0644 -t "$(DESTDIR)/$(CONFDIR)" -D config/zfsbootmenu/mkinitcpio.conf

show-version:
	@ ./scripts/version.sh
