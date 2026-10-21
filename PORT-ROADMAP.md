# Rust port roadmap

This roadmap plans the port of ZFSBootMenu from Bash and Perl to Rust, in eight phases. Its goal is **parity**: the
Rust version does what the legacy code in `legacy/` does today, nothing more. New features are listed separately in
[ROADMAP.md](ROADMAP.md). [CONTRIBUTING.md](CONTRIBUTING.md) explains how to build and test.

## Contents

- [Approach](#approach)
- [End state](#end-state)
- [Rules for every phase](#rules-for-every-phase)
- [Rust unit tests](#rust-unit-tests)
- [Compatibility contract](#compatibility-contract)
- [Overview](#overview)
- [Status](#status)
- Phases: [1](#phase-1-foundations) · [2](#phase-2-shared-libraries-and-zbm-kcl) · [3](#phase-3-generate-zbm) ·
  [4](#phase-4-runtime-design) · [5](#phase-5-minimal-bootable-runtime) · [6](#phase-6-complete-the-runtime) ·
  [7](#phase-7-beta-with-the-legacy-image-as-fallback) · [8](#phase-8-cutover-and-removal-of-the-legacy-code)
- [Alternatives](#alternatives)
- [Out of scope](#out-of-scope)

## Approach

The port mixes two strategies, because the code has two kinds of parts:

- **Host tools are replaced one at a time.** `zbm-kcl` and `generate-zbm` are standalone programs. Each Rust version
  replaces the legacy one as soon as it passes the same tests, at no extra cost, and users get Rust code early.
- **The boot runtime is ported as a whole.** The Bash code in the image shares state through files under
  `/zfsbootmenu` and global variables, and `fzf` calls back into scripts that reload the libraries. Replacing it piece
  by piece would force the Rust code to copy those interfaces until the end. Instead, the Rust runtime is designed as
  one program and built next to the legacy one, starting with a version that can only boot the default boot
  environment. From then on the VM tests run against it every day, and the number that pass measures progress.
- **The legacy image stays as a fallback.** During a beta, the Rust image is the first boot entry and the legacy image
  the second, so a faulty Rust build never leaves a machine unbootable.

| | Fully phased | **Mixed (this plan)** | Everything in one go |
| --- | --- | --- | --- |
| Host tools | One at a time | One at a time | At the end |
| Boot runtime | One part at a time, Bash and Rust calling each other | As a whole, from a minimal bootable version | As a whole |
| First Rust code in use | After about 2 months | After about 2 months | At the end |
| Glue between Bash and Rust | 1–2 months of work, discarded at the end | None: tools are swapped whole, the runtime is switched whole | None |
| First full boot of the Rust runtime | Every phase, with part of the runtime in Rust | Phase 5, after 4 to 5 months | At the end |
| Total, one developer | 9–13 months | 9–13 months | 8–12 months |

The mixed plan takes about as long as the fully phased one: the glue it saves pays for a design phase and a beta. What
it buys is a runtime designed as a whole instead of shaped by the Bash code, and no throwaway glue. The fully phased
plan remains the better choice for a single developer who wants a shippable result at every step; see
[Alternatives](#alternatives).

## End state

- **In the boot image,** one Rust program, `zbm`, replaces the ZFSBootMenu code in `legacy/zfsbootmenu/`: the
  libraries, the start-up sequence, the menu, the LUKS code and the recovery shell commands. Like BusyBox, it is a
  single binary that acts as each command through its name or a subcommand, which keeps the image small.
- **On the host,** Rust versions of `generate-zbm` (Perl today) and `zbm-kcl` (Bash today) replace the legacy tools.
- **Bash stays in the image** for the recovery shell and for user hooks, which are shell scripts. The menu keeps using
  `fzf`, as it does today; a built-in menu is a new feature (ROADMAP.md #12).
- **External programs stay the same at first:** `zfs`, `zpool`, `cryptsetup`, `kexec`, `gpg`, `clevis`, `tpm2-tools`,
  `fido2-assert`, `ykchalresp`. A program is replaced by a library only when the tests show the same behaviour.
- **Packaging stays shell:** the dracut module and the mkinitcpio hooks must be shell scripts, so they remain, but only
  to install `zbm` and the programs it needs.

## Rules for every phase

1. **Host tools are swapped whole; the runtime is switched whole.** A Rust host tool replaces the legacy one when it
   passes its tests. For the boot runtime, a build-time setting selects the legacy or the Rust runtime for the entire
   image; the legacy runtime stays the default until the beta (Phase 7) ends.
2. **Tests first.** Behaviour that the port touches but no test covers gets a test, which must pass against the legacy
   code before the Rust version is written. The legacy code is the reference: when the two disagree, the Rust code is
   wrong unless the legacy code has a bug.
3. **Rust unit tests from the start.** Every change that adds or ports Rust code adds Rust unit tests for it, in the
   same change, from the first code written in Phase 1. Code without them is not merged. See
   [Rust unit tests](#rust-unit-tests).
4. **Freeze the legacy runtime and track every change.** From Phase 4 on, the legacy runtime gets bug fixes only. Each
   change to it, including changes merged from upstream ZFSBootMenu, is recorded in a tracking issue and must be ported
   before the beta ends. A bug found in the legacy code while porting is fixed there too, with a test.
5. **Every suite runs against both images.** CI builds a legacy and a Rust image and runs the LUKS unit suites
   (`tests/luks/unit/run-all.sh` and `run-in-container.sh`), the LUKS VM tests (`tests/luks/vm/run-all.sh`), the
   upstream test VMs (`tests/setup.sh`, `tests/run.sh`) and the CI build; the Rust unit tests run on every change.
   Results against the legacy image decide whether a change can be merged. Until Phase 6 ends, results against the
   Rust image are reported as the progress measure; after that, they decide too.
6. **No new features.** Changes in behaviour, even improvements, belong to ROADMAP.md, not to this plan. Its phase 1
   improvements can follow a host tool once it has been swapped, and the runtime once the cutover (Phase 8) is done,
   so the beta compares the same behaviour. Within the port, an error message changes only where the legacy message is
   wrong.
7. **Keep `unsafe` out** except where a crate needs FFI, and document each use.

## Rust unit tests

Rust unit tests are written from the start, together with the code they test. They do not replace the existing
suites: the Bash unit suites and the VM tests check that the Rust code behaves like the legacy code on real devices
and in real boots, which takes minutes to hours and needs root, QEMU and the build container. Rust unit tests check
each piece of logic on its own, in seconds, without root, on every change, so most mistakes are found before a VM
boots.

- **What they cover:** everything that decides something without changing the system.
  - Parsing: `zbm.*` options, kernel command lines, crypttab and `zbm.luks` options, `config.yaml`, `zbm-release`, and
    the output of `zfs`, `zpool`, `cryptsetup`, `tpm2-tools` and `gpg`.
  - Decisions: import policies, which volumes may hold ZFS, the order of key methods, boot environment sorting and the
    default choice, kernel selection, the naming and clean-up of generated images, EFI section offsets.
  - What goes out to the system: the arguments and environment of every external program, the hook environment, the
    `kexec` command line.
  - Menu state: which screen follows which key, and what each row shows.
- **Testable code:** code that runs programs, reads files, waits for devices, or reads the clock or the keyboard does so
  through a small interface (a Rust trait), which unit tests replace with a fake. Phase 4 defines these interfaces for
  the runtime. The fakes record what the code tried to do, so tests also check what it must not do, such as passing a
  secret in an argument, the environment or a file.
- **Real tool output:** output of `zfs`, `zpool`, `cryptsetup luksDump`, `tpm2_getcap` and similar commands is
  recorded in the build container (OpenZFS 2.4.4) and in the VM tests, and stored next to the tests
  (`crates/*/tests/fixtures/`), so parsers are tested against real output, not hand-made examples only.
- **Parity tests:** where a Bash function has a Rust counterpart without side effects (the command line tokenizer,
  option parsing, crypttab parsing, sort keys), a test runs both on the same inputs and compares the results. Inputs
  are written by hand and generated with `proptest`. These tests need only Bash and the legacy source tree, so they
  run in the normal Rust CI job. Before Phase 8 removes `legacy/`, their expected results are saved as fixtures, so the
  tests keep working without the Bash code.
- **Regressions:** a bug found by the existing suites, in the beta or by users gets a Rust unit test that reproduces
  it before it is fixed, whenever the bug is in logic a unit test can reach.
- **Measuring:** CI runs `cargo test` on every change (the Rust workflow already does) and measures line coverage with
  `cargo llvm-cov`. Library crates aim for at least 80%, and coverage must not drop. Code that only acts on the system
  (mounting, `kexec`, waiting for devices) is left to the VM tests.
- **Test-only dependencies** such as `proptest`, and `insta` for snapshot tests of menu screens, are dev-dependencies:
  they never reach the boot image.

## Compatibility contract

These interfaces are used by systems, scripts and users, and must not change during the port. The tests of each phase
cover the parts it touches.

| Interface | Where it is defined today |
| --- | --- |
| Kernel command line options: `zbm.prefer`, `zbm.import_policy`, `zbm.timeout`, `zbm.skip`, `zbm.show`, `zbm.lines`, `zbm.columns`, `zbm.sort_key`, `zbm.wait_for`, `zbm.import_delay`, `zbm.retry_delay`, `zbm.force_import`, `zbm.set_hostid`, `zbm.hookroot`, `zbm.kcl_override`, `zbm.luks`, `zbm.luks_key` and the rest | `legacy/zfsbootmenu/pre-init/zfsbootmenu-parse-commandline.sh`, `zfsbootmenu(7)`, `zfsbootmenu-luks(7)` |
| ZFS properties: `org.zfsbootmenu:commandline`, `:kernel`, `:active`, `:keysource`, `:rootprefix`, `:readonly`, `:devicetree` | `zfsbootmenu(7)` |
| Build settings: the `zfsbootmenu_*` dracut and mkinitcpio variables (`zfsbootmenu_module_root`, `zfsbootmenu_hook_root`, `zfsbootmenu_luks_crypttab` and the rest) | `legacy/zfsbootmenu/install-helpers.sh`, `zfsbootmenu(7)` |
| `generate-zbm` configuration (`config.yaml`) and options | `generate-zbm(5)`, `generate-zbm(8)` |
| Hook directories (`early-setup.d`, `setup.d`, `load-key.d`, `boot-sel.d`, `teardown.d`) and their environment (`ZBM_SELECTED_BE`, `ZBM_SELECTED_KERNEL`, `ZBM_SELECTED_INITRAMFS`, `ZBM_SELECTED_MOUNTPOINT`, `ZBM_LOCKED_FS`, `ZBM_ENCRYPTION_ROOT`) | `zfsbootmenu(7)` |
| Shell functions that hooks source from `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` (the hooks in `contrib/` do) | `legacy/zfsbootmenu/lib/` |
| The crypttab format and LUKS options | `zfsbootmenu-crypttab(5)` |
| Recovery shell commands and key bindings | `docs/online/`, `zluks(8)`, `zbm-kcl(8)` |
| Installed paths and file names | `Makefile` |

## Overview

The legacy code holds about 10,800 lines of Bash and Perl in `legacy/` (not counting help pages and fonts), plus
about 480 lines of dracut and mkinitcpio glue in `packaging/`. Line counts are rough, because
`lib/zfsbootmenu-core.sh` is split across several work packages. The estimates are for one developer who knows the
code.

| Phase | Work | Legacy lines | Estimate | Users get |
| --- | --- | --- | --- | --- |
| 1. Foundations | Build, packaging, test setup, Rust unit test tooling | (new) | 3–4 weeks | — |
| 2. Shared libraries and `zbm-kcl` | Logging, command line parsing, KCL handling, ZFS command layer, `zbm-kcl` | ~1,300 | 3–4 weeks | Rust `zbm-kcl` |
| 3. `generate-zbm` | The image generator | ~1,170 | 5–7 weeks | Rust `generate-zbm`, no Perl |
| 4. Runtime design | Data model, process model, crates, tests | — | 2–3 weeks | — |
| 5. Minimal bootable runtime | Start-up, import, default boot environment, `kexec` | part of A and C | 4–6 weeks | — |
| 6. Complete the runtime | Work packages A to F | ~8,000¹ | 18–26 weeks | — |
| 7. Beta | Both images, legacy as fallback | — | 3–4 weeks | Rust image (beta) |
| 8. Cutover | Rust by default, legacy removed, release | — | 2–3 weeks | Rust release |

¹ Including about 1,450 lines of install helpers and packaging glue, which shrink rather than move to Rust.

The total is about 40 to 57 weeks, or 9 to 13 months. The estimates include writing the Rust unit tests, which
take roughly a sixth of each phase and save time later, because most mistakes show up in seconds instead of in VM
runs. Phase 3 does not depend on the runtime: with a second developer on phases 2 and 3 and on some of the work
packages, the port takes about 6 to 9 months.

## Status

| Phase | Status |
| --- | --- |
| 1. Foundations | Done, with the deviations below |
| 2. Shared libraries and `zbm-kcl` | Done |
| 3. `generate-zbm` | Done |
| 4 to 8 | Not started |

**Delivered by phases 1 to 3:**

- **Crates:** `zbm-sys` (the only FFI: signals and process credentials), `zbm-core` (running programs, file tests
  with Perl's rules, the Bash semantics the legacy code relies on, `getopts`, release information), `zbm-log`,
  `zbm-cmdline`, `zbm-zfs`, the host tools `zbm-kcl` and `generate-zbm`, the multi-call program `zbm` with an empty
  command table, and `zbm-testutil` (fakes, helpers that run legacy Bash functions and Perl code, a seeded input
  generator). Every crate follows The Rust Style Guide (`CONTRIBUTING.md`).
- **Building:** `make` builds the Rust programs with `cargo build --release --locked` from crates vendored in
  `vendor/`; `make install` installs `generate-zbm` and `zbm-kcl` in place of the legacy tools (`TOOLS=legacy` keeps
  them) and `zbm` as `/usr/libexec/zfsbootmenu/zbm`. The build container builds the programs from the tree it builds
  images from, offline, and builds images with the Rust `generate-zbm` (`-g perl` selects the Perl one), so
  `legacy/cpanfile` is no longer needed. `scripts/version.sh` sets the version of the Rust programs too.
- **Images:** the dracut module and the mkinitcpio hook install `zbm` as `/libexec/zbm`. The build setting
  `zfsbootmenu_runtime` selects the runtime; only `legacy`, the default, is accepted until the Rust runtime boots.
- **Tests:** 139 Rust tests. Parity tests compare the Rust code with the legacy Bash functions, with Perl's
  `Getopt::Long`, `File::Glob`, `YAML::PP`, `Data::Dumper` and `Sort::Versions`, and with the whole legacy
  `zbm-kcl` and `generate-zbm` in sandboxes, on hand-written and generated inputs. The release, recovery and
  container-default images built by the Perl and the Rust `generate-zbm` have the same contents
  (`scripts/dev/compare-generators.sh`). The LUKS unit suites load the code under test through
  `tests/luks/unit/impl.sh` (`ZBMTEST_IMPL`). `scripts/dev/record-fixtures.py` records `zfs`, `zpool` and mount
  output of OpenZFS 2.4.4 as fixtures for the `zbm-zfs` tests.
- **CI:** the Rust workflow checks formatting, line widths, clippy, the tests, the documentation, the oldest supported
  compiler, the musl and aarch64 targets and line coverage; the build workflow builds the images and compares the
  two generators.
- **Legacy bugs fixed while porting,** in the legacy code too and with tests: `%{parent}` with an `&` in the parent's
  command line, unsafe arithmetic evaluation of untrusted input in `spl_hostid`, and in `generate-zbm` unquoted paths
  in shell commands, a second unmount after a signal, commands killed by signals counted as successes, an exit status
  of 0 when `uname -r` prints nothing, offsets computed without a section alignment, offsets misprinted in the debug
  log, and unchecked writes of the configuration (`CHANGELOG.md` lists them).

**Deviations from the plan:**

- Generated test inputs come from a small seeded generator in `zbm-testutil` (`ZBM_TEST_SEED` selects other inputs)
  instead of `proptest`, and `insta` is not used yet; both would add dependencies to the vendored tree for little
  gain so far. Phase 6 decides on `insta` for the menu screens.
- The runtime crates (`zbm-luks`, `zbm-boot`, `zbm-menu`) are created in Phase 4, whose design decides them; Phase 1
  created the crates the host tools need and the `zbm` program.
- Until the Rust runtime can boot (Phase 5), there is no Rust image to build: CI builds images with both generators
  and compares them, and every image includes `zbm`. The Rust image joins CI when `zfsbootmenu_runtime=rust` works.
- Fixtures are recorded in a QEMU virtual machine booted from an image the build container builds, because creating
  pools needs the OpenZFS 2.4.4 kernel module, which the container cannot load on most hosts.
- The coverage job reports coverage; the 80% target and the rule that coverage must not drop will be enforced once
  the reports have settled.

**Known differences between the Perl and the Rust `generate-zbm`,** in cases that do not occur in practice:

- A configuration section that is not a mapping (`EFI: yes`) ends both with an error; Perl exits with whatever error
  number an earlier system call left, the Rust version with 255. Perl's fatal messages also name its source lines.
- The Rust version reads the configuration as UTF-8 and skips file names that are not valid UTF-8; the Perl version
  writes non-ASCII configuration strings as Latin-1 bytes, which breaks paths that contain them.
- Complex mapping keys in the configuration (`? [a, b]`) and cyclic aliases are rejected by the Rust version; YAML::PP
  accepts the first and rejects the second.
- `--help` prints the manual page as Pod::Usage renders it; the Perl version runs `perldoc`, whose output depends on
  the host. Both exit with status 1.

## Phase 1: Foundations

**Goal:** everything needed to build, ship and test Rust code, before any behaviour is ported.

- **Crates:** split the workspace into crates: `zbm-core` (shared types, errors), `zbm-log`, `zbm-cmdline`, `zbm-zfs`,
  `zbm-luks`, `zbm-boot`, `zbm-menu`, the `zbm` binary, and the host tools `generate-zbm` and `zbm-kcl`. Phase 4 may
  change the runtime crates.
- **One binary:** set up `zbm` as a multi-call binary with an empty set of commands.
- **Linking:** link dynamically against the host's C library, glibc or musl, like the other programs dracut and
  mkinitcpio install (the test VMs include Alpine and Void musl systems); libcryptsetup is a shared library. Check that
  x86_64 and aarch64 builds work, since ZFSBootMenu supports both.
- **Building:** `make` builds the workspace with `cargo build --release --locked`. The build container compiles it
  from the source tree, offline, from dependencies vendored with `cargo vendor`, so container builds need no network.
- **Image integration:** the dracut module and the mkinitcpio hooks install `zbm`, and a build-time setting selects
  the legacy or the Rust runtime for the whole image (legacy by default).
- **Tests:** the LUKS unit suites source the Bash libraries directly. Give them a way to call the code under test
  through commands, so the same suites can run against the Rust code. CI builds a legacy and a Rust image and can run
  every suite against both.
- **Rust unit test tooling:** a helper that runs a function from a legacy Bash library and returns its output, for
  parity tests; a script that records tool output in the build container as fixtures; `proptest` and `insta` as
  dev-dependencies; and a coverage job (`cargo llvm-cov`) in the Rust workflow.

**Done when:** an image that contains `zbm` but does not use it passes every suite; CI builds both images; release
builds for x86_64 and aarch64 work in the build container; a first parity test runs in CI and coverage is reported.

## Phase 2: Shared libraries and zbm-kcl

**Goal:** the libraries that `zbm-kcl` needs now and the runtime needs later, and the first Rust program that users
run: `zbm-kcl`.

| Ports | From |
| --- | --- |
| Logging to the kernel log with levels (`zdebug` to `zerror`), and reading it back | `lib/kmsg-log-lib.sh`, `lib/echo-log-lib.sh` |
| Kernel command line options (`zbm.*`) and their defaults | `pre-init/zfsbootmenu-parse-commandline.sh` |
| Splitting, editing and joining kernel command lines (`kcl_tokenize`, `kcl_suppress`, `kcl_append`, `kcl_assemble`) | `lib/zfsbootmenu-kcl.sh` |
| Running `zfs` and `zpool` and parsing their `-H -p` output; property, mount and unmount helpers | parts of `lib/zfsbootmenu-core.sh` |
| Reading the release information in `/etc/zbm-release` | the version and name handling in `lib/zfsbootmenu-core.sh` |
| `zbm-kcl`: reading and changing command lines in `org.zfsbootmenu:commandline` and in EFI images | `bin/zbm-kcl` |

The command line code is shared: `zbm-kcl` on the host and the menu in the image must split and join command lines
the same way.

**Done when:** Rust unit tests cover the libraries: parity tests compare the tokenizer and the option parser with
the Bash versions on hand-written and generated inputs, including every quoting case, and the `zfs` and `zpool`
parsers are tested against recorded OpenZFS 2.4.4 output. A test runs legacy and Rust `zbm-kcl` on the same boot
environments and EFI images and gets the same results, and `make install` installs the Rust `zbm-kcl`.

## Phase 3: generate-zbm

**Goal:** a Rust `generate-zbm` that reads the same configuration and builds the same images, so that Perl is no
longer needed. This phase does not depend on the runtime and can run in parallel with phases 4 to 6.

- **Configuration:** read `config.yaml` with the same keys, defaults and errors, and accept the same command line
  options.
- **Image generation:** find kernels and their versions, build the initramfs with dracut or mkinitcpio, and keep the
  components and EFI versions with the same naming and clean-up rules.
- **EFI bundles:** assemble them with `objcopy` as today: the stub, the section offsets and alignment, and the
  `.osrel`, `.cmdline`, `.linux`, `.initrd`, `.splash` and `.dtb` sections.
- **Hooks and environment:** run the pre- and post-generation hooks (`PreHooksDir`, `PostHooksDir`), pass mkinitcpio
  hook directories (`InitCPIOHookDirs`, `--hookd`), and support the way the build container runs `generate-zbm`
  (variables and paths in `build-init.sh`).

**Done when:** for the release, recovery and test configurations, the Rust and Perl generators produce images with the
same contents. The comparison checks file lists, contents, permissions and symlinks after extraction, the method used
to check the repository reorganization. Rust unit tests cover every configuration key, default and error described
in `generate-zbm(5)`, the naming and clean-up rules, kernel version ordering (compared with Perl's `Sort::Versions`)
and the EFI section offsets (compared with images built by the Perl version). The build container and
`scripts/release/make-binary.sh` use the Rust generator, and `legacy/cpanfile` is no longer needed for builds.

## Phase 4: Runtime design

**Goal:** decide how the Rust runtime is built before writing it, and freeze the legacy runtime.

The design is written down in `crates/DESIGN.md` and answers at least:

- **Data model:** pools, boot environments, kernels, snapshots, LUKS volumes and keys, and how they relate.
- **Process model:** which work happens in one long-running process, and how commands that `fzf` and hooks start
  (previews, key bindings, the recovery shell commands) reach the state the legacy code keeps in files under
  `/zfsbootmenu`.
- **Hooks:** how hooks run, which environment they get, and how the compatibility library for hooks that source
  `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` calls `zbm`.
- **Errors and logging:** one error type, how errors reach the screen and the kernel log, and how the recovery shell
  is offered after a failure.
- **External programs:** how they are run, how their output is parsed, and how secrets are passed to them without
  command-line arguments, environment variables or files.
- **Testing:** the interfaces through which the runtime reaches the system (programs, files, devices, clock,
  console), so that every part can be unit-tested with fakes; how each work package is tested; and how the existing
  suites drive the Rust runtime.

The legacy runtime is frozen at the start of this phase (rule 4), and the tracking issue for its changes is opened.

**Done when:** `crates/DESIGN.md` has been reviewed, the crate layout matches it, the system interfaces exist with
their fakes and their own unit tests, and the tracking issue exists.

## Phase 5: Minimal bootable runtime

**Goal:** the smallest Rust runtime that boots a machine: from start-up to `kexec`, with no menu.

- **Included:** loading kernel modules, the host ID, waiting for devices, importing pools with the default import
  policy, finding the default boot environment (`bootfs`) and its kernel, initramfs and command line, exporting the
  pools, and `kexec`. It behaves like the legacy runtime with `zbm.skip`.
- **Left out until Phase 6:** LUKS, the menu, hooks, ZFS native encryption and every option the default boot does not
  need. When the Rust runtime meets something it cannot handle yet, it says so and opens the recovery shell.
- **New test:** a VM test boots the default boot environment of an unencrypted pool. It must pass with the legacy
  image first (rule 2).

**Done when:** Rust unit tests cover the import, the choice of boot environment and kernel, and the `kexec`
arguments, using the fakes from Phase 4; the new VM test passes with the Rust image; and CI runs every suite against
the Rust image from now on and reports how many tests pass.

## Phase 6: Complete the runtime

**Goal:** everything else the legacy runtime does, until every suite passes against the Rust image.

The work is split into six packages. They can be done in any order, or by several developers at once, but the order
below follows the start-up sequence and puts the largest risk early. A package is done when its Rust unit tests
pass and the suites covering it pass against the Rust image; the phase is done when every suite passes against it.

| Package | Ports | Legacy lines | Estimate |
| --- | --- | --- | --- |
| [A](#a-startup-and-pool-import) | Start-up sequence, hooks, every import option | ~750 | 2–3 weeks |
| [B](#b-luks-and-key-handling) | Unlocking, tokens, GnuPG, YubiKey, `zluks` | ~2,300 | 6–8 weeks |
| [C](#c-boot-environments-and-booting) | Discovery, kernel selection, ZFS encryption keys, `kexec` | ~1,000 | 2–3 weeks |
| [D](#d-snapshot-and-pool-management) | Snapshots, rollback, clones, checkpoints, chroot, diff | ~650 | 2–3 weeks |
| [E](#e-interactive-menu) | Menu screens with `fzf`, previews, help, KCL editor | ~1,400 | 4–6 weeks |
| [F](#f-recovery-tools-and-image-assembly) | Recovery shell commands, hook library, install helpers | ~1,900 | 2–3 weeks |

Phase 5 already covers the basic paths of A and C, so their estimates are lower than porting them from scratch.

### A. Startup and pool import

| Ports | From |
| --- | --- |
| Start-up sequence and its order | `libexec/zfsbootmenu-init`, `pre-init/zfsbootmenu-preinit.sh`, the dracut ready checks |
| Kernel modules, host ID, waiting for devices (`zbm.wait_for`) | `init.d/10-kmods`, `init.d/20-hostid`, `init.d/30-device-wait` |
| Running hooks with their environment | `libexec/zfsbootmenu-run-hooks`, `init.d/40-early-hooks` |
| Pool import: import policies, retries and delays, read-only import, host ID matching, unsupported feature detection, `zbm.prefer` | `init.d/50-import-pools`, `import_pool`, `export_pool`, `check_for_pools`, `match_hostid`, `write_hostid` |
| Timed prompts and the resume check | `timed_prompt`, `has_resume_device`, `resume_prompt` |

**Done when:** Rust unit tests cover every import policy, retry and host ID case, and the upstream test VMs and the
LUKS VM tests that cover import policies, host IDs and late devices pass.

### B. LUKS and key handling

The largest and most security-sensitive package.

| Ports | From |
| --- | --- |
| Configuration: embedded crypttab, `zbm.luks`, `zbm.luks_key` | `lib/zfsbootmenu-luks.sh` (`luks_load_config`, `luks_parse_options`) |
| Finding volumes, deciding which may hold ZFS, waiting for devices, late and hot-plugged volumes | `luks_discover_devices`, `luks_may_hold_zfs`, `luks_wait_device`, `init.d/50-import-pools` |
| Passphrases (and reusing one for several volumes), key files, key devices, key scripts | `luks_unlock_all`, `luks_open_device`, `luks_find_keyfile`, `luks_read_secret` |
| TPM2 and FIDO2 tokens through systemd plugins or directly, the TPM lockout check, clevis | `lib/zfsbootmenu-luks-keys.sh` (`luks_try_tokens` and the functions it calls) |
| GnuPG key files with a smart card or passphrase, YubiKey challenge-response | `luks_gpg_*`, `luks_try_ykchalresp` |
| Closing volumes and forgetting secrets before booting | `hooks/teardown.d/80-luks-close.sh`, `luks_close_all`, `luks_forget_secrets` |
| `zluks` | `bin/zluks` |

Secrets stay out of command lines, environment variables and files, as they do today. Calling `cryptsetup` through
`libcryptsetup-rs` instead of the program is allowed if every suite passes with it.

**Done when:** Rust unit tests cover the configuration parsing (with parity tests against the Bash parser), the order
of key methods, token metadata parsed from recorded `cryptsetup luksDump` output, and check that no secret reaches an
argument, the environment or a file. The LUKS unit suites (248 checks today) and every LUKS VM test pass against the
Rust image, on the host and in the build container, including the key, lockout, hot-plug and firmware tests.

### C. Boot environments and booting

| Ports | From |
| --- | --- |
| Boot environment discovery and sorting (`zbm.sort_key`), `bootfs` and `org.zfsbootmenu:active` | `find_be_candidates`, `populate_be_list`, `get_sort_key` |
| Kernels and initramfs images, `org.zfsbootmenu:kernel`, `:rootprefix`, `:devicetree` | `find_be_kernels`, `find_be_initramfs`, `select_kernel`, `find_root_prefix` |
| Command lines for boot environments, and their cache | `load_be_cmdline`, `kernel_kcl_cache`, `validate_cmdline_cache` |
| ZFS native encryption: encryption roots, key loading, `org.zfsbootmenu:keysource` and its key cache, `load-key.d` hooks | `be_has_encroot`, `be_is_locked`, `be_keysource`, `load_key`, `cache_key` |
| Booting: `boot-sel.d` and `teardown.d` hooks, exporting pools, `kexec` | `kexec_kernel`, `bin/zkexec` |
| Automatic boot after the timeout (`zbm.timeout`, `zbm.skip`, `zbm.show`) | `libexec/zfsbootmenu-init` |

**Done when:** Rust unit tests cover sorting, kernel selection, the command line cache, key source handling and the
hook environment, and every VM test that boots a boot environment passes, including the native encryption,
`keysource`, `rootprefix` and device tree cases of the upstream test VMs.

### D. Snapshot and pool management

| Ports | From |
| --- | --- |
| Creating, duplicating, cloning and rolling back snapshots | `create_snapshot`, `duplicate_snapshot`, `clone_snapshot`, `rollback_snapshot` |
| Default boot environment and kernel | `set_default_env`, `set_default_kernel` |
| Read-write and read-only pool imports, checkpoint rewind | `set_rw_pool`, `set_ro_pool`, `is_writable`, `rewind_checkpoint` |
| Snapshot diff | `libexec/zfsbootmenu-diff` |
| Chroot into a boot environment | `zfs_chroot`, `bin/zfs-chroot` |
| Pool status for the health screen | `draw_pool_status` (the data part) |

Most of these actions have no automated test today, so this package adds the tests first, against the legacy image.

**Done when:** Rust unit tests check the commands each action runs, a suite test covers each action, and each test
passes against the Rust image.

### E. Interactive menu

The menu keeps driving `fzf`, with the same screens, key bindings and previews.

| Ports | From |
| --- | --- |
| Main loop, screens and navigation | `bin/zfsbootmenu`, `lib/zfsbootmenu-ui.sh` |
| `fzf` options and feature detection | `lib/fzf-defaults.sh` |
| Previews, header and column layout (`zbm.lines`, `zbm.columns`) | `libexec/zfsbootmenu-preview`, `global_header`, `column_wrap` |
| Help viewer (the pages in `help-files/`) | `libexec/zfsbootmenu-help` |
| Command line editor, power menu, input helper | `libexec/zfsbootmenu-edit-kcl`, `libexec/zpowermenu`, `libexec/zfsbootmenu-input` |
| Console set-up hooks | `hooks/early-setup.d/*` |

**Done when:** Rust unit tests cover the menu state for every key binding in `docs/online/`, and snapshot tests the
text of each screen; every key binding works, each screen shows the same information, and the VM tests, which drive
the menu over the serial console, pass.

### F. Recovery tools and image assembly

| Ports | From |
| --- | --- |
| Recovery shell commands: `zlogtail`, `ztrace`, `zsnapshots`, `mount_esp`, `reboot` and its aliases | `bin/` |
| Error printer, debug report (`zreport`), shell completions | `libexec/recovery-error-printer`, `zreport`, `lib/zfsbootmenu-completions.sh` |
| The functions hooks source from `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` | a small compatibility library, written in shell, that calls `zbm` |
| Choosing what goes into the image | `install-helpers.sh`, `packaging/dracut/module-setup.sh`, `packaging/initcpio/install/zfsbootmenu` |

The install helpers shrink to installing `zbm`, the external programs and libraries it needs, the hooks, help pages
and fonts. `zbm` can list the programs it needs, so the list is kept in one place. The xHCI teardown hook
(`hooks/teardown.d/90-xhci-unbind.sh`) stays a shell script, like user hooks.

**Done when:** Rust unit tests cover the list of programs `zbm` needs and the recovery commands' logic; the Rust image
contains no ZFSBootMenu Bash libraries except the compatibility library; the hooks in `contrib/` work unchanged; image
sizes are compared with the legacy image.

## Phase 7: Beta with the legacy image as fallback

**Goal:** real machines run the Rust runtime, with the legacy runtime one boot entry away.

- **Both images:** release and recovery builds produce a Rust image and a legacy image from the same tree. The Rust
  image is the first boot entry; the documentation shows how to keep the legacy image as a second entry with
  `efibootmgr`, or as a second entry in other boot loaders.
- **Catch up:** every change recorded in the tracking issue (rule 4) is ported, and the issue is closed.
- **Bugs:** every bug found in the beta gets a Rust unit test, or a suite test where a unit test cannot reach it,
  that fails before the fix.
- **Hardware:** the beta is tested on UEFI x86_64 and aarch64 machines, on firmware with Secure Boot enabled, and on a
  BIOS machine booting through syslinux, in addition to the VM tests.

**Done when:** every suite passes against both images, the tracking issue is closed, and no bug that prevents booting
has been open for four weeks of beta.

## Phase 8: Cutover and removal of the legacy code

**Goal:** Rust is the only implementation.

- **Default:** make the Rust runtime the default, and ship one release with the legacy runtime still selectable.
- **Removal:** after that release, save the expected results of the parity tests as fixtures, then remove
  `legacy/`, the build-time setting and the legacy CI build. `make install` installs the Rust programs at the same
  paths as before.
- **Documentation:** update the man pages, the online help and `docs/`, which describe behaviour and stay valid, and
  remove references to Perl and the legacy libraries.
- **Release tooling:** update `scripts/release/` for the Rust builds, and give the release notes upgrade instructions
  for distribution packagers (Rust toolchain, vendored dependencies, no Perl).

**Done when:** every suite and every Rust unit test passes on the Rust-only tree; release images are built and signed
with the updated tooling; the release notes are published.

## Alternatives

### Fully phased

Each work package of Phase 6 becomes its own phase, in the order A to F, and replaces its part of the legacy runtime
as soon as it is done. Phases 4 and 5 are dropped. Each part gets its own build-time setting, and Bash and Rust call
each other where ported and legacy parts meet: until package B is done, for example, the Rust start-up sequence from
package A calls the legacy LUKS code.

This ships a working result at every step and keeps the first full boot of Rust code early. It costs 1 to 2 months of
glue that is thrown away at the end, and the Rust code has to follow the Bash code's state files until the last
package. Choose it if a single developer wants shippable progress at every step. The total time is the same, 9 to 13
months.

### Everything in one go

The host tools are also written next to the legacy ones and switched together with the runtime. Nothing is usable
until the end, and the risk of the whole port lands on one switch. It saves a few weeks at most (8 to 12 months), and
is not recommended.

## Out of scope

- **New features and behaviour changes:** see [ROADMAP.md](ROADMAP.md).
- **The build container scripts** (`packaging/container/`): they run on the host or in the container, not in the
  image, and stay shell.
- **The dracut and mkinitcpio glue:** both expect shell scripts, so it stays shell (work package F shrinks it).
- **The test suites:** the Bash unit suites and the Python VM tests stay as they are, because they check that the
  Rust code behaves like the legacy code. The Rust unit tests are written alongside them (see
  [Rust unit tests](#rust-unit-tests)).
- **`contrib/` scripts and user hooks:** they stay shell, and keep working through the compatibility library.
