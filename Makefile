DESTDIR=
PREFIX=/usr
CONFDIR=/etc/zfsbootmenu
MODDIR=$(PREFIX)/share
DRACUTDIR=$(PREFIX)/lib/dracut/modules.d
INITCPIODIR=$(PREFIX)/lib/initcpio
MANDIR=$(PREFIX)/share/man
BINDIR=$(PREFIX)/bin
EXAMPLES=$(PREFIX)/share/examples/zfsbootmenu

.PHONY: install core dracut initcpio zbm-release show-version

zbm-release:
	./scripts/version.sh -u

install: core dracut initcpio zbm-release

core: zbm-release
	./scripts/install-tree.sh legacy/zfsbootmenu "$(DESTDIR)/$(MODDIR)/zfsbootmenu"
	install -m 0644 -t "$(DESTDIR)/$(CONFDIR)" -D config/zfsbootmenu/config.yaml
	install -m 0755 -t "$(DESTDIR)/$(BINDIR)" -D legacy/bin/*
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
