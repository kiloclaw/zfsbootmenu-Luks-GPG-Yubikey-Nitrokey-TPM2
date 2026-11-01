# Legacy implementation

This directory holds the implementation of ZFSBootMenu that ships today:

- `zfsbootmenu/`: the bootloader runtime, written in Bash. `make install` installs it as `/usr/share/zfsbootmenu`, and
  the dracut module and mkinitcpio hooks in `packaging/` copy it into the boot image.
The Rust crates in `crates/` replace this code. The Perl `generate-zbm` and the Bash `zbm-kcl` that were here are gone:
the Rust ones replaced them, and the parity tests keep the results they recorded from them as expected results
(`tests/golden/legacy`). The runtime in `zfsbootmenu/` will be replaced as a whole; the Rust images still install its
shell libraries, recovery tools, fonts, help pages and hooks, until R6 of [ROADMAP.md](../ROADMAP.md) moves them. What
goes into an image is decided by `zbm image plan` (`crates/zbm-host/src/image`, ROADMAP.md SB8), which replaced its
`install-helpers.sh`.

Until then this code stays the reference, and the runtime is frozen: since Phase 4 of the port it gets bug fixes only,
each with a test, and every change is recorded in [RUNTIME-CHANGES.md](RUNTIME-CHANGES.md) so the Rust runtime ports it
(`scripts/dev/legacy-runtime-changes.sh` lists them). Run the test suites in `tests/` against it. See
[ROADMAP.md](../ROADMAP.md), [crates/DESIGN.md](../crates/DESIGN.md) and
[CONTRIBUTING.md](../CONTRIBUTING.md).
