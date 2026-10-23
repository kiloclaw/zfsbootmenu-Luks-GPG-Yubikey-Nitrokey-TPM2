# Boot health

Four features that answer the questions people ask the menu after something went wrong: *will this environment
boot?*, *what happens when it doesn't?*, *can the running system choose the next boot?* and *can I get one file back
without rolling back?*. The prototype draws them on its fake machine; the checks, the counting and the restore are
built in R4 (`zbm-app`) and R5. Nothing here is in the images yet.

```sh
cargo run -p zbm-tui -- --demo               # checks, and file restore from a snapshot
cargo run -p zbm-tui -- --demo --failing     # the default used up its boot tries
cargo run -p zbm-tui -- --demo --next-boot   # the running system asked for tank/ROOT/arch once
```

## Will it boot?

Before the menu shows an environment, `zbm-app` runs a few cheap checks on it, from what start-up already read. Each
check has a name, a verdict (pass, warning, failure), what it found, and, when it did not pass, what to do:

| Check | Fails when | Warns when |
| --- | --- | --- |
| Kernel and initramfs | the kernel that boots has no initramfs, or one of them cannot be read | — |
| ZFS module | the initramfs has no `zfs.ko` for that kernel | — |
| Pool features | — | the pool has an active feature the environment's ZFS cannot read (it would import read-only, or not at all) |
| Root dataset | `mountpoint` is not `/`, or `canmount=off` | — |
| Free space | — | the pool has less than 5% free |

The main view shows the result where the decision is made:

- a row whose checks found something carries `1 problem` (failure, in the danger role) or `2 warnings` (warning
  role), next to `default` and the kernel; a row that passed carries nothing, so the list stays quiet;
- the inspector's first fact is `Checks`: `√ all 5 passed`, or the worst finding; when the pane is tall, a `CHECKS`
  section lists each check with its glyph;
- booting an environment with a failure, by `Enter`, `b`, from its kernels or from the command line editor, opens an
  action sheet *“tank/ROOT/debian May Not Boot”* that says what failed and what to do, with **Boot Anyway** and
  **Cancel** (wireframe `may-not-boot`). Warnings never stop a boot.

The countdown never asks: nobody may be in front of the machine. It boots the default as before, and the checks are
in the log.

## Boot counting and the fallback

An environment can be given a number of boot tries, `zbm.boot_tries=3` on its command line (or the
`org.zfsbootmenu:boot_tries` property). Each boot of it from the menu takes one, kept in an EFI variable (or, without
EFI, a property of the environment written before the kexec). The booted system gives them back once it is up, with
`zbm bless` run by a unit or service late in its start-up. An environment without the option is not counted.

When the default has none left, it failed to start that many times in a row, and the menu does not boot it again:

- the banner, in the warning role, says so and what boots instead: *“zroot/ROOT/void failed to start 3 times.
  Booting a clone of pre-upgrade in 10 s.”* (wireframe `main-fallback`), with shorter forms on narrow screens;
- the fallback is the last environment that blessed a boot, or else a clone of the default's newest snapshot taken
  by ZFSBootMenu (`@pre-upgrade` in the demo), booted as `BootSnapshot` does;
- with the countdown off, the banner only says what failed, and nothing boots on its own;
- the inspector shows `Boot Tries  0 of 3 left`, and the command palette offers *“Give zroot/ROOT/void Its Boot Tries
  Back”* (`ResetBootTries`) once the cause is fixed.

## Boot once

The running system can ask for the next boot only, as `systemctl reboot --boot-loader-entry` does for systemd-boot:
`zbm next-boot tank/ROOT/arch` writes an EFI variable that the menu reads and clears at start-up. The menu then
selects that environment, labels its row `next boot`, and the banner says *“Booting tank/ROOT/arch once, as the
running system asked, in 10 s.”* (wireframe `main-next-boot`). Any key stops it as usual; the default is unchanged.

## Restore files from a snapshot

`f` on a snapshot (`Diff`) opens the **Changes** view instead of a notification: what changed since the snapshot,
one row per path, with its kind (`M` modified, `+` created, `-` removed, `R` renamed, with `→` and the new name), and a
preview of the selected change, to the right from 100 columns and below otherwise (wireframes `diff` and
`diff-marked`).

- `Space` or `Tab` marks paths; `Enter` restores the marked ones, or the selection when none is marked. A created
  path has nothing to restore and is left out, with a notification when it is the only one.
- The confirmation alert names the files, says they replace the files as they are now, and that the pool is imported
  read/write and a safety snapshot `@before-restore` is taken first; it wants the environment's short name typed
  (wireframe `restore-confirm`).
- `RestoreFiles` copies each path back from `.zfs/snapshot/<name>/`, keeping owner, mode and times; the view is then
  refreshed, and the restored paths no longer show.

`Esc` goes back to the snapshots.

## What `zbm-app` gains

In [interfaces.md](interfaces.md): `checks` and `tries` on each environment, `fallback` and `next_boot` in the
snapshot, the actions `RestoreFiles` and `ResetBootTries`, and the event `DiffReady` with the changes and their
previews. The commands `zbm bless` and `zbm next-boot` and the `zbm.boot_tries` option are proposals for R5; their
names are open until then.
