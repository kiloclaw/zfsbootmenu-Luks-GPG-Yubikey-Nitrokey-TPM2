# ADR 0009: The hook root on machines that unlock themselves

- **Status:** accepted, 31 October 2026 (proposed the same day)
- **Decides:** the last item of SE10 in [ROADMAP.md](../../../ROADMAP.md), the threat model's R1
- **Governs:** `zbm.hookroot`, the start-up stage `40-early-hooks`, measured boot ([ADR 0008](0008-measured-boot.md))

## Context

`zbm.hookroot=device//path` names a file system whose hooks ZFSBootMenu adds to its own: in `40-early-hooks` it mounts
the file system read-only, copies each stage directory's hooks into the image's hook directories, and runs the
`early-setup.d` hooks right away. All of it happens before any LUKS volume opens or any pool imports, and the hooks run
as root.

Whoever controls the kernel command line (A1) or the device it names (A2, A3: a stick with the same label or UUID)
therefore runs code in ZFSBootMenu. On a machine an attended person boots, that person sees what happens. On one whose
volumes unlock themselves with the TPM, nobody does, and the code runs before the TPM releases the keys: it can wait
for them, read them, or start a process that outlives the unlock. SE10 made the rest of an unattended boot hold to what
unlocked (`hostid` imports as `strict`, only a pool on an unlocked volume boots by itself), but the hook root comes
before all of that.

Skipping the hook root's later stages after an unattended unlock does not help: its `early-setup.d` hooks have run by
then, and a hostile hook can rewrite the later ones or replace programs in the image's tmpfs. What protects the
machine has to judge the hook root before its first hook runs, or keep the keys from it.

A second gap, of ADR 0008's: a TPM2 token bound to PCR 9 unseals against PCR 9 as ZFSBootMenu finds it before its
first extension. Enrolled from the booted system with `--tpm2-pcrs=7+9`, it binds to PCR 9 as the booted system sees
it, after ZFSBootMenu's extensions, and never unseals. The enrolment needs the value ZFSBootMenu unseals at, which the
event log's start records hold; nothing prints it yet.

## Options

1. **Measure the hook root into PCR 9 before its hooks run.** Each hook copied from the hook root (its stage, its name,
   its contents) is extended into PCR 9 and recorded in the event log, in copy order, before `early-setup.d` runs. A
   token bound to PCR 9 then unseals only with the hooks it was enrolled with: a stranger's hook root makes the TPM
   refuse, and the machine asks for a passphrase, so a person is there. The hooks still run; what they cannot do is
   get the key. Changing the hooks means enrolling the token again, with the value `zbm measure` prints. Machines
   without a TPM, or tokens not bound to PCR 9, are as today.
2. **Pin the hook root's hooks in the image.** The image lists the SHA-256 of each hook it accepts from the hook root
   (`generate-zbm` reads them from a directory of the build host); a hook not on the list is not copied, and is
   logged. Nothing of a stranger runs, on any machine; every change of a hook means a new image, which takes away most
   of what a hook root is for.
3. **A build option that refuses the hook root.** `zfsbootmenu_hookroot=no` makes the image ignore `zbm.hookroot`,
   and the documentation advises it for machines that unlock themselves. Simple; such machines then have no hook root.
4. **Document the risk only**, as R1 does now.

## Proposal

Option 1, with the enrolment gap closed the same way:

- the hook root's hooks are measured into PCR 9 (events `hook`, the value `<stage>/<name>`, the digest of the file's
  contents) after the start records and before the first `early-setup.d` hook; the log reaches the booted system as in
  ADR 0008;
- `zbm measure` prints the values ZFSBootMenu unseals at: PCR 9 after the start and the hook root, before the boot's
  files and the separator (`zbm measure unseal`, from this boot's log), for
  `systemd-cryptenroll --tpm2-pcrs=7+9:sha256=<value>`, and the documentation says to enrol so;
- a hook root that cannot be measured (no TPM, an extension refused) is copied and run as today, and the log says so:
  measured boot stays evidence, not a gate (ADR 0008, point 6). The gate is the token's policy.

## Consequences

- On a machine that unlocks itself with a token bound to PCR 9, a hook root someone else put there cannot get the
  key; the boot falls back to a passphrase.
- Changing the hook root's hooks needs the token enrolled again, as a firmware update needs it for PCR 7.
- The image's own hooks are not measured here: the firmware or systemd-stub measures the image.

## How it is checked

The VM test `integrity-hookroot`: `auto0`'s token is enrolled bound to PCR 9 with the value `zbm measure unseal`
printed for a hook root holding one `early-setup.d` hook; the machine boots by itself with that hook root, and asks for
the passphrase with a hook root whose hook differs. Unit tests check the events, their order and the values printed.

## Where the work stands

Accepted; the work has not started. Until `zbm measure unseal` comes, the documentation gives the value from the event
log's start record for PCR 9.

## Answers of the maintainer

Accepted as proposed, on 31 October 2026:

1. **The option:** the hook root is measured into PCR 9 before its hooks run (option 1).
2. **The PCR:** 9, with the boot's files, as ADR 0008.
3. **The enrolment value:** `zbm measure unseal` prints PCR 9 as ZFSBootMenu unseals at, for
   `systemd-cryptenroll --tpm2-pcrs=7+9:sha256=<value>`.
