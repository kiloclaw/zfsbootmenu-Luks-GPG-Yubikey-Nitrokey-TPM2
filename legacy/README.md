# Legacy implementation

This directory holds the implementation of ZFSBootMenu that ships today:

- `zfsbootmenu/`: the bootloader runtime, written in Bash. `make install` installs it as `/usr/share/zfsbootmenu`, and
  the dracut module and mkinitcpio hooks in `packaging/` copy it into the boot image.
- `bin/generate-zbm`: the Perl program that builds boot images; `cpanfile` lists the Perl modules it needs.
- `bin/zbm-kcl`: a Bash tool that reads and changes the kernel command lines of boot environments and EFI executables.

The Rust crates in `crates/` replace this code. The Rust `generate-zbm` and `zbm-kcl` have replaced the two tools:
`make install` and the build container use them, and `make install TOOLS=legacy` still installs the ones here. The
runtime in `zfsbootmenu/` will be replaced as a whole, after the Rust runtime has passed a beta. The parity tests of the
Rust crates run the code here and compare the results, so keep both tools working. Until then this code stays the reference: fix bugs
here, record changes to the runtime as the port roadmap asks, and run the test suites in `tests/` against it. See
[PORT-ROADMAP.md](../PORT-ROADMAP.md) and [CONTRIBUTING.md](../CONTRIBUTING.md).
