# Rust port roadmap

This roadmap plans the port of ZFSBootMenu from Bash and Perl to Rust, in ten phases. Its goal is **parity**: the Rust
version does what the legacy code in `legacy/` does today, nothing more. New features wait until the port is done;
they are listed separately in [ROADMAP.md](ROADMAP.md). [CONTRIBUTING.md](CONTRIBUTING.md) explains how to build and
test.

## Contents

- [End state](#end-state)
- [Rules for every phase](#rules-for-every-phase)
- [Compatibility contract](#compatibility-contract)
- [Overview](#overview)
- Phases: [1](#phase-1-foundations) · [2](#phase-2-shared-libraries-and-zbm-kcl) · [3](#phase-3-generate-zbm) ·
  [4](#phase-4-startup-and-pool-import) · [5](#phase-5-luks-and-key-handling) ·
  [6](#phase-6-boot-environments-and-booting) · [7](#phase-7-snapshot-and-pool-management) ·
  [8](#phase-8-interactive-menu) · [9](#phase-9-recovery-tools-and-image-assembly) ·
  [10](#phase-10-cutover-and-removal-of-the-legacy-code)
- [Out of scope](#out-of-scope)

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

1. **Replace one part at a time.** Every phase ends with a working, shippable image. Until a phase is done, a
   build-time setting selects the legacy or the Rust version of each ported part; the legacy version stays the default
   until the Rust version passes every test.
2. **Tests first.** Behaviour that a phase ports but no test covers gets a test, which must pass against the legacy code
   before porting starts. The legacy code is the reference: when the two disagree, the Rust code is wrong unless the
   legacy code has a bug.
3. **Fix bugs in both.** A bug found in the legacy code while porting is fixed in the legacy code too, with a test,
   so the two stay comparable until the legacy code is removed.
4. **Every phase runs every suite:** the LUKS unit suites (`tests/luks/unit/run-all.sh` and `run-in-container.sh`),
   the LUKS VM tests (`tests/luks/vm/run-all.sh`), the upstream test VMs (`tests/setup.sh`, `tests/run.sh`), the Rust
   tests and the CI build. CI runs the suites against both the legacy and the Rust build until Phase 10.
5. **No new features.** Changes in behaviour, even improvements, belong to ROADMAP.md, not to this plan. Its phase 1
   improvements can follow a part as soon as that part matches the legacy code, as separate changes with their own
   tests. Within the port, an error message changes only where the legacy message is wrong.
6. **Keep `unsafe` out** except where a crate needs FFI, and document each use.

## Compatibility contract

These interfaces are used by systems, scripts and users, and must not change during the port. Each phase lists the
parts it touches, and its tests cover them.

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
about 480 lines of dracut and mkinitcpio glue in `packaging/`. Line counts per phase are rough, because
`lib/zfsbootmenu-core.sh` is split across several phases. The estimates are for one developer who knows the code, and
add up to about 9 to 13 months.

| Phase | Ports | Legacy lines | Estimate |
| --- | --- | --- | --- |
| 1. Foundations | Build, packaging and test setup | (new) | 3–4 weeks |
| 2. Shared libraries and `zbm-kcl` | Logging, command line parsing, KCL handling, ZFS command layer, `zbm-kcl` | ~1,300 | 3–4 weeks |
| 3. `generate-zbm` | The image generator | ~1,170 | 5–7 weeks |
| 4. Startup and pool import | Start-up sequence, hooks, pool import | ~750 | 4–6 weeks |
| 5. LUKS and key handling | Unlocking, tokens, GnuPG, YubiKey, `zluks` | ~2,300 | 7–9 weeks |
| 6. Boot environments and booting | Discovery, kernel selection, ZFS encryption keys, `kexec` | ~1,000 | 4–6 weeks |
| 7. Snapshot and pool management | Snapshots, rollback, clones, checkpoints, chroot, diff | ~650 | 3–4 weeks |
| 8. Interactive menu | Menu screens with `fzf`, previews, help, KCL editor | ~1,400 | 5–7 weeks |
| 9. Recovery tools and image assembly | Recovery shell commands, hook library, install helpers | ~1,900¹ | 3–4 weeks |
| 10. Cutover | Rust by default, legacy removed, release | — | 2–3 weeks |

¹ Most of it is the install helpers and packaging glue, which shrink rather than move to Rust.

Phases 2 and 3 run on the host and do not touch the boot path, so they come first: they prove the toolchain, the
packaging and the release process at low risk. Phases 4 to 9 follow the order in which the image runs: start-up,
unlocking, choosing and booting a boot environment, then the menu and the tools around it.

## Phase 1: Foundations

**Goal:** everything needed to build, ship and test Rust code, before any behaviour is ported.

- **Crates:** split the workspace into crates along the phases: `zbm-core` (shared types, errors), `zbm-log`,
  `zbm-cmdline`, `zbm-zfs`, `zbm-luks`, `zbm-boot`, `zbm-menu`, the `zbm` binary, and the host tools `generate-zbm`
  and `zbm-kcl`.
- **One binary:** set up `zbm` as a multi-call binary with an empty set of commands.
- **Linking:** link dynamically against the host's C library, glibc or musl, like the other programs dracut and
  mkinitcpio install (the test VMs include Alpine and Void musl systems); libcryptsetup is a shared library. Check that
  x86_64 and aarch64 builds work, since ZFSBootMenu supports both.
- **Building:** `make` builds the workspace with `cargo build --release --locked`. The build container compiles it
  from the source tree, offline, from dependencies vendored with `cargo vendor`, so container builds need no network.
- **Image integration:** the dracut module and the mkinitcpio hooks install `zbm`, and a build-time setting chooses
  legacy or Rust for each ported part (all legacy for now).
- **Tests:** the LUKS unit suites source the Bash libraries directly. Give them a way to call the code under test
  through commands, so the same suites can run against the Rust code. CI builds two images, legacy and Rust, and runs
  every suite against both.

**Done when:** an image that contains `zbm` but does not use it passes every suite; CI runs the suites against both
builds; release builds for x86_64 and aarch64 work in the build container.

## Phase 2: Shared libraries and zbm-kcl

**Goal:** the libraries every later phase needs, and the first Rust program that users run: `zbm-kcl`.

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

**Done when:** Rust unit tests cover the libraries, including the quoting cases of the Bash tokenizer; a test runs
legacy and Rust `zbm-kcl` on the same boot environments and EFI images and gets the same results; the image uses the
Rust command line parser and logging, and every suite passes.

## Phase 3: generate-zbm

**Goal:** a Rust `generate-zbm` that reads the same configuration and builds the same images, so that Perl is no
longer needed.

- **Configuration:** read `config.yaml` with the same keys, defaults and errors, and accept the same command line
  options.
- **Image generation:** find kernels and their versions, build the initramfs with dracut or mkinitcpio, and keep the
  components and EFI versions with the same naming and clean-up rules.
- **EFI bundles:** assemble them with `objcopy` as today: the stub, the section offsets and alignment, and the
  `.osrel`, `.cmdline`, `.linux`, `.initrd`, `.splash` and device tree sections.
- **Hooks and environment:** run the pre- and post-generation hooks (`PreHooksDir`, `PostHooksDir`), pass mkinitcpio
  hook directories (`InitCPIOHookDirs`, `--hookd`), and support the way the build container runs `generate-zbm`
  (variables and paths in `build-init.sh`).

**Done when:** for the release, recovery and test configurations, the Rust and Perl generators produce images with the
same contents. The comparison checks file lists, contents, permissions and symlinks after extraction, the method used
to check the repository reorganization. The build container and `scripts/release/make-binary.sh` use the Rust
generator, and `legacy/cpanfile` is no longer needed for builds.

## Phase 4: Startup and pool import

**Goal:** the image starts up and imports pools with Rust code, then hands over to the legacy menu.

| Ports | From |
| --- | --- |
| Start-up sequence and its order | `libexec/zfsbootmenu-init`, `pre-init/zfsbootmenu-preinit.sh`, the dracut ready checks |
| Kernel modules, host ID, waiting for devices (`zbm.wait_for`) | `init.d/10-kmods`, `init.d/20-hostid`, `init.d/30-device-wait` |
| Running hooks with their environment | `libexec/zfsbootmenu-run-hooks`, `init.d/40-early-hooks` |
| Pool import: import policies, retries and delays, read-only import, host ID matching, unsupported feature detection, `zbm.prefer` | `init.d/50-import-pools`, `import_pool`, `export_pool`, `check_for_pools`, `match_hostid`, `write_hostid` |
| Timed prompts and the resume check | `timed_prompt`, `has_resume_device`, `resume_prompt` |

LUKS unlocking (`init.d/45-luks-unlock`) runs between device waiting and pool import. Until Phase 5, the Rust
start-up sequence calls the legacy LUKS code at that point.

**Done when:** the upstream test VMs and every LUKS VM test pass with the Rust start-up sequence, including the import
policy, host ID and late-device cases.

## Phase 5: LUKS and key handling

**Goal:** unlocking and closing LUKS volumes in Rust, with every key method this fork supports. This is the largest
and most security-sensitive phase.

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
`libcryptsetup-rs` instead of the program is allowed in this phase if every suite passes with it.

**Done when:** the LUKS unit suites (248 checks today) and every LUKS VM test pass against the Rust code, on the host
and in the build container, including the key, lockout, hot-plug and firmware tests.

## Phase 6: Boot environments and booting

**Goal:** finding what can be booted and booting it, in Rust.

| Ports | From |
| --- | --- |
| Boot environment discovery and sorting (`zbm.sort_key`), `bootfs` and `org.zfsbootmenu:active` | `find_be_candidates`, `populate_be_list`, `get_sort_key` |
| Kernels and initramfs images, `org.zfsbootmenu:kernel`, `:rootprefix`, `:devicetree` | `find_be_kernels`, `find_be_initramfs`, `select_kernel`, `find_root_prefix` |
| Command lines for boot environments, and their cache | `load_be_cmdline`, `kernel_kcl_cache`, `validate_cmdline_cache` |
| ZFS native encryption: encryption roots, key loading, `org.zfsbootmenu:keysource` and its key cache, `load-key.d` hooks | `be_has_encroot`, `be_is_locked`, `be_keysource`, `load_key`, `cache_key` |
| Booting: `boot-sel.d` and `teardown.d` hooks, exporting pools, `kexec` | `kexec_kernel`, `bin/zkexec` |
| Automatic boot after the timeout (`zbm.timeout`, `zbm.skip`, `zbm.show`) | `libexec/zfsbootmenu-init` |

**Done when:** every VM test that boots a boot environment passes with Rust doing the selection and the `kexec`,
including native encryption, `keysource`, `rootprefix` and device tree cases from the upstream test VMs.

## Phase 7: Snapshot and pool management

**Goal:** every action the menu offers on snapshots and pools, as Rust commands the legacy menu can call.

| Ports | From |
| --- | --- |
| Creating, duplicating, cloning and rolling back snapshots | `create_snapshot`, `duplicate_snapshot`, `clone_snapshot`, `rollback_snapshot` |
| Default boot environment and kernel | `set_default_env`, `set_default_kernel` |
| Read-write and read-only pool imports, checkpoint rewind | `set_rw_pool`, `set_ro_pool`, `is_writable`, `rewind_checkpoint` |
| Snapshot diff | `libexec/zfsbootmenu-diff` |
| Chroot into a boot environment | `zfs_chroot`, `bin/zfs-chroot` |
| Pool status for the health screen | `draw_pool_status` (the data part) |

**Done when:** tests cover each action on the upstream test VMs (most have no automated test today, so this phase
adds them first), and every action behaves the same with the Rust commands.

## Phase 8: Interactive menu

**Goal:** the menu itself in Rust. It keeps driving `fzf` with the same screens, key bindings and previews.

| Ports | From |
| --- | --- |
| Main loop, screens and navigation | `bin/zfsbootmenu`, `lib/zfsbootmenu-ui.sh` |
| `fzf` options and feature detection | `lib/fzf-defaults.sh` |
| Previews, header and column layout (`zbm.lines`, `zbm.columns`) | `libexec/zfsbootmenu-preview`, `global_header`, `column_wrap` |
| Help viewer (the pages in `help-files/`) | `libexec/zfsbootmenu-help` |
| Command line editor, power menu, input helper | `libexec/zfsbootmenu-edit-kcl`, `libexec/zpowermenu`, `libexec/zfsbootmenu-input` |
| Console set-up hooks | `hooks/early-setup.d/*` |

**Done when:** every key binding in `docs/online/` works, each screen shows the same information, and the VM tests,
which drive the menu over the serial console, pass.

## Phase 9: Recovery tools and image assembly

**Goal:** the last ZFSBootMenu code in the image is Rust, and the image contains only what `zbm` needs.

| Ports | From |
| --- | --- |
| Recovery shell commands: `zlogtail`, `ztrace`, `zsnapshots`, `mount_esp`, `reboot` and its aliases | `bin/` |
| Error printer, debug report (`zreport`), shell completions | `libexec/recovery-error-printer`, `zreport`, `lib/zfsbootmenu-completions.sh` |
| The functions hooks source from `/lib/zfsbootmenu-core.sh` and `/lib/kmsg-log-lib.sh` | a small compatibility library, written in shell, that calls `zbm` |
| Choosing what goes into the image | `install-helpers.sh`, `packaging/dracut/module-setup.sh`, `packaging/initcpio/install/zfsbootmenu` |

The install helpers shrink to installing `zbm`, the external programs and libraries it needs, the hooks, help pages
and fonts. `zbm` can list the programs it needs, so the list is kept in one place. The xHCI teardown hook
(`hooks/teardown.d/90-xhci-unbind.sh`) stays a shell script, like user hooks.

**Done when:** the image contains no ZFSBootMenu Bash libraries except the compatibility library; the hooks in
`contrib/` work unchanged; every suite passes; image sizes are compared with the legacy image.

## Phase 10: Cutover and removal of the legacy code

**Goal:** Rust is the only implementation.

- **Default:** make Rust the default for every part, and ship one release with the legacy code still selectable as a
  fallback.
- **Removal:** after that release, remove `legacy/`, the build-time switch and the second CI build. `make install`
  installs the Rust programs at the same paths as before.
- **Documentation:** update the man pages, the online help and `docs/`, which describe behaviour and stay valid, and
  remove references to Perl and the legacy libraries.
- **Release tooling:** update `scripts/release/` for the Rust builds, and give the release notes upgrade instructions
  for distribution packagers (Rust toolchain, vendored dependencies, no Perl).

**Done when:** every suite passes on the Rust-only tree; release images are built and signed with the updated tooling;
the release notes are published.

## Out of scope

- **New features and behaviour changes:** see [ROADMAP.md](ROADMAP.md).
- **The build container scripts** (`packaging/container/`): they run on the host or in the container, not in the
  image, and stay shell.
- **The dracut and mkinitcpio glue:** both expect shell scripts, so it stays shell (Phase 9 shrinks it).
- **The test suites:** the Bash unit suites and the Python VM tests stay as they are, because they check that the
  Rust code behaves like the legacy code. Rust unit tests are added alongside them.
- **`contrib/` scripts and user hooks:** they stay shell, and keep working through the compatibility library.
