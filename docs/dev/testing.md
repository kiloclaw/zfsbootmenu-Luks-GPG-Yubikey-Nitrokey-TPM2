# Testing

## The suites

| Suite | Command | Needs | Where |
| --- | --- | --- | --- |
| Rust device tests (real LUKS volumes on loop devices) | `sudo env ZBM_DEVICE_TESTS=1 cargo test -p zbm-crypt --test devices` | root, `losetup`, `cryptsetup`, a kernel with device-mapper; they skip and say why otherwise | `crates/*/tests/devices.rs` |
| Rust unit, parity and recorded-output tests | `cargo xtask test` | the Rust toolchain; Bash, Perl with `YAML::PP`, `Sort::Versions` and `boolean`, binutils and `systemd-boot-efi` for the parity tests, which skip what they cannot run | next to the code, `crates/*/tests/` |
| LUKS unit suites, legacy runtime | `tests/luks/unit/run-all.sh` | root, cryptsetup, swtpm, clevis, GnuPG, FIDO2 tools | `tests/luks/unit/` |
| LUKS unit suites, Rust runtime | `ZBMTEST_IMPL=rust tests/luks/unit/run-all.sh` | as above, and the test build of `zbm` (`cargo build -p zbm --features test-seams`) | `tests/luks/unit/` |
| Either, in the build container | `tests/luks/unit/run-in-container.sh` | podman or docker | |
| LUKS VM tests | `tests/luks/vm/run-all.sh [test ...]`; `ZBMTEST_ZBM_RUNTIME=rust` boots the Rust runtime | root, QEMU (KVM if available), the build container image | `tests/luks/vm/` |
| Upstream test VMs | `cd tests && ./setup.sh -a && ./run.sh` | root, QEMU, ZFS | `tests/README.md` |
| Same images from the Perl and Rust `generate-zbm` | `scripts/dev/compare-generators.sh` | podman or docker | |

`cargo xtask test` runs the Rust tests under [cargo-nextest](https://nexte.st) when it is installed
(`cargo install cargo-nextest --locked`), which runs each test in its own process and reports slow ones, and the
documentation tests, which nextest cannot run, under `cargo test --doc`; without it everything runs under
`cargo test`. The profiles are in `.config/nextest.toml`; CI uses `NEXTEST_PROFILE=ci`, which reports every failure
instead of stopping at the first. `cargo xtask coverage` measures line coverage per crate ([tooling.md](tooling.md)).

`runtime-rust-pilot` (`tests/luks/vm/run-all.sh runtime-rust-pilot`) runs the pilots of REFACTOR-ROADMAP.md in the
Rust image: the menu on the serial console with `TERM=vt100` and `TERM=linux`, and `zluks --pilot open` on a LUKS
volume formatted inside the VM.

[docs/luks/testing.rst](../luks/testing.rst) describes every LUKS test and its latest results. CI runs the Rust tests
and both LUKS unit suites on every pull request that touches the code they cover.

REFACTOR-ROADMAP.md moves every suite to Rust: the LUKS unit suites in phase R2, the VM harness in R3 and R4, the
rest in R7. Until then, a change to the runtime is checked by the suites above, in both runtimes where it applies.

## Writing tests

New tests follow the conventions of REFACTOR-ROADMAP.md ("Testing strategy"), which keep them easy to read for
people and for AI tools alike:

- **One behaviour per test, named as a sentence:** `a_wrong_pin_counts_towards_the_lockout`, not `test_pin_2`.
- **Arrange, act, assert,** separated by blank lines. Build the machine under test with the fakes of
  `zbm-testkit` (`FakeSystem`, `FakeFiles`, `FakeRunner`, `FakeConsole`) rather than real programs and devices.
- **Tables for variations,** with a name per case, so that a failure says which case failed.
- **Deterministic:** no wall clock, sleeping, network or unseeded randomness. Generated inputs come from a seeded
  generator; set `ZBM_TEST_SEED` to try other inputs, and quote the seed a failing test prints to reproduce it.
- **Self-contained:** a test does not depend on another test, on the host's tools, or on the order tests run in.
- **Secrets:** a test that handles a secret ends with `FakeSystem::assert_no_leak`, which fails if the secret reached
  arguments, the environment, files, logs or the screen.
- **Snapshot tests for screens:** the menu's `view` is drawn into Ratatui's `TestBackend` at 80×25, 120×40 and 52×20
  and compared with `insta` snapshots in `crates/zbm-tui/src/snapshots/`. After a deliberate change, review the new
  screens with `cargo insta review`, or accept them all with `cargo xtask bless`, which lists the `.snap` files that
  changed, and read their diff in the pull request.
- **Property tests for parsers:** `proptest` checks that parsing any text never panics and that valid values
  round-trip (`zbm-types`).
- **Recorded output over hand-written output:** parsers are tested against output recorded from the real programs
  (`crates/*/tests/fixtures/`, recorded by `scripts/dev/record-fixtures.py` and
  `scripts/dev/record-luks-fixtures.sh`).

Parity tests that run the legacy Bash or Perl code (through `zbm_testkit::legacy`) are counted by
`cargo xtask metrics` as "legacy tests". REFACTOR-ROADMAP.md replaces them with golden files recorded from the legacy
code in phase R1, so do not add new ones unless a port needs them; prefer recorded expectations.

## When a test fails

- Rust tests print the seed and, for parity tests, both outputs; rerun one test with
  `cargo nextest run -p <crate> <test name>` or `cargo test -p <crate> <test name>`.
- The LUKS unit suites keep their output in `ZBMTEST_UNIT_WORK` (default `/var/tmp/zbm-luks-unit`).
- The LUKS VM tests keep the serial console log and a step log per test below `ZBMTEST_WORK`
  (default `/var/tmp/zbm-luks-test/tests/<test>/`).
