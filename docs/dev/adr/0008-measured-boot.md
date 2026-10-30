# ADR 0008: Measured boot

- **Status:** accepted, 30 October 2026 (proposed the same day)
- **Decides:** feature #10 of [ROADMAP.md](../../../ROADMAP.md#fe-features-for-users), the measurement half of item SE10
- **Governs:** SE10, CD10 (`PCR_Extend` in Rust), #11 (re-sealing after firmware updates)

## Context

When ZFSBootMenu boots as an EFI image, the firmware, or systemd-stub around it, measures ZFSBootMenu into the TPM.
Nothing measures what ZFSBootMenu then boots: the boot environment's kernel, its initramfs and the command line. So
the booted system cannot seal its own secrets to "this kernel, booted with this command line", and a secret that
ZFSBootMenu itself unsealed (a TPM2 token of a LUKS volume) can be unsealed again by whatever ZFSBootMenu booted,
since the PCRs still hold the values that released it.

The first half of SE10 is done without this record: under lockdown or Secure Boot the kernel is loaded with
`kexec_file_load` alone, and a kernel it refuses is not loaded (SB4's fallback to `kexec` is off then); after an
unattended unlock, the import policy `hostid` imports as `strict`, and only a pool on a volume that unlocked itself
boots by itself.

What is measured, where, in which log, and how a user predicts the values are choices the booted systems and their
policy tools will depend on for years, so they are decided here first.

## Decision

**Before `kexec`, ZFSBootMenu extends PCRs with the kernel, the initramfs and the command line it loads, records each
event in a TCG Canonical Event Log (CEL-JSON), and extends a separator once its own unsealing is over; `zbm measure`
predicts the values for a boot environment.**

1. **Which PCRs.** GRUB's convention: the command line into **PCR 8**, the kernel and the initramfs into **PCR 9**,
   each with the SHA-256 of the bytes `kexec_file_load` reads (the files ZFSBootMenu opens, not a second read of their
   paths). Policy tools (systemd-pcrlock, `tpm2_eventlog`) and documentation already know these two as "the boot
   loader's"; systemd's own (11 to 15) belong to systemd-stub and the booted system.
2. **Which banks.** Every active PCR bank of the TPM (SHA-256 at least, SHA-1 when it is active), as `TPM2_PCR_Extend`
   takes one digest per bank.
3. **The event log.** A CEL-JSON record per extension (`recnum`, `pcr`, `digests`, `content_type` `"zbm"`, the
   content: the path of the kernel or initramfs inside the boot environment, or the command line). It is written:
   - to `/run/log/zfsbootmenu/tpm-measure.json` of the booted system, as a cpio appended to the initramfs the kernel
     loads, so that the booted system's tools find it without a mount; and
   - to the saved logs on the ESP (`EFI/zfsbootmenu/logs/<boot>/`, ROADMAP.md #9) when they are saved.

   The appended cpio is itself measured as part of the initramfs, which the log says.
4. **The separator.** Once the last LUKS volume of the boot is open, before any hook of the boot environment runs,
   ZFSBootMenu extends **PCR 9** with the event `"zbm-unsealed"`. A TPM2 token of ZFSBootMenu's volumes that includes
   PCR 9 in its policy then no longer unseals in the booted system. The documentation tells users to bind their tokens
   to PCR 9 (with 7, the Secure Boot state) for that; tokens bound to other PCRs stay unsealable, as today.
5. **The prediction.** `zbm measure predict DATASET [KERNEL]` prints the PCR 8 and 9 values ZFSBootMenu would leave
   for that boot environment and kernel, with its command line, from the files on the dataset; `zbm measure log` reads
   a CEL-JSON log and replays it. The host packages ship `zbm`, so it runs on the booted system before an upgrade too.
6. **The extension itself.** `tpm2_pcrextend` until CD10 makes `PCR_Extend` a call of `zbm-crypt`; with no TPM, or a
   TPM that refuses, the boot goes on and the log says so (measured boot is evidence for the booted system, not a
   gate of ZFSBootMenu's).

## Consequences

- A booted system can seal its secrets to the kernel and command line ZFSBootMenu booted, and verify them remotely
  against the log.
- Tokens bound to PCR 9 are released to ZFSBootMenu alone, not to the system it boots, which closes the "unseal again
  after kexec" gap.
- Each upgrade of a kernel changes PCR 9, so a booted system's own sealing needs the prediction before the upgrade,
  as with GRUB.
- The appended cpio costs one copy of the initramfs in memory before `kexec` (the kernel reads one initramfs file).

## Alternatives considered

- **systemd's PCRs (11, 12)**: they mean "the unified kernel image and its command line" to systemd-pcrlock and
  `systemd-measure`; extending them from a boot loader would make their predictions wrong.
- **A log passed on the kernel command line**: too small for a log, and visible to every process.
- **No separator, a dedicated PCR for ZFSBootMenu's tokens instead (PCR 15)**: simpler policies, but PCR 15 is
  systemd's machine identity; and a separator in PCR 9 needs no new convention.
- **Measuring paths instead of contents**: cheaper, but proves nothing about the bytes that ran.

## How it is checked

A VM test with swtpm boots a boot environment, reads PCRs 8 and 9 there (`tpm2_pcrread`), compares them with
`zbm measure predict` run before the boot and with the replayed log, and checks that a TPM2 token bound to PCR 9 no
longer unseals in the booted system.

## Where the work stands

Accepted; the work has not started. The rest of SE10 is done (see Context).

## Answers of the maintainer

Accepted as proposed, on 30 October 2026:

1. **The PCRs:** 8 for the command line and 9 for the kernel and the initramfs, as GRUB.
2. **The event log:** CEL-JSON, in a cpio appended to the initramfs, at `/run/log/zfsbootmenu/tpm-measure.json` in
   the booted system, and in the saved logs on the ESP when they are saved.
3. **The separator:** PCR 9 is extended with `zbm-unsealed` after the last unseal, and the documentation advises
   binding ZFSBootMenu's TPM2 tokens to PCR 9 (with 7).
4. **The prediction:** `zbm measure predict` and `zbm measure log`, applets of `zbm`, shipped to the booted system by
   the host packages.
