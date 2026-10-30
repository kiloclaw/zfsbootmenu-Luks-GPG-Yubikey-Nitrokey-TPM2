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
   each with the digest of the bytes `kexec_file_load` reads: ZFSBootMenu reads each file once, measures it, and
   loads its copy in the state directory, so that what is measured is what loads, and is the file as it is on the boot
   environment, which the prediction reads. Policy tools (systemd-pcrlock, `tpm2_eventlog`) and documentation already know these two as "the boot
   loader's"; systemd's own (11 to 15) belong to systemd-stub and the booted system.
2. **Which banks.** Every active PCR bank of the TPM (SHA-256 at least, SHA-1 when it is active), as `TPM2_PCR_Extend`
   takes one digest per bank.
3. **The event log.** A CEL-JSON record per extension, as JSON text sequences (RFC 7464): `recnum`, `pcr`,
   `digests`, `content_type` `"zfsbootmenu"`, the content: the path of the kernel or initramfs inside the boot
   environment, the command line, or the separator. It starts with a record `start` for PCR 8 and one for PCR 9,
   which hold what the PCRs held before ZFSBootMenu's first extension: on a UEFI machine the firmware or the EFI stub
   of ZFSBootMenu's own kernel may have extended PCR 9 already, so a replay from zeros would be wrong. It is written:
   - into the initramfs the kernel loads, at `/.extra/zfsbootmenu/tpm-measure.json`, in a cpio archive appended to
     the copy of the initramfs; the initramfs is freed at `switch_root`, and `/run` is a tmpfs over the initramfs's
     own, so the host packages' dracut module and mkinitcpio hook `zfsbootmenu-measured`, in the boot environment's
     initramfs, copy it to `/run/log/zfsbootmenu/tpm-measure.json`, which the booted system keeps; and
   - to the saved logs on the ESP (`EFI/zfsbootmenu/logs/<boot>/`, ROADMAP.md #9) when they are saved.

   The log is not measured: it cannot hold its own digest, and the prediction needs the initramfs as it is on the
   boot environment. It is evidence, which the PCRs check by replay. A second boot attempt in the same run of
   ZFSBootMenu continues the log, as the PCRs continue.
4. **The separator.** Once the last LUKS volume of the boot is open, before any hook of the boot environment runs,
   ZFSBootMenu extends **PCR 9** with the event `"zbm-unsealed"`: right after the command line, before `kexec`, also
   when the files could not be measured. After a boot attempt that failed, tokens bound to PCR 9 do not unseal
   until the next start of the machine. A TPM2 token of ZFSBootMenu's volumes that includes
   PCR 9 in its policy then no longer unseals in the booted system. The documentation tells users to bind their tokens
   to PCR 9 (with 7, the Secure Boot state) for that; tokens bound to other PCRs stay unsealable, as today.
5. **The prediction.** `zbm measure predict DATASET [KERNEL]` prints the PCR 8 and 9 values ZFSBootMenu would leave
   for that boot environment and kernel (the one ZFSBootMenu picks, by `org.zfsbootmenu:kernel` or the version, when
   none is given), from the files on the dataset, mounted, with the command line of this boot's log (or
   `--command-line`), from the starts in this boot's log: what the firmware and the same ZFSBootMenu image leave there
   again. It prints them as `9:sha256=HEX`, which `systemd-cryptenroll --tpm2-pcrs=` takes. `zbm measure log` reads a
   log and replays it. The host packages ship `zbm`, so it runs on the booted system before an upgrade too.
6. **The extension itself.** `tpm2_pcrextend` (and `tpm2_getcap pcrs`, `tpm2_pcrread`) until CD10 makes
   `PCR_Extend` a call of `zbm-crypt`; with no TPM (`/dev/tpmrm0`, `/dev/tpm0`), or a TPM that refuses, the boot goes
   on, ZFSBootMenu's log says so, and the event log leaves out the extensions that did not happen (measured boot is
   evidence for the booted system, not a gate of ZFSBootMenu's). A device tree is not measured.

## Consequences

- A booted system can seal its secrets to the kernel and command line ZFSBootMenu booted, and verify them remotely
  against the log.
- Tokens bound to PCR 9 are released to ZFSBootMenu alone, not to the system it boots, which closes the "unseal again
  after kexec" gap.
- Each upgrade of a kernel changes PCR 9, so a booted system's own sealing needs the prediction before the upgrade,
  as with GRUB.
- The copies cost the kernel and the initramfs once more in memory (the state directory is a tmpfs) until the
  kernel has loaded them, and the initramfs once in the process while it is read and measured.
- SHA-1 is computed for the SHA-1 bank, which TPMs often keep active beside SHA-256: the `sha1` crate, which signs
  and checks nothing.

## Alternatives considered

- **systemd's PCRs (11, 12)**: they mean "the unified kernel image and its command line" to systemd-pcrlock and
  `systemd-measure`; extending them from a boot loader would make their predictions wrong.
- **A log passed on the kernel command line**: too small for a log, and visible to every process.
- **No separator, a dedicated PCR for ZFSBootMenu's tokens instead (PCR 15)**: simpler policies, but PCR 15 is
  systemd's machine identity; and a separator in PCR 9 needs no new convention.
- **Measuring paths instead of contents**: cheaper, but proves nothing about the bytes that ran.

## How it is checked

The VM test `integrity-measured` boots, with swtpm, a boot environment on a volume whose TPM2 token is bound to PCR 9,
which must unseal; the boot environment reads PCRs 8 and 9 (`/sys/class/tpm/tpm0/pcr-sha256`) and prints the log its
initramfs kept; they must agree by replay, the log's digests of the kernel and initramfs must be those of the files on
the dataset, which `zbm measure predict` reads, the command line measured must be the one the kernel got, and PCR 9
must have moved on from the value the token is bound to, so that it no longer unseals in the booted system. Unit tests
check the PCR values against digests computed apart, the log and the cpio archive, and that the prediction gives
what the boot extends.

## Where the work stands

Implemented, with the rest of SE10 (see Context): `zbm_crypt::measure` (the digests, the log, the replay, the cpio),
`zbm_crypt::tpm2` (`active_banks`, `read_pcrs`, `extend`), `zbm_boot::boot::measured` (the measurement before
`kexec`, the copies), `zbm measure`, and the host packages' `zfsbootmenu-measured` for dracut and mkinitcpio. The log
went to `/.extra` and the boot environment's initramfs rather than straight to `/run/log`, which the booted system
cannot see (point 3); the starts were added for UEFI machines.

## Answers of the maintainer

Accepted as proposed, on 30 October 2026:

1. **The PCRs:** 8 for the command line and 9 for the kernel and the initramfs, as GRUB.
2. **The event log:** CEL-JSON, in a cpio appended to the initramfs, at `/run/log/zfsbootmenu/tpm-measure.json` in
   the booted system, and in the saved logs on the ESP when they are saved.
3. **The separator:** PCR 9 is extended with `zbm-unsealed` after the last unseal, and the documentation advises
   binding ZFSBootMenu's TPM2 tokens to PCR 9 (with 7).
4. **The prediction:** `zbm measure predict` and `zbm measure log`, applets of `zbm`, shipped to the booted system by
   the host packages.
