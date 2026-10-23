# Users, tasks and scenarios

The menu exists for a few minutes at a time, between the firmware and an operating system, often when something has
gone wrong. Its users are not exploring: they came to do one thing, and they want to be done. This page says who they
are, what they come to do, and the situations they do it in; the rest of the design follows from it.

## Users

**The desktop user.** One pool, one or two boot environments, an encrypted root (native ZFS encryption or LUKS). Boots
every day and never wants to see the menu; sees it after a bad upgrade, to go back. Knows ZFS a little. Monitor and
keyboard, a large terminal.

**The server administrator.** Several pools, boot environments per release, a serial console through IPMI or a
hypervisor at 115200 baud, often 80×24 or less, `TERM=vt100` or `TERM=linux`, sometimes without colour. Sees the menu
when a disk was replaced, a pool came from another host, or a boot failed at 3 a.m. Knows ZFS well; wants every
decision shown and nothing done behind their back.

**The security-minded laptop user.** LUKS with a TPM2 PIN, a FIDO2 key or a YubiKey, and a passphrase as fallback;
sometimes a GnuPG-encrypted key file. Meets the menu at every boot, at the unlock prompt. Wants to know which method
is being asked for, and what to do when the key is not plugged in.

**The tinkerer.** Many boot environments, hundreds of snapshots, several kernels each. Clones, diffs, sets defaults,
boots an older kernel to bisect a regression. Uses the menu more than anyone, and knows the `fzf` menu's keys.

**Anyone, during a failed boot.** Any of the above, under stress: a pool will not import, a key does not load, a
volume is missing, the boot environment's initramfs drops to a shell. Needs to read what failed and what can be done
next, without the manual.

## Core tasks

These shape the layout: each starts from the main view, and each has a keystroke budget that the prototype and R5
measure (see [Measures](#measures)).

| Task | What the user does | Budget |
| --- | --- | --- |
| T1. Boot the default | Nothing: the countdown runs out | 0 keys |
| T2. Boot another environment | Stop the countdown, choose it, boot | 4 keys, plus filter letters |
| T3. Boot another kernel | Stop, open the kernels of the selection, choose, boot | 5 keys |
| T4. Roll back, or boot a snapshot | Open the snapshots, choose one, choose the action, confirm | 6 keys, plus the typed confirmation of a rollback |
| T5. Unlock volumes | Answer the prompt with the method offered, or choose another | The secret and Enter |
| T6. Edit the command line once | Open the editor on the selection, edit, boot | 3 keys, plus the edit |
| T7. Recover a failed boot | Read the problem, choose a next step (retry, import another way, shell, logs) | 2 keys from the problem view |

Secondary tasks, reachable from the main view or the command palette in at most two steps: set the default
environment or kernel, create a snapshot, clone or duplicate a snapshot, diff two snapshots, pool and volume status,
import read/write, rewind to a checkpoint, chroot, logs, power off, reboot, firmware setup.

## Scenarios

Each scenario is a script for the [usability sessions](usability.md) and a VM test of R5.

1. **Monday morning.** The desktop user turns the machine on and goes to make coffee. The default environment boots
   after the countdown. *Watch for:* nothing on the screen that asks for attention; the countdown visible and
   readable from a distance.
2. **The bad upgrade.** Yesterday's upgrade broke the graphics driver. The desktop user stops the countdown, finds
   yesterday's snapshot of the root, and boots it, first as a clone to check it, then by rolling back. *Watch for:*
   whether they find the snapshots from the main view; whether they understand what a rollback destroys before they
   confirm; whether the safety snapshot is noticed.
3. **The disk swap.** The administrator replaced a failed disk over a serial console at 80×24 without colour. The
   pool imports `DEGRADED`; another pool belongs to a host with a different host ID. They read the pool status, import
   the foreign pool with the matching host ID, and boot. *Watch for:* whether the start-up view explains the import
   problem and offers the right choices; whether the screen is readable in monochrome and ASCII.
4. **The missing key.** The laptop user boots without the YubiKey. ZFSBootMenu asks for the FIDO2 key, then offers the
   passphrase. *Watch for:* whether the dialog says which method it is waiting for, how long, and how to skip to
   another one; whether a wrong PIN and the TPM lockout are explained.
5. **The kernel regression.** The tinkerer boots the second-newest kernel of the default environment, then makes it the
   default kernel. *Watch for:* whether the kernels view shows which kernel is the default and which was booted last.
6. **The rescue target.** The administrator boots once with `systemd.unit=rescue.target` added. *Watch for:* whether
   the edit is clearly temporary; whether a typo in the command line is caught before the boot.
7. **Many snapshots.** The tinkerer looks for the snapshot taken before a particular package upgrade among 400. *Watch
   for:* whether the command palette or the filter finds it by part of its name; whether sorting by date is found.
8. **The failed boot.** The ZFS key of the root is typed wrong three times. *Watch for:* whether the error view says
   what failed and offers retry, another environment and the shell, and whether the user picks the right one.

## What the `fzf` menu taught

From the issue tracker, the documentation and the code of the legacy menu:

- Keys differ between screens (`MOD+D` is *set bootfs* on the main screen, *diff* among snapshots and *set default*
  among kernels; `Enter` boots on two screens and duplicates on the third). The new menu keeps one meaning per key.
- Start-up and unlock prompts are printed on the console before the menu exists, in another style, and a failed
  import ends in a bare retry loop. The new menu shows start-up as one of its views.
- Destructive actions are one key and a yes/no question away. The new menu shows what will be lost and asks for a
  typed confirmation.
- Status (read-only pools, warnings in the log) is shown by colour alone (`[!]` yellow or red, red names). The new menu
  writes every state as text and uses colour as a second signal.
- Long names and narrow consoles truncate the important part. The new menu lays out for 52×20 first.

## Measures

The prototype measures each core task as the number of key presses from the main view (T5 and T7 from their dialog)
in the shortest path, and compares it with the `fzf` menu; the tests of the prototype keep those numbers from
growing. The usability sessions add time on task, errors (a wrong action started, a dialog dismissed unread) and
whether help was needed.

The prototype's shortest paths, from `crates/zbm-tui/src/menu/demo/measures.rs`, which fails when one grows. Text
typed (filter letters, a name, a secret, an edit) is not counted. The `fzf` column is counted by hand from that
menu's key help, to be checked in the sessions.

| Task | Path in the prototype | Keys | Budget | `fzf` menu |
| --- | --- | --- | --- | --- |
| T1 | nothing | 0 | 0 | 0 |
| T2 | Down, Enter; or `/`, letters, Enter | 2 | 4 | 3 (Esc to stop the countdown, Down, Enter) |
| T3 | `k`, Down, Enter | 3 | 5 | 4 (Esc, `MOD+K`, Down, Enter) |
| T4, boot a snapshot | `s`, `o` (newest first), `b` | 3 | 6 | no direct path: clone it, then boot the clone |
| T4, roll back | `s`, `o`, `r`, the name, Enter | 4 | 6 | Esc, `MOD+S`, choose, `MOD+R`, then confirm |
| T5 | the secret, Enter | 1 | 1 | 1 |
| T6 | `e`, the edit, Enter | 2 | 3 | about 3 (Esc, `MOD+E`, the edit, Enter) |
| T7 | Down, Enter from the problem | 2 | 2 | depends on `zbm.import_policy`; otherwise the recovery shell |
