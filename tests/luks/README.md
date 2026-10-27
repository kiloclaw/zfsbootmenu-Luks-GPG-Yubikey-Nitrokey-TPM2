# Tests of the LUKS support

- `vm/` — the files of the QEMU tests that go into images and the test boot
  environment (`be/`, `hooks/`, `functions.sh`). The tests themselves are the
  Rust harness `crates/zbm-vmtest`: pools created with ZFS 2.4.4, booted
  through ZFSBootMenu images built by the build container into a ZFS 2.4.4
  boot environment. Run `cargo xtask test vm`, or build it with
  `cargo build --release -p zbm-vmtest` and run `target/release/zbm-vmtest run`
  as root.

See `docs/luks/testing.rst` for the requirements, every test, and the results.
