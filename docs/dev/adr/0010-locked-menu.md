# ADR 0010: The locked menu and the one gate to a shell

- **Status:** accepted, 31 October 2026 (proposed the same day)
- **Decides:** feature #8 of [ROADMAP.md](../../../ROADMAP.md#fe-features-for-users), item SE9, the threat model's R8
- **Governs:** `RecoveryPolicy` and `ShellGate` ([ADR 0002](0002-trust-zones.md), point 1 of its answers), the menu's
  actions, the supervisor's recovery after a crash, the menu over SSH, `argon2` as a dependency (decision 1)

## Context

Whoever sits at the console of a ZFSBootMenu machine is root: the recovery shell is a key away, the command line of
the boot can be edited (`init=/bin/sh`), and any boot environment, snapshot or kernel can be booted. On a machine that
asks for a passphrase this gives nothing the person did not have: they typed the key. On one that unlocks itself with
the TPM (SE10, ADR 0008), it gives the decrypted data to anyone who can switch the machine on. The same holds over SSH
(R8): a login is root in the menu and the shell.

There is no single place a shell starts today. The start-up program opens the recovery shell when a stage fails (no
pool, no ZFS module, an import that fails) and when the menu asks for it (`Action::Shell`); the supervisor opens it
after the start-up program crashed, and its fallback after the shell failed to start; and `Action::Chroot`, which the
Rust runtime does not run yet (UI10), will run a shell in a boot environment. A lock has to close all of them, and stay
closed when the menu crashes.

A lock only means something when the image and its command line cannot be changed. Whoever edits ZFSBootMenu's own
kernel command line can add `rdinit=/bin/sh`, and the kernel runs a shell before ZFSBootMenu starts; whoever replaces
the image replaces the lock. The lock is for images booted as a signed unified kernel image under Secure Boot, whose
stub refuses a changed command line. Without that it is a speed bump, which the documentation and the log must say.

The image is readable by anyone: it lies on the unencrypted EFI system partition. Whatever checks the PIN there can be
copied and attacked offline, as often as the attacker likes.

## Options

What the lock covers:

1. **Everything but the default boot.** Locked: the recovery shell (every way to it), `chroot`, editing the command
   line, booting anything but the default environment with its default kernel (another environment, kernel or
   snapshot), every change (snapshots, rollback, clones, the default, read/write and forced imports, the checkpoint,
   re-sealing), and typing a key file path. Open: the countdown and the default boot, Escape and looking around
   (lists, logs, diffs, help), unlocking a volume with its own passphrase, reboot, power off and the firmware setup
   (which has its own password). As #8 proposed.
2. **The shell and the command line only.** Simpler, but booting an old snapshot, or one with a known root password,
   stays open, and so do destructive changes.
3. **The shell only.** Leaves `init=` on the command line open, so it protects nothing.

Where the PIN is checked:

1. **An Argon2id hash in the image.** Works on every machine. The hash can be attacked offline, so the PIN must be
   long: the build refuses fewer than 8 characters, and the documentation advises a passphrase of a few words.
   Parameters as OWASP's (19 MiB, 2 passes) or stronger, written into the hash so they can rise later.
2. **The TPM.** The PIN is the authorisation of a sealed object; the TPM's dictionary-attack lockout limits guessing
   across reboots, so a short PIN is safe. Needs a TPM, an enrolment from the booted system, and fails with the TPM
   (a cleared TPM locks the menu for good, until a new image).
3. **Both:** the TPM's check where the image was enrolled with one, the hash otherwise.

Who else is let in:

1. **Whoever typed a LUKS passphrase that unlocked a volume this boot**, as well as whoever knows the PIN. They hold
   the data's key already, and can do with the disk whatever the menu would let them; an attended machine then never
   shows the PIN screen.
2. **The PIN alone.** Stricter, and asks twice on machines that ask for a passphrase.

## Proposal

The first option of each, built in two steps.

