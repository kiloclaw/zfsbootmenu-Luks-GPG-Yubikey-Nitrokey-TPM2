# Notes for Claude

## Long tests run on the maintainer's machine

The maintainer runs the long test suites locally, with KVM. Do not run them in a cloud session:

- the LUKS VM tests (`tests/luks/vm/run-all.sh`);
- the Bash LUKS unit suites (`tests/luks/unit/run-all.sh`, with and without `ZBMTEST_IMPL=rust`).

Run the quick checks yourself: `cargo xtask lint`, `cargo xtask test`, coverage, metrics, parity and
`cargo xtask test devices`.

When a change needs a long run, push it first, then tell the maintainer:

- the commit to check out;
- the exact commands, split into parallel lanes for a long run (groups are independent; within a group the
  `-setup` test comes first);
- whether `ZBMTEST_REUSE=1` is safe, or the test images must be rebuilt because code in the images changed;
- anything to install or rebuild first, such as the build container image when `packaging/container/` changed
  (`sudo podman build -t localhost/zbm-builder packaging/container`).
