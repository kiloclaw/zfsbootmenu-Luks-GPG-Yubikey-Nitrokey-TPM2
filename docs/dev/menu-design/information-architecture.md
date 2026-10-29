# Information architecture

The menu is one workspace ([ADR 0007](../adr/0007-menu-workspace.md)): four tabs instead of separate screens, and in
each, drill-down columns from the general to the particular, so a selection keeps its context. An inspector says what
the selection is and what booting it would do, a sidebar names what needs attention, the command line does anything
with a dry run first, and what cannot be undone opens a review page. Nothing is more than a tab and two steps away, and
`Esc` always goes back one step.

## The frame

```text
 ZFSBootMenu [1 Boot] 2 Snapshots  3 Pools  4 Log       tank DEGRADED · 3/3 volumes open · ! 1     tab bar
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.                                banner
┌ POOLS ──── 2 ┐┌ BOOT ENVIRONMENTS ─────────────── 4 ┐┌ zroot/ROOT/void ──────── default ┐
│  the sidebar ││          the columns                ││         the inspector             │
└──────────────┘└─────────────────────────────────────┘└───────────────────────────────────┘
 √ Done: zroot/ROOT/void is the default of zroot                                                  notification
 : commands   / filter   ? every key                                                              command hint
 Enter Boot   l Kernels   s Snapshots   e Edit   d Default                              ? More    toolbar
```

- **Tab bar** (always shown once start-up is over): the program's name, the four tabs with their digits, the current
  one in brackets so that it shows without colour, and the status of the machine on the right: the worst pool and its
  health, the LUKS volumes open, the warnings and errors of the log. Narrower, the name goes, then the status becomes
  the count of what needs attention (`! 4`), then the tabs take their short names (`2 Snap`), then their digits.
- **Banner**: the countdown while it runs; any key stops it.
- **Notification line**: the outcome of the last action, with a glyph and a word (`√ Done`, `! Warning`, `× Failed`);
  it stays until the next key press, and the log keeps it.
- **Command hint**: from 24 rows, how to reach the command line, the filter and every key.
- **Toolbar**: up to five actions of the focus, from the key map in order of importance, and `? More`.

The current map draws the same frame with a navigation bar instead of the tab bar: the back label (`‹` and the view
`Esc` returns to), the title of the view, and the same status ([apple-hig.md](apple-hig.md), rule 1).

## The tabs

