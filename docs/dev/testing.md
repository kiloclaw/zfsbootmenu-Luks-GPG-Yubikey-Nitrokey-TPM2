# Testing

## The suites

| Suite | Command | Needs | Where |
| --- | --- | --- | --- |
| LUKS device suites (the LUKS code against real LUKS volumes on loop devices) | `cargo xtask test devices` (as root) | root, `losetup`, `cryptsetup`, `swtpm`, `systemd-cryptenroll`, clevis, GnuPG, `sfdisk`, `mkfs.ext4`; the build container has them | `crates/zbm-luks/tests/devices/` |
| `zbm-crypt` device tests (its adapters against real LUKS volumes) | `sudo env ZBM_DEVICE_TESTS=1 cargo test -p zbm-crypt --test devices` | root, `losetup`, `cryptsetup`, a kernel with device-mapper; they skip and say why otherwise | `crates/zbm-crypt/tests/devices.rs` |
| Rust unit, parity and recorded-output tests | `cargo xtask test` | the Rust toolchain; Bash, Perl with `YAML::PP`, `Sort::Versions` and `boolean`, binutils and `systemd-boot-efi` for the parity tests: under `cargo xtask test` (`ZBM_REQUIRE_TOOLS=1`) a missing tool fails them; under plain `cargo test` they skip and say so | next to the code, `crates/*/tests/` |
| LUKS unit suites, legacy runtime | `tests/luks/unit/run-all.sh` | root, cryptsetup, swtpm, clevis, GnuPG, FIDO2 tools | `tests/luks/unit/` |
| LUKS unit suites, Rust runtime | `ZBMTEST_IMPL=rust tests/luks/unit/run-all.sh` | as above, and the test build of `zbm` (`cargo build -p zbm --features test-seams`) | `tests/luks/unit/` |
| Either, in the build container | `tests/luks/unit/run-in-container.sh` | podman or docker | |
| LUKS VM tests | `cargo xtask test vm [test ...]`, or `zbm-vmtest run [test ...]` as root after `cargo build --release -p zbm-vmtest`; `ZBMTEST_ZBM_RUNTIME=rust` boots the Rust runtime | root, QEMU (KVM if available), the build container image | `crates/zbm-vmtest/`, guest files in `tests/luks/vm/` |
| Upstream test VMs | `cd tests && ./setup.sh -a && ./run.sh` | root, QEMU, ZFS | `tests/README.md` |
| Same images from the Perl and Rust `generate-zbm` | `scripts/dev/compare-generators.sh` | podman or docker | |

