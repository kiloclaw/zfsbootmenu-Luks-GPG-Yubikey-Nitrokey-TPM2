# Legacy implementation

This directory holds the implementation of ZFSBootMenu that ships today:

- `zfsbootmenu/`: the bootloader runtime, written in Bash. `make install` installs it as `/usr/share/zfsbootmenu`, and
  the dracut module and mkinitcpio hooks in `packaging/` copy it into the boot image.
- `bin/generate-zbm`: the Perl program that builds boot images; `cpanfile` lists the Perl modules it needs.
- `bin/zbm-kcl`: a Bash tool that reads and changes the kernel command lines of boot environments and EFI executables.

The Rust crates in `crates/` will replace this code one part at a time. Until a Rust part matches the code it replaces,
this code stays the reference: fix bugs here, and run the test suites in `tests/` against it. See
[CONTRIBUTING.md](../CONTRIBUTING.md).
