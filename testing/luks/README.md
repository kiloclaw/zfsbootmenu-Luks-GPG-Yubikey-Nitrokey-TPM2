# Tests of the LUKS support

- `unit/` — shell suites that run ZFSBootMenu's LUKS code against real LUKS
  headers on loop devices, a software TPM, clevis and GnuPG. Run
  `unit/run-all.sh` as root, or `unit/run-in-container.sh` to use the tool
  versions of the build container.
- `vm/` — QEMU tests: pools created with ZFS 2.4.4, booted through ZFSBootMenu
  images built by the build container into a ZFS 2.4.4 boot environment. Run
  `vm/run-all.sh` as root.

See `docs/luks/testing.rst` for the requirements, every test, and the results.
