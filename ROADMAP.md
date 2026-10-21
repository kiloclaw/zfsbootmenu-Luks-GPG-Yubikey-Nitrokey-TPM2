# Roadmap

This roadmap lists features to add to ZFSBootMenu during and after its port to Rust. Each entry is a proposal, not a
commitment. [CONTRIBUTING.md](CONTRIBUTING.md) explains how the port itself proceeds: the Bash and Perl code in
`legacy/` ships until a Rust part matches it.

Each feature lists why it is useful, how it could work and when it counts as done. Effort is a rough estimate for one
developer who knows the code: **Small** is under a week, **Medium** a few weeks, **Large** a few months.

## Principles

- The legacy code is the reference. A Rust part first behaves like the code it replaces; new behaviour comes after.
- Features that change how a system boots are off unless configured. Warnings and better messages can be on by default.
- Every feature comes with tests in `tests/` (unit or VM tests) and with documentation in `docs/`.
- Features marked **security** document what they protect against, and get a review before they ship.
- Kernel command line options, configuration files and `org.zfsbootmenu:*` properties stay compatible with upstream
  ZFSBootMenu, so boot environments work with either.

## Overview

| # | Feature | Phase | Effort | Status |
| --- | --- | --- | --- | --- |
| 1 | [Wipe secrets from memory](#1-wipe-secrets-from-memory) | 1 | Small | Proposed |
| 2 | [Unlock with whatever arrives first](#2-unlock-with-whatever-arrives-first) | 1 | Medium | Proposed |
| 3 | [Parallel startup](#3-parallel-startup) | 1 | Medium | Proposed |
| 4 | [Clear unlock errors](#4-clear-unlock-errors) | 1 | Medium | Proposed |
| 5 | [Reproducible images](#5-reproducible-images) | 1 | Small | Proposed |
| 6 | [Check the boot environment's ZFS before booting](#6-check-the-boot-environments-zfs-before-booting) | 2 | Small | Proposed |
| 7 | [Boot counting with automatic fallback](#7-boot-counting-with-automatic-fallback) | 2 | Medium | Proposed |
| 8 | [Locked menu](#8-locked-menu) | 2 | Medium | Proposed |
| 9 | [Saved boot log and bug-report bundle](#9-saved-boot-log-and-bug-report-bundle) | 2 | Small | Proposed |
| 10 | [Measured boot](#10-measured-boot) | 3 | Medium | Proposed |
| 11 | [Re-seal the TPM after firmware updates](#11-re-seal-the-tpm-after-firmware-updates) | 3 | Medium | Proposed |
| 12 | [Built-in menu interface](#12-built-in-menu-interface) | 4 | Large | Proposed |
| 13 | [Own init](#13-own-init) | 4 | Large | Proposed |
| 14 | [Built-in SSH server for remote unlock](#14-built-in-ssh-server-for-remote-unlock) | 4 | Large | Proposed |

Status is one of Proposed, In progress or Done. Update it in the pull request that changes it.

## Phase 1: improvements built into the port

These come with porting the code they touch, at little extra cost.

### 1. Wipe secrets from memory

**Security.**

- **Why:** Bash keeps passphrases and key material in variables, here-strings and pipe buffers that cannot be reliably
  erased. Copies can remain in RAM after ZFSBootMenu hands over to the booted kernel. ZFSBootMenu already closes LUKS
  mappings before booting, which wipes the volume keys, but not the secrets used to unlock them.
- **How:** hold secrets in types that erase themselves when dropped (`zeroize`, `secrecy`), lock their pages in RAM
  (`mlock`), and hand them to libcryptsetup and helper tools through memory or pipes: never through command-line
  arguments, environment variables or files. Wipe everything that is left before `kexec`.
- **Done when:** a test unlocks volumes with a known passphrase and key file, then searches process memory, command
  lines, environments and tmpfs for them and finds nothing.

### 2. Unlock with whatever arrives first

- **Why:** the Bash code tries unlock methods in turn. A FIDO2 key or a USB key stick plugged in while the passphrase
  prompt is shown is not noticed until the prompt ends, and a slow TPM delays the prompt.
- **How:** run one task per method (TPM2, FIDO2, YubiKey challenge-response, GnuPG smart card, key file or key device,
  passphrase) and watch device hot-plug through kernel uevents. The first method that unlocks the volume cancels the
  others. Configuration keeps its meaning: a volume configured for a key file alone never shows a prompt. PIN attempts
  against the TPM stay one at a time, so they do not use up the TPM's lockout counter.
- **Done when:** the hot-plug and lockout VM tests (`complex-hotplug`, `complex-hotplug-auto`, `firmware-usbkey`,
  `firmware-usblate`, `keys-lockout`) pass, and a new test plugs in a key while the passphrase prompt is shown.

### 3. Parallel startup

- **Why:** kernel discovery mounts one boot environment after another, and several LUKS volumes that share a
  passphrase each run a slow Argon2id key derivation in turn. Systems with many boot environments or disks wait the
  longest.
- **How:** scan boot environments with a thread pool, and unlock volumes in parallel. Each Argon2id derivation can use
  up to 1 GiB of RAM, so the number of parallel unlocks depends on free memory. Pool imports stay serial where the
  `zpool` command needs it.
- **Done when:** the VM tests measure the time from kernel start to the menu, it drops on a layout with four LUKS
  members and on a pool with twenty boot environments, and the menu shows the same entries as before.

### 4. Clear unlock errors

- **Why:** several different failures look alike today. Users need to know whether the passphrase was wrong, the
  volume has no key slot or token for the method, the LUKS header is damaged, the TPM refused because the system
  changed (PCR mismatch), the FIDO2 key or GnuPG card is missing, or its PIN was wrong. The TPM lockout is already
  detected.
- **How:** call libcryptsetup through `libcryptsetup-rs` and the TPM through `tss-esapi` instead of parsing tool output,
  and map their return codes to one error type with a message and a hint for each case.
- **Done when:** every failure case in `tests/luks/unit/edge.sh` produces its own message, and the test checks it.

### 5. Reproducible images

- **Why:** the same source and build container should give the same bytes, so that anyone can check a published image
  and sign it. Today two builds from the same tree have identical contents but different bytes, because the initramfs
  records timestamps.
- **How:** set `SOURCE_DATE_EPOCH` from the last commit and pass dracut's `--reproducible` option; check the EFI bundle
  that `objcopy` writes for timestamps. This can start in the Perl `generate-zbm` before it is ported. The Rust
  generator writes the archive itself, with fixed timestamps, owners and file order.
- **Done when:** CI builds the release image twice and the two images are identical.

## Phase 2: first new features

### 6. Check the boot environment's ZFS before booting

- **Why:** ZFSBootMenu warns when its own ZFS does not support a pool's features. Nothing warns when the selected boot
  environment's ZFS is too old for the features active on its root pool. That boot environment then fails to import
  its root pool after `kexec`, away from ZFSBootMenu's recovery tools.
- **How:** before booting, read the ZFS module version that the boot environment's initramfs (or, failing that, its
  module tree) contains for the selected kernel. Look up the features that version supports in the `compatibility.d`
  files that OpenZFS ships (`openzfs-2.2-linux` and so on), and compare them with the features that are active on the
  pool. Features that are enabled but unused do not block an import. List the features the boot environment does not
  support, and ask before booting.
- **Done when:** a VM test with an old ZFS in the boot environment, like `complex-oldzfs`, shows the warning before
  booting, and boot environments with a recent enough ZFS boot without it.

### 7. Boot counting with automatic fallback

- **Why:** after an update that does not boot, an unattended or remote machine stays down until someone reaches the
  console.
- **How:** a boot environment or kernel can be marked for trial, from the menu or with a tool in the booted system.
  ZFSBootMenu decrements a counter before each trial boot, and when it reaches zero boots the last known-good boot
  environment instead. A small service in the booted system marks the boot good, as `systemd-bless-boot` does for
  systemd-boot. ZFSBootMenu imports pools read-only, so the counter is kept in an EFI variable; systems without UEFI
  are not supported at first.
- **Done when:** a VM test boots a boot environment that crashes and ZFSBootMenu falls back after the configured number
  of tries, and a boot environment whose service marks the boot good keeps booting.

### 8. Locked menu

**Security.**

- **Why:** anyone at the console can edit the kernel command line (for example `init=/bin/sh`), open the recovery
  shell, or boot another boot environment or snapshot. On a system that unlocks with a TPM and no PIN, that means
  anyone who can start the machine reaches the decrypted data.
- **How:** optionally require a PIN (stored as an Argon2id hash in the image) or a FIDO2 key before editing the command
  line, opening the recovery shell, choosing anything but the default boot environment, or managing snapshots. The
  default boot stays unattended. The lock only helps when the image itself cannot be replaced, so the documentation
  pairs it with Secure Boot and signed images.
- **Done when:** VM tests check that each locked action asks for the PIN, that a wrong PIN is refused with a delay,
  and that the default boot still needs no input.

### 9. Saved boot log and bug-report bundle

- **Why:** ZFSBootMenu logs to the kernel log of its own kernel, which is lost when it boots another kernel or the
  machine restarts. Failures on unattended machines cannot be diagnosed afterwards.
- **How:** optionally write the log to the EFI system partition, keeping the last few, and mount the partition only
  for the write. A `zbm report` command in the recovery shell writes pool status, LUKS header summaries, token and TPM
  status, versions and the log to the EFI system partition or a USB stick. It never includes key material.
- **Done when:** a VM test restarts the machine and finds the previous log on the EFI system partition, and a test
  checks that a report contains none of the test passphrases or keys.

## Phase 3: TPM

### 10. Measured boot

**Security.**

- **Why:** the firmware measures ZFSBootMenu itself into the TPM when it boots as an EFI image, but nothing measures the
  kernel, initramfs and command line that ZFSBootMenu boots. The booted system cannot tie its own TPM secrets to
  "this kernel, booted with this command line".
- **How:** before `kexec`, extend a PCR with the hashes of the kernel, the initramfs and the command line, and record
  the events in a log the booted system can read. The PCRs to use (GRUB uses PCR 8 for command lines and PCR 9 for
  files) and the log format are to be decided and documented. Extend only after ZFSBootMenu's own TPM unlocks, so the
  booted system cannot unseal those secrets again, much as systemd measures boot phases into PCR 11. A tool predicts
  the PCR values for a boot environment, so policies can be prepared before an upgrade.
- **Done when:** a VM test with a software TPM compares the PCR values after booting with the prediction, and checks
  that ZFSBootMenu's TPM tokens no longer unseal after the extension.

### 11. Re-seal the TPM after firmware updates

**Security.**

- **Why:** a firmware update changes PCR values. TPM unlocking then fails, and the passphrase is needed at every boot
  until someone re-enrolls the token from the booted system.
- **How:** when a TPM2 token fails because of a PCR mismatch and the volume is then unlocked with a passphrase or
  recovery key, show which PCRs changed and offer to re-seal the token to the current values. Re-sealing trusts the
  current state of the machine, so it needs explicit confirmation. The same code gives `zluks` commands to enroll
  TPM2 and FIDO2 tokens and recovery keys.
- **Done when:** a VM test changes a PCR the token is bound to, unlocks with the passphrase, re-seals, and the next
  boot unlocks with the TPM alone.

## Phase 4: long term

### 12. Built-in menu interface

- **Why:** the menu runs `fzf` with Bash previews, which starts many processes and depends on the `fzf` version in the
  image. A built-in interface behaves the same everywhere and can add mouse support, themes, and a plain mode for
  serial consoles and screen readers.
- **How:** `ratatui` for the layout and `nucleo` (the fuzzy matcher of the Helix editor) or `skim` for filtering. The
  key bindings stay as documented in `docs/online/`, so the help pages and users' habits stay valid.
- **Done when:** everything in the online help works, and the VM tests drive the menu over the serial console as they
  do today.

### 13. Own init

- **Why:** dracut and mkinitcpio add size and boot time, and they differ between distributions. For example, Ubuntu's
  dracut builds host-only images by default, which left the ZFS and dm-crypt modules out of container builds.
- **How:** a Rust program runs as PID 1 in a minimal initramfs. It mounts `/proc`, `/sys` and `/dev`, loads modules
  using `modules.dep`, waits for devices through kernel uevents instead of udev, sets up the console and keymap, and
  starts the menu. The Rust `generate-zbm` writes the image itself. dracut and mkinitcpio stay supported until the new
  image passes every test. The risk is hardware that dracut handles today: USB keyboards, NVMe, firmware files and
  microcode.
- **Done when:** every VM test passes with the new image type, and its size and boot time are measured against dracut
  images.

### 14. Built-in SSH server for remote unlock

**Security.**

- **Why:** remote unlocking needs dropbear, `dracut-crypt-ssh` and NetworkManager in the image, assembled by
  `contrib/remote-ssh-build.sh`.
- **How:** an SSH server written in Rust (`russh`) with key authentication only and host keys created at build time.
  The network is configured by DHCP or from the kernel command line (`ip=`). A session offers unlocking and the menu;
  a shell only when configured.
- **Done when:** a VM test unlocks a pool over SSH, password logins and unknown keys are refused, and a security review
  is done.

## Not planned

- **Bindings to `libzfs`.** Its interface changes between OpenZFS releases. The port calls the `zfs` and `zpool`
  commands as the Bash code does, and reads their JSON output (`-j`, OpenZFS 2.3 and later), falling back to `-H -p`
  for older ZFS in images built on a host. `libzfs_core`, which is more stable, can serve the few operations where
  speed matters.
- **Changes that break compatibility with upstream ZFSBootMenu** command line options, configuration or properties.
