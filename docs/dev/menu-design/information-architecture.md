# Information architecture

One main view, and every other view opened from it and returning to it. Nothing is more than two steps from the main
view, and `Esc` always goes back one step.

## The frame

Every view is drawn in the same frame:

```text
┌ ZFSBootMenu ─────────────────────────────────────── status bar ┐   1 line
│ Booting zroot/ROOT/void in 7 s · any key stops                 │
│ pools 2 ok · volumes 3 open · log 1 warning                    │
├────────────────────────────────────────────────────────────────┤
│                                                                │
│                     the view                                   │
│                                                                │
├────────────────────────────────────────────────────────────────┤
│ notification: Snapshot zroot/ROOT/void@safety created          │   0 or 1 line
│ [Enter] boot  [k] kernels  [s] snapshots  [e] edit  [?] help   │   footer
└────────────────────────────────────────────────────────────────┘
```

- **Status bar** (always shown): the countdown while it runs, else the name of the view; the pools (how many, and
  the worst health with its name: `pools 2 · tank DEGRADED`), the LUKS volumes (open, locked, failed), and the log (the
  number of warnings and errors). On 52 columns the parts shorten (`2 pools · 1 DEGRADED`), never wrap.
- **Notification line**: the outcome of the last action, with its level as a word (`done`, `warning`, `failed`); it
  stays until the next key press, and the log keeps it.
- **Footer**: the keys of the focused element, from the key map, in order of importance; as many whole bindings as fit.

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
- **Confirm**: for destructive actions (rollback, destroy, promote, force import): what will happen, what will be lost
  (snapshots newer than the target, with their sizes), whether a safety snapshot will be taken, and a field that wants
  the short name of the target typed.
- **Choose**: the actions for a snapshot (boot a clone, roll back, clone, duplicate, clone and promote, diff, chroot).
- **Error**: what failed, why (the cause chain of the error, in words), and the next steps offered.

## The main view

```text
 Boot environments          2/3 │ zroot/ROOT/void              default
 > * zroot/ROOT/void   6.8.9   │ pool      zroot ONLINE, read-only
     tank/ROOT/arch    6.9.1   │ kernel    6.8.9 (3 kernels)
     zroot/ROOT/ubuntu 6.8.0 🔒│ snapshots 12, newest @2026-10-22 2 h ago
                                │ encryption zroot · key loaded
                                │ command line  quiet loglevel=4
```

- The list: one line per environment, the default marked with `*` and the word `default` in the detail pane, the
  kernel that would boot, and `locked` for an encrypted environment whose key is not loaded (an icon only where the
  terminal can draw it, the word always). Grouped by pool when there is more than one pool, defaults first.
- The detail pane: to the right from 100 columns, below the list from 80, and three lines at 52 (pool health,
  kernel, snapshots). It shows the facts that decide whether to boot: pool health and mode, kernel, snapshots,
  encryption, command line.
- `/` filters the list with `nucleo-matcher`; the matched characters are highlighted; `Esc` clears the filter.

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