**Step 1, the gate (SE9's first half, no new crate).** `ShellGate::open(&Authorized)` in `zbm-app` becomes the only
way to start an interactive shell or `chroot`; `Authorized` is a zero-sized token whose constructor is private to
`zbm_app::auth`. Without a lock in the image, `auth::unlocked()` mints it at once, and nothing changes for users. The
start-up program's recovery shell, the menu's `Shell` and `Chroot`, and the supervisor's shell and its fallback go
through it, and `Chroot` will when UI10 builds it; `xtask metrics`' `shell_starts` counts the gate's own start alone,
and is ratcheted there. The supervisor's crash recovery takes a `RecoveryPolicy`: `Open` gives the shell as today, `Locked`
starts `zbm lock prompt` in its place, which asks for the PIN on the terminal (no echo), and opens the shell through the
gate once it is right. The supervisor itself still parses nothing and holds no secret.

**Step 2, the lock (#8, with `argon2`).**

- **The image.** `zfsbootmenu_lock_hash="$argon2id$v=19$m=…"` in the image's configuration (`generate-zbm` and the
  dracut and mkinitcpio shims), written by `generate-zbm --lock-hash`, which reads the PIN twice without echo and
  refuses one shorter than 8 characters. The hash goes into the image as `/etc/zfsbootmenu/lock`, mode 0400. Nothing on
  the command line turns the lock off or changes it; a locked image ignores `zbm.hookroot`, and logs a warning at start-up
  when neither lockdown nor Secure Boot is on (`kexec::checked_boot`), since its command line may then have been changed.
- **The check.** The keyholder (Z1, ADR 0002, point 2) checks a PIN: `UnlockRequest` gains `Verify { credential }`, its
  outcome `Verified` or `Refused`. The PIN moves into the request and is wiped there; the comparison is constant-time.
  Each wrong PIN doubles a delay before the next answer, from 1 second to 30, kept by the keyholder for the boot. A
  passphrase that unlocked a volume this boot (`UnlockOutcome::Unlocked` from a typed credential, not a token) counts as
  verified too.
- **The session.** The session (Z2) holds the authorisation: it checks each action a client sends against the lock
  (ADR 0002, point 3), and answers a locked one with `Asked { Pin }`; the menu shows the PIN dialog, and the action runs
  once the keyholder says `Verified`. The authorisation lasts until the boot, or until the menu has been idle for 5
  minutes, or the recovery shell ends; then the menu is locked again. The same holds for the menu over SSH, which is a
  client of the same session (R8).
- **The menu.** Locked actions stay visible, marked with a lock in the footer and the palette, so that nobody wonders
  where they went; choosing one asks for the PIN. The countdown and the default boot never ask.

## Consequences

- A machine that unlocks itself keeps its data from whoever can switch it on, as long as its image is signed and booted
  under Secure Boot; the default boot still needs nobody.
- An attended machine is as today: the passphrase typed at start-up opens everything.
- Forgetting the PIN locks out only the menu, not the data: the default boot works, the passphrase still opens the menu,
  and a new image removes the lock.
- The keyholder learns a second kind of request; the session learns to refuse actions. The front ends only draw a
  dialog they already have.
- `argon2` (RustCrypto, beside SE7's crates) becomes a dependency of `zbm-crypt`, used by the keyholder and by
  `generate-zbm`.

## Alternatives considered

- **The PIN checked in the supervisor or the session**: simpler, but puts a secret in a zone that ADR 0002 keeps free
  of secrets.
- **The PIN on a ZFS property**: anyone who plugs in a pool, or imports one elsewhere, writes it.
- **FIDO2 instead of a PIN**: left to CD7, which brings CTAP2 over `hidraw`; a security key can then authorise as the
  PIN does.
- **Lock the countdown too**: the default boot is what an unattended machine is for.

## How it is checked

- Step 1: unit tests that every shell start in `zbm-startup`, `zbm-app` and the supervisor goes through the gate, and
  `shell_starts` at one; a FakeSystem test that a crash under `Locked` starts the prompt, not the shell.
- Step 2: the VM test `integrity-locked`, on the integrity disks with a locked image: the default boot needs no input
  with the TPM; Escape, then the recovery shell, asks for the PIN; a wrong PIN is refused and the next answer comes
  after the delay; the right one opens the shell; editing the command line, another environment and a rollback each
  ask; without the TPM, the typed passphrase opens the menu without the PIN; a crash of the start-up program (`zbm debug
  panic`) leads to the PIN prompt, not to a shell. Unit tests for the hash, the delay and each action's lock.

## Where the work stands

Accepted; the work has not started. Neither the gate nor `RecoveryPolicy` exists yet. The start-up program opens its shell
with `programs::interactive_shell` (`zbm_startup::shell`), and the supervisor spawns its shell and the fallback itself
(`zbm::supervisor::running`).

## Answers of the maintainer

Accepted as proposed, on 31 October 2026:

1. **What the lock covers:** everything but the default boot (option 1).
2. **Where the PIN is checked:** an Argon2id hash in the image, of at least 8 characters, checked by the keyholder
   (option 1).
3. **Who else is let in:** whoever typed a passphrase that unlocked a volume this boot, as well as whoever knows the
   PIN (option 1).
4. **`argon2`:** approved as a dependency (decision 1), for the keyholder and `generate-zbm`.