`cargo xtask test` runs the Rust tests under [cargo-nextest](https://nexte.st) when it is installed
(`cargo install cargo-nextest --locked`), which runs each test in its own process and reports slow ones, and the
documentation tests, which nextest cannot run, under `cargo test --doc`; without it everything runs under
`cargo test`. The profiles are in `.config/nextest.toml`; CI uses `NEXTEST_PROFILE=ci`, which reports every failure
instead of stopping at the first. `cargo xtask coverage` measures line coverage per crate ([tooling.md](tooling.md)).

`runtime-rust-pilot` (`zbm-vmtest run runtime-rust-pilot`) runs the pilots of REFACTOR-ROADMAP.md in the
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
  `zbm-testkit` rather than real programs and devices. `FakeMachine` (`zbm_testkit::machine`) describes the disks in
  one expression, and its `cryptsetup`, `blkid`, `lsblk` and `zpool` answer from them:

  ```rust
  let machine = FakeMachine::new()
      .with_luks_volume("/dev/sda2", "pw")
      .with_pool("tank", "/dev/mapper/cryptroot")
      .typing("pw");
  ```

  It dereferences to a `FakeSystem` (`FakeFiles`, `FakeRunner`, `FakeConsole` and the others), so anything a
  `FakeSystem` test does works on it too.
- **Readable failures:** `assert_ran!(machine, "zpool import -N -a")` and `assert_not_ran!(machine, "zpool export")`
  list every command that ran when they fail.
- **Tables for variations,** with a name per case, so that a failure says which case failed. `cases!` runs every
  case and names each one that fails, not only the first:

  ```rust
  cases! {
      for (text, expected) in {
          "empty" => ("", Reason::Empty),
          "digit first" => ("1tank", Reason::FirstNotLetter),
      } {
          assert_eq!(pool_reason(text), expected);
      }
  }
  ```
- **Deterministic:** no wall clock, sleeping, network or unseeded randomness. Generated inputs come from a seeded
  generator; set `ZBM_TEST_SEED` to try other inputs, and quote the seed a failing test prints to reproduce it.
- **Self-contained:** a test does not depend on another test, on the host's tools, or on the order tests run in.
- **Secrets:** a test that handles a secret ends with `assert_no_secret_leaked!(machine, b"pw")`
  (`FakeSystem::assert_no_leak`), which fails if the secret reached arguments, the environment, plain standard input,
  files (the kernel log and the journal among them) or the screen.
- **Snapshot tests for screens:** the menu's `view` is drawn into Ratatui's `TestBackend` at 80×25, 120×40 and 52×20
  and compared with `insta` snapshots in `crates/zbm-tui/src/snapshots/`. After a deliberate change, review the new
  screens with `cargo insta review`, or accept them all with `cargo xtask bless`, which lists the `.snap` files that
  changed, and read their diff in the pull request.
- **Property tests for parsers:** `proptest` checks that parsing any text never panics and that valid values
  round-trip (`zbm-types`).
- **Recorded output over hand-written output:** parsers are tested against output recorded from the real programs
  (`crates/*/tests/fixtures/`). `cargo xtask record-zfs` records what `zfs` and `zpool` print, in JSON and in text,
  into `crates/zbm-zfs/tests/fixtures/<major.minor>/`: it runs the commands of `xtask/src/record_zfs.rs` in a test VM
  (scenario `runtime-record-zfs`, root and QEMU). The older recordings in `crates/zbm-zfs/tests/fixtures/` (by
  `zbm-vmtest run runtime-record-fixtures`) serve the legacy-compatible `ZfsCommand` until it goes;
  `scripts/dev/record-luks-fixtures.sh` records `cryptsetup` output. `crates/zbm-boot/tests/fixtures/uutils-sort-v.txt`
  is uutils' `sort -V` order of generated kernel names, which kernel ordering must match; where the host's `sort` is
  uutils (the build container) the test checks the recording against it, and `ZBM_RECORD_UUTILS_ORDER=1` rewrites
  it there.
- **Fakes of the adapters:** code that asks ZFS questions takes a `&dyn zbm_zfs::Zfs` and is tested with
  `zbm_testkit::zfs::FakeZfs`, an in-memory model of pools, datasets and snapshots built with builders; it records each
  call (`FakeZfs::calls`).

### Parity tests and recorded legacy results

A parity test gives the same input to a legacy function and to its Rust counterpart and compares the results. The
legacy half runs inside `parity.legacy(case, || ...)` (`zbm_testkit::parity`), and its result is recorded in
`tests/golden/legacy/<crate>/<module>/<test>.json`. A normal test run reads the recording and never runs Bash, Perl or
`legacy/`; the helpers of `zbm_testkit::legacy` panic outside a recording. `ZBM_PARITY` chooses:

| `ZBM_PARITY` | What happens |
| --- | --- |
| `replay` (default) | The recorded results are used; a case that was not recorded, or whose input changed, fails |
| `record` | The legacy code runs and the files are written: `cargo xtask record-parity` |
| `check` | The legacy code runs and must give the recorded results: `cargo xtask record-parity --check`, and `cargo xtask coverage` |
| `live` | The legacy code runs and the files are left alone; the default when `ZBM_TEST_SEED` is set, since other seeds generate inputs that were never recorded |

Rules for a parity test:

- Build the inputs (generated values, directories, fake pools) outside the closure, so that both modes see the same
  ones, and describe the case with them: the description is checked against the recording.
- Return only what the comparison needs, and nothing that depends on the machine: write temporary directories as
  `$DIR` or `$BASE`, describe EFI images with `zbm_testkit::efi::describe`, and leave out the environment.
- Skip a test for a missing tool only when `parity.mode().needs_legacy()`, unless the Rust code needs the tool too.
- Never name a helper or a variable `perl`, or a helper `legacy`: `cargo xtask metrics` counts tests that mention
  the legacy helpers outside a recording as "legacy tests", which must stay at zero.

Recording needs what the legacy code needs (Bash, Perl with its modules, binutils, the systemd EFI stub) and root for
the cases that compare with it as root. REFACTOR-ROADMAP.md lists behaviour changes that update a recording on
purpose.

### Fuzz targets

`fuzz/` holds a fuzz target for each group of parsers of untrusted input in [threat-model.md](threat-model.md), which
names the threat each one covers. A target feeds arbitrary bytes to the parsers and fails on a panic or a hang.

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cargo xtask fuzz                          # every target, 60 seconds each
cargo xtask fuzz crypttab --seconds 600   # one target
```

Each run starts from the seeds in `fuzz/seeds/<target>` (committed) and grows a corpus in `fuzz/corpus/<target>`
(not committed). An input that makes a target fail is written to `fuzz/artifacts/<target>/`; replay it with
`cargo +nightly fuzz run <target> <file>`, fix the parser, and add the input to a unit test of it. CI runs every
target for five minutes each night (`.github/workflows/fuzz.yml`), and `cargo xtask lint` checks that the targets
build, so a parser that moves takes its target along.

## When a test fails

- Rust tests print the seed and, for parity tests, both outputs; rerun one test with
  `cargo nextest run -p <crate> <test name>` or `cargo test -p <crate> <test name>`.
- The LUKS unit suites keep their output in `ZBMTEST_UNIT_WORK` (default `/var/tmp/zbm-luks-unit`).
- The LUKS VM tests keep the serial console log and a step log per test below `ZBMTEST_WORK`
  (default `/var/tmp/zbm-luks-test/tests/<test>/`).
- A Rust runtime program that panicked left a crash report: `zbm debug crash` in the recovery shell prints it, and
  `cargo xtask symbolize <report> --debug-dir <image build>/build/debug` names the functions and lines
  ([tooling.md](tooling.md), "Crash reports"). The VM test `runtime-rust-crash` checks that whole way.
- A Rust image booted with `zbm.save_logs` kept each boot's journal, crash reports and kernel log on the EFI system
  partition, below `EFI/zfsbootmenu/logs/`; `zbm debug saved` lists them, and `zbm report` writes a bug report
  without key material ([tooling.md](tooling.md), "Saved logs and bug reports"). The VM test `runtime-rust-saved`
  checks both.
