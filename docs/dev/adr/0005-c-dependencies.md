# ADR 0005: C dependencies

- **Status:** accepted, 29 October 2026 (proposed 28 October 2026)
- **Decides:** decision 12 of [ROADMAP.md](../../../ROADMAP.md#decisions)
- **Governs:** CD4 to CD12, SB3, SB4, SB10, SB11, SE6, SE13; the table "C dependencies" of ROADMAP.md

## Context

`zbm` links glibc and nothing else (`cargo xtask check-links`, a CI job, since CD1). The C code is in the programs a
Rust image runs: 45 of them, which `cargo xtask metrics` counts and which may only go down
([dependencies.md](../dependencies.md#c-libraries-and-programs-of-the-boot-runtime) lists them with the item that
replaces each). Three have left Rust images: `openssl`, `sha256sum` and `base64`, whose work in the LUKS and TPM2
paths RustCrypto's crates do (CD3, SE7). `dmesg` and `lsblk` have their replacements, `/dev/kmsg` and `/sys/block`
(CD2), and stay as the fallback until the VM suites pass them.

The programs that remain fall in three groups:

- **Small tools** with a system call or a file format behind them: `setfont`, `modprobe` and its kin, `kexec`,
  `mount`, `ykchalresp`, `objcopy`.
- **The key methods**: `cryptsetup`, `tpm2_*`, `fido2-*`, `gpg`, `clevis` with `jose` and `luksmeta`. They hold the
  secrets and parse LUKS headers and tokens from devices an attacker may control (A1, A2).
- **What has no Rust equivalent worth having**: `zfs` and `zpool`, udev, `pcscd` and `scdaemon`, the EFI stub, the
  compressors, Bash and the text tools for users.

Each program is a process per use (the boot's process count, B7), a dependency of the image (its size, B1), and, for
the key methods, a secret crossing a pipe. The other way to drop the programs is to link their libraries
(libcryptsetup, tss2-esys, libfido2), which keeps the C and adds a library that can be missing.

## Decision

**No C library is linked. Programs are replaced by Rust where the Rust keeps what users need, behind the same port,
with the program as the fallback. What has no good Rust equivalent stays a program.**

1. **glibc only.** `zbm` links glibc and GCC's unwinder (`libgcc_s`), nothing else; `check-links` fails otherwise.
   glibc itself stays: Rust's standard library needs a libc, and libc-free Rust is experimental.
2. **Replace behind the port.** A Rust implementation sits behind the port its program's adapter implements
   (`KernelLoader`, `Modules`, `Mounts`, `LuksVolumes`, `Tpm2`, `Fido2`), and the adapter keeps the program as the
   fallback until the VM suites have passed the Rust code on every case they cover. The program then leaves the image
   unless the recovery shell needs it.
3. **Fallback only where it is safe.** The Rust code falls back to the program when it cannot do the job (an
   unsupported header version, a missing system call, a token type it does not know), never after it tried a secret
   and failed: a wrong PIN is not tried twice against the TPM's lock-out, and a wrong passphrase is reported, not
   retried by `cryptsetup`.
4. **The key methods in `zbm-unlock`.** LUKS2 (CD9), TPM2 (CD10), FIDO2 (CD7), GnuPG key files (CD6), the Clevis pins
   (CD8, CD11) and YubiKey challenge-response (SB11) are written in Rust and run only in the keyholder
   ([ADR 0002](0002-trust-zones.md)). LUKS2 and TPM2 get a security review of their own before they become the default
   (SE13); until then they run behind `zbm.native=`, off by default, for tests and for users who choose them.
5. **Proven against the programs.** Each replacement has differential tests: the Rust code and the program on the same
   volumes, devices or files, in the device suites and the VM suites, with `swtpm` for the TPM. Parsers of on-disk or
   device data take `Untrusted` input and are fuzzed.
6. **Crates as decisions.** Each new crate (`devicemapper`, `aes`, `xts-mode`, `aes-gcm`, `argon2`, `pgp`, an FIDO2
   crate if one fits) is an entry of decision 1, with its reason in [dependencies.md](../dependencies.md), and goes
   through `cargo vet` once SE12 sets it up.
7. **What stays a program, and why:** `zfs`, `zpool`, `zdb` and `mount.zfs` (no Rust ZFS, and no libzfs, decision 11);
   `udevadm` and `systemd-udevd` (no mature Rust udev, and `cryptsetup` relies on device-mapper's rules); `pcscd` and
   `scdaemon` (OpenPGP cards go through PC/SC); the systemd EFI stub (signed by distributions for Secure Boot);
   `zstd` and `xz`; Bash and the text tools for hooks and the recovery shell; `cryptsetup` and `tpm2-tools` for
   enrolment and the recovery shell.

The order follows the gain and the risk: the small tools first (CD4, CD5, SB3, SB4, CD12), then the key methods from
the least to the most sensitive (SB11, CD6, CD8, CD7, then CD9 and CD10 with their review).

## Consequences

- Fewer processes per boot and a smaller image (B1, B7), and secrets that stay in one process instead of crossing
  pipes into programs.
- The Rust LUKS2 and TPM2 code is security-critical code of our own, which needs the review, the differential tests and
  the fuzzing above before anyone relies on it.
- The fallbacks keep two paths alive for a while: the adapters and their tests carry both until a program leaves.
- Users of exotic setups (LUKS1, Tang, OpenPGP cards) keep the programs; the image for them stays larger.
- No C library means no ABI to track between distributions: an image built on one keeps working, and a missing
  library cannot stop a boot.

## Alternatives considered

- **Link the libraries** (libcryptsetup, tss2-esys, libfido2, libkmod). Mature code and fewer processes, but the C
  stays in our process with the secrets, the libraries' own dependencies come along (OpenSSL, json-c, libudev), and a
  library missing from the image stops the boot. Kept as the way back if the Rust LUKS2 or TPM2 code fails its review.
- **Keep every program.** Least work and the best-tested code, but every boot pays the processes, and secrets keep
  crossing pipes.
- **Rust bindings to the C libraries** (`libcryptsetup-rs`, `tss-esapi`). Safer to call than the C API, but they are
  the first alternative with an extra layer.

## How it is checked

- `cargo xtask check-links` in CI: `zbm` links glibc only.
- The metric `c_programs` of `cargo xtask metrics`, which may only go down.
- Differential tests per replacement in the device suites (`cargo xtask test devices`) and the VM suites.
- Fuzz targets for every parser of LUKS headers, tokens, TPM2 responses and CTAP2 replies.
- SE13's review of CD9 and CD10 before `zbm.native=` is on by default.

## Where the work stands

On 29 October 2026, when the decision was accepted:

- Done: `cargo xtask check-links` fails CI when `zbm` links a library other than glibc's, and the metric `c_programs`
  is ratcheted (CD1; 45 programs on 28 October 2026). `openssl`, `sha256sum` and `base64` left Rust images (CD3, SE7);
  `dmesg` and `lsblk` have their replacements, with the programs as the fallback (CD2); ZFS filesystems are mounted
  with `mount(2)`, with `mount.zfs` as the fallback (SB3).
- To come: CD4 to CD12, SB4 and SB11, in the order above. `zbm.native=` arrives with CD9 and CD10.

## Answers of the maintainer

Accepted as proposed, on 29 October 2026:

1. **When the native key methods become the default:** LUKS2 and TPM2 after SE13's review; the others (GnuPG files,
   the Clevis TPM2 pin, YubiKey) once their VM tests pass, since their programs stay as the fallback.
2. **The fallback programs in release images:** kept until the release after the native code became the default,
   then only in images that ask for them, through a new option of `generate-zbm`'s configuration.
3. **FIDO2:** Mozilla's `authenticator` if it covers hmac-secret without its async parts; otherwise a small CTAP2
   client of our own over `hidraw`.