| Tab | Columns | Inspector | Today's prototype |
| --- | --- | --- | --- |
| `1` Boot | pool › environment › kernels | will it boot, the checks, the kernel, the boot tries, the pool, the volumes it needs, the snapshots, the command line, the next steps | the environments and their kernels (`l`), the sidebar, the inspector |
| `2` Snapshots | the selection's snapshots as a timeline, newest first, grouped by day, week and month, older months folded, with the space each month holds (#23) | what changed since the snapshot (packages, files), and what to do with it | the snapshots view of the selected environment |
| `3` Pools | pools › their vdevs and devices; the LUKS volumes | what the state means, the next steps with their commands | the pools and volumes view |
| `4` Log | the log since start, and the boot history from the saved logs (#9) | the entry, and the saved report of a boot | the log view |

`Tab` and `Shift-Tab` go round the tabs; a digit goes to its tab. The Snapshots tab shows the snapshots of the
environment the Boot tab selected, or of the one whose kernels are shown.

## The Boot tab

From 110 columns, three panes side by side; the wireframe is `workspace-120x40`:

```text
┌ POOLS ────────────── 2 ┐┌ BOOT ENVIRONMENTS ──────────────────────────── 4 ┐┌ zroot/ROOT/void ────────────── default ┐
│ zroot        ONLINE ro ││ ▶ zroot/ROOT/void              default  6.12.9_1 ││       Checks  √ all 5 passed           │
│ tank       DEGRADED ro ││   tank/ROOT/arch       1 warning  6.11.5-arch1-1 ││   Boot Tries  3 of 3 left              │
└────────────────────────┘│   tank/ROOT/debian     1 problem  6.1.0-26-amd64 ││         Pool  zroot · ONLINE · read-o… │
┌ VOLUMES ─────── 3 of 3 ┐│   zroot/ROOT/ubuntu             6.8.0-47-generic ││       Kernel  6.12.9_1 · 3 kernels     │
│ luks-zroot-a      open ││                                                  ││ …                                      │
└────────────────────────┘│                                                  ││ NEXT STEPS                             │
┌ ATTENTION ────────── 4 ┐│                                                  ││   l kernels         s snapshots        │
│ ! tank DEGRADED        ││                                                  ││   e edit            d default          │
│   3 Pools              ││                                                  ││                                        │
```

- **The sidebar**: the pools with their health and mode, or why they are not imported; the volumes, open or not; and
  what needs attention: pools not imported or not `ONLINE`, volumes not open, environments whose checks found
  something, the log's problems, each with the tab that leads to it.
- **The columns**: the boot environments; `l` (or `Right`) goes into the kernels of the selection, `h` (or `Esc`) out.
  The columns of UI10 group the environments by pool and add the last boot and the checks as columns.
- **The inspector**: the facts that decide whether to boot, the next steps with their keys, then, when tall, the checks,
  the kernels and the newest snapshots. Long values wrap in UI10; the prototype cuts them with `…`.

At 80 columns, as over a serial line, the sidebar becomes one line of what needs attention above the list, and the
inspector goes below it (`workspace-80x24`, `workspace-80x24-ascii`):

```text
 [1 Boot] 2 Snapshots  3 Pools  4 Log            tank DEGRADED · 3/3 open · ! 1
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.
 ! tank DEGRADED   ! arch: 1 warning   × debian: 1 problem           3 Pools
┌ BOOT ENVIRONMENTS ──────────────────────────────────────────────────────── 4 ┐
│ ▶ zroot/ROOT/void                                          default  6.12.9_1 │
```

At 52×20 the list and the inspector alone, and the tab bar counts what needs attention.

## The command line and review pages

Planned for UI14 and UI10; the mockups show them.

- **`:` opens the command line** with fuzzy completion, grouped as actions, items and help, each with its key and what
  it costs, and a history (`Ctrl-R`). Before anything runs it shows the dry run: the steps of the action's plan, the
  command each runs (`zfs snapshot …`, `zfs rollback …`), the effect, the undo, and what the plan needs (a pool
  writable, a key). The prototype's `:` opens the command palette, which finds environments, snapshots and actions.
- **A review page** for a rollback, a forced import, a destroy or a rewind: what is lost, what is kept, the steps with
  their commands, the undo, a safer alternative (boot a clone, restore single files), and a confirmation by typing the
  target's name. It replaces the alerts below for these actions.
- **Start-up decides nothing silently**: a problem stops one step of the start-up timeline and offers choices with the
  commands they run (`1` to `9`), and a safe default after a time-out, which any key holds, such as continuing
  without a foreign pool that no boot environment needs.

## Views

The views the tabs show, which the current map opens as screens of their own:

| View | Opened by | Shows | Primary action (`Enter`) |
| --- | --- | --- | --- |
| Start-up | the boot | the stages of start-up with their state; unlock and import dialogs | answer the dialog |
| Main | start-up done | boot environments and a detail pane | boot the selection |
| Command palette | `:` anywhere | one filter over environments, snapshots, kernels and actions | run the match |
| Snapshots | `s` | the snapshots of the selected environment, newest first, with size and age | choose an action for the snapshot |
| Changes | `f` on a snapshot | what changed since the snapshot in two segments, files and packages, each with a preview of the selection | files: restore the marked files, or the selection; packages: show its files |
| Kernels | `k` | the kernels of the selected environment, default marked | boot that kernel |
| Command line | `e` | the command line of the selection, editable, with its origin | boot with it |
| Pools and volumes | `p` | each pool with health, mode and host ID; each LUKS volume with its state and methods | details of the selection |
| Recovery | `r` | shell, chroot, logs, report, firmware setup, reboot, power off | run the tool |
| Help | `?` | the key map of the view it was opened from, then every other | — |

Dialogs open over a view and return to it:

- **Unlock**: a volume, the method being tried (passphrase, TPM2 PIN, security key, key file passphrase), the time
  left for a device to appear, why this method is asked (the TPM kept the key, or the last try was wrong), the other
  methods, and the keyboard layout; the secret field echoes only after `Ctrl-R` ([unlocking](unlocking.md)).
- **Re-seal** (an alert): after the TPM kept a key, what changed and the risk of trusting it; the volume's name typed.
- **Import problem**: which pool, why it did not import (another host, missing devices, unsupported features), and the
  choices that apply (retry, wait for devices, import read-only, match the host ID, force, shell).
- **Confirm** (an alert): for destructive actions (rollback, destroy, promote, force import): what will happen, what will be lost
  (snapshots newer than the target, with their sizes), whether a safety snapshot will be taken, and a field that wants
  the short name of the target typed.
- **Choose** (an action sheet, with **Cancel** set apart): the actions for a snapshot (boot a clone, roll back, clone, duplicate, clone and promote, diff, chroot).
- **May not boot** (an action sheet): booting an environment whose checks found a failure says what failed and
  what to do, with **Boot Anyway** and **Cancel** ([boot health](boot-health.md)).
- **Error**: what failed, why (the cause chain of the error, in words), and the next steps offered.

## The main view of the current map

```text
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.
┌ BOOT ENVIRONMENTS ──────────────────────────────────────────────────────── 4 ┐
│ ▶ zroot/ROOT/void                                          default  6.12.9_1 │
│   tank/ROOT/arch                                   1 warning  6.11.5-arch1-1 │
│   tank/ROOT/debian                                 1 problem  6.1.0-26-amd64 │
│   zroot/ROOT/ubuntu                                         6.8.0-47-generic │
└──────────────────────────────────────────────────────────────────────────────┘
┌ zroot/ROOT/void ──────────────────────────────────────────────────── default ┐
│       Checks  √ all 5 passed                                                 │
│   Boot Tries  3 of 3 left                                                    │
│         Pool  zroot · ONLINE · read-only                                     │
│       Kernel  6.12.9_1 · 3 kernels                                           │
│    Snapshots  400 · newest pre-upgrade, 12 h ago                             │
│   Encryption  key loaded                                                     │
│ Command Line  quiet loglevel=4 · from zroot/ROOT                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

- The list: a grouped list, one value row per environment: the name, then `locked` for an encrypted environment
  whose key is not loaded, `default` for the default, and the kernel that would boot, right-aligned. Defaults first,
  then by name.
- Rows whose checks found something carry `1 problem` or `2 warnings`, and the environment asked for once carries
  `next boot` ([boot health](boot-health.md)).
- The banner says what the countdown will boot. After the default used up its boot tries, it says so in the warning
  role and names the fallback; for a one-time boot, it says the running system asked for it.
- The inspector: to the right from 100 columns (split view), below the list otherwise. Its title is the selection,
  with `default` on the right; right-aligned labels, then the facts that decide whether to boot: pool health and mode,
  kernel, snapshots, encryption, command line, after the checks, the boot tries and the next boot when they apply.
  When tall, the checks, the kernels and the newest snapshots follow.
- `/` opens the search field at the top of the list; the matched characters are highlighted, the header counts
  `2 of 4`, and `Esc` clears the search.

## Navigation

```text
start-up ─▶ 1 Boot ─┬─ l kernels                    ┌─ : command line (from anywhere)
                    ├─ e command line               │
                    └─ s ─┐                         ├─ ? help
            2 Snapshots ◀─┘─┬─ choose ─▶ review     │
                            └─ f changes ─▶ review  │
            3 Pools ─▶ unlock / import              │
            4 Log                                   └─ ! shell, x power
```

`Tab`, `Shift-Tab` and the digits go between tabs; `Esc` goes back one step, and from the Boot tab it clears the
filter. Any key stops the countdown, and only `Enter` (or `b`) boots. A view that opened a dialog gets the focus back
when it closes, with the selection it had.

## States every view has

Each view is designed, and snapshot-tested, in these states: loading (the first frame before data), empty (no
environments, no snapshots, no kernels), filtered without matches, an action in progress, an action failed, a pool
read-only, and the narrowest size.
