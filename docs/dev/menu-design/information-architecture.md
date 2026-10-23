# Information architecture

One main view, and every other view opened from it and returning to it. Nothing is more than two steps from the main
view, and `Esc` always goes back one step.

## The frame

Every view is drawn in the same frame ([apple-hig.md](apple-hig.md), rules 1, 2, 9 and 10):

```text
 ‹ Boot Environments               Snapshots     tank DEGRADED · 3/3 open · ! 1    navigation bar
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.                   banner (countdown only)
┌ SNAPSHOTS BY CREATION ────────────────────────────────────────────────── 400 ┐
│                              the view                                        │
└──────────────────────────────────────────────────────────────────────────────┘
 √ Done: zroot/ROOT/void is the default of zroot                                     notification (0 or 1 line)
 Enter Choose   r Roll Back   c Clone   f Diff   y Duplicate              ? More     toolbar
```

- **Navigation bar** (always shown): the back label (`‹` and the view `Esc` returns to), the title of the view in the
  middle, and the status of the machine: the worst pool and its health, the LUKS volumes open, and the warnings and
  errors of the log. On narrow screens the back label shortens to `‹` and the status to its short forms, never wraps.
- **Banner**: the countdown while it runs, in the tint colour; any key stops it, and its line goes back to the view.
- **Notification line**: the outcome of the last action, with a glyph and a word (`√ Done`, `! Warning`,
  `× Failed`); it stays until the next key press, and the log keeps it.
- **Toolbar**: up to five actions of the focused element, from the key map in order of importance, and `? More`,
  which opens help.

## Views

| View | Opened by | Shows | Primary action (`Enter`) |
| --- | --- | --- | --- |
| Start-up | the boot | the stages of start-up with their state; unlock and import dialogs | answer the dialog |
| Main | start-up done | boot environments and a detail pane | boot the selection |
| Command palette | `:` anywhere | one filter over environments, snapshots, kernels and actions | run the match |
| Snapshots | `s` | the snapshots of the selected environment, newest first, with size and age | choose an action for the snapshot |
| Kernels | `k` | the kernels of the selected environment, default marked | boot that kernel |
| Command line | `e` | the command line of the selection, editable, with its origin | boot with it |
| Pools and volumes | `p` | each pool with health, mode and host ID; each LUKS volume with its state and methods | details of the selection |
| Recovery | `r` | shell, chroot, logs, report, firmware setup, reboot, power off | run the tool |
| Help | `?` | the key map of the view it was opened from, then every other | — |

Dialogs open over a view and return to it:

- **Unlock**: a volume, the method being tried (passphrase, TPM2 PIN, security key, key file passphrase), the time
  left for a device to appear, and the other methods; the secret field never echoes.
- **Import problem**: which pool, why it did not import (another host, missing devices, unsupported features), and the
  choices that apply (retry, wait for devices, import read-only, match the host ID, force, shell).
- **Confirm** (an alert): for destructive actions (rollback, destroy, promote, force import): what will happen, what will be lost
  (snapshots newer than the target, with their sizes), whether a safety snapshot will be taken, and a field that wants
  the short name of the target typed.
- **Choose** (an action sheet, with **Cancel** set apart): the actions for a snapshot (boot a clone, roll back, clone, duplicate, clone and promote, diff, chroot).
- **Error**: what failed, why (the cause chain of the error, in words), and the next steps offered.

## The main view

```text
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.
┌ BOOT ENVIRONMENTS ──────────────────────────────────────────────────────── 4 ┐
│ ▶ zroot/ROOT/void                                          default  6.12.9_1 │
│   tank/ROOT/arch                                              6.11.5-arch1-1 │
│   tank/ROOT/debian                                            6.1.0-26-amd64 │
│   zroot/ROOT/ubuntu                                         6.8.0-47-generic │
└──────────────────────────────────────────────────────────────────────────────┘
┌ zroot/ROOT/void ──────────────────────────────────────────────────── default ┐
│         Pool  zroot · ONLINE · read-only                                     │
│       Kernel  6.12.9_1 · 3 kernels                                           │
│    Snapshots  400 · newest pre-upgrade, 12 h ago                             │
│   Encryption  key loaded                                                     │
│ Command Line  quiet loglevel=4 · from zroot/ROOT                             │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

- The list: a grouped list, one value row per environment: the name, then `locked` for an encrypted environment
  whose key is not loaded, `default` for the default, and the kernel that would boot, right-aligned. Defaults first,
  then by name.
- The inspector: to the right from 100 columns (split view), below the list otherwise. Its title is the selection,
  with `default` on the right; right-aligned labels, then the facts that decide whether to boot: pool health and mode,
  kernel, snapshots, encryption, command line. When tall, the kernels and the newest snapshots follow.
- `/` opens the search field at the top of the list; the matched characters are highlighted, the header counts
  `2 of 4`, and `Esc` clears the search.

## Navigation

```text
            ┌──────────── : palette (from anywhere) ────────────┐
start-up ─▶ main ─┬─ s snapshots ─▶ choose ─▶ confirm
                  ├─ k kernels
                  ├─ e command line
                  ├─ p pools and volumes ─▶ unlock / import
                  ├─ r recovery
                  └─ ? help
```

`Esc` goes back one step; from the main view it clears the filter. Any key stops the countdown, and only `Enter` boots.
A view that opened a dialog gets the focus back when the dialog closes, with the selection it had.

## States every view has

Each view is designed, and snapshot-tested, in these states: loading (the first frame before data), empty (no
environments, no snapshots, no kernels), filtered without matches, an action in progress, an action failed, a pool
read-only, and the narrowest size.
