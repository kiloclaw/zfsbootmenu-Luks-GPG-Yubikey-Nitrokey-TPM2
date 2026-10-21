# Legacy implementation

This directory holds the implementation of ZFSBootMenu that ships today:

- `zfsbootmenu/`: the bootloader runtime, written in Bash. `make install` installs it as `/usr/share/zfsbootmenu`, and
  the dracut module and mkinitcpio hooks in `packaging/` copy it into the boot image.
- `bin/generate-zbm`: the Perl program that builds boot images; `cpanfile` lists the Perl modules it needs.
- `bin/zbm-kcl`: a Bash tool that reads and changes the kernel command lines of boot environments and EFI executables.

The Rust crates in `crates/` will replace this code: `generate-zbm` and `zbm-kcl` one at a time, and the runtime in
`zfsbootmenu/` as a whole, after the Rust runtime has passed a beta. Until then this code stays the reference: fix bugs
here, record changes to the runtime as the port roadmap asks, and run the test suites in `tests/` against it. See
[PORT-ROADMAP.md](../PORT-ROADMAP.md) and [CONTRIBUTING.md](../CONTRIBUTING.md).
