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
  health, the LUKS volumes open, the count of what needs attention (`! 4`), and from 120 columns the host ID and the
  time in UTC. From 120 columns, when they do not all fit, the name goes first, then the host ID. Below 120 columns
  neither the host ID nor the time shows, and the name has room again. Narrower still, the name goes, then the
  status becomes the count alone, then the tabs take their short names (`2 Snap`), then their digits.
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

From 110 columns, the sidebar beside the path and two columns; from 120 columns the inspector shows beside them too,
and narrower below them, so that the environments keep their room. The wireframe is `workspace-120x40`:

```text
┌ POOLS ────────────── 2 ┐ zroot › zroot/ROOT/void
│ zroot        ONLINE ro │┌ BOOT ENVIRONMENTS ───────── 4 ┐┌ KERNELS ────────────── 3 ┐┌ zroot/ROOT/void ───── default ┐
│ tank       DEGRADED ro ││   zroot · ONLINE              ││   6.12.9_1               ││       Checks  √ all 5 passed  │
└────────────────────────┘│ ▶ zroot/ROOT/void     default ││   6.12.8_1               ││   Boot Tries  3 of 3 left     │
┌ VOLUMES ─────── 3 of 3 ┐│   zroot/ROOT/ubuntu           ││   6.8.0_1                ││         Pool  zroot · ONLINE  │
│ luks-zroot-a      open ││   tank · DEGRADED             ││                          ││               · read-only     │
│ luks-zroot-b      open ││   tank/ROOT/arch    1 warning ││                          ││       Kernel  6.12.9_1 · 3    │
│ luks-tank         open ││   tank/ROOT/debian  1 problem ││                          ││               kernels         │
└────────────────────────┘│                               ││                          ││    Snapshots  400 · newest    │
┌ ATTENTION ────────── 6 ┐│                               ││                          ││               pre-upgrade, 12 │
│ ! tank DEGRADED        ││                               ││                          ││               h ago           │
```

- **The sidebar**: the pools with their health and mode, or why they are not imported; the volumes, open or not; and
  what needs attention: pools not imported or not `ONLINE`, volumes not open, environments whose checks found
  something, the log's problems, each with the tab that leads to it.
- **The columns**: the boot environments, grouped by pool under a header that is not an item, the default's pool first
  and the default first in its pool; `l` (or `Right`) goes into the kernels of the selection, `h` (or `Left`, `Esc`)
  out, and each column keeps its selection. The path above them names what they select:
  `zroot › zroot/ROOT/void › 6.12.9_1`. Beside the sidebar, and from 80 columns, the kernels show beside the
  environments; at 52 columns the deepest column shows alone.
- **The inspector**: of the deepest column, an environment or a kernel: the facts that decide whether to boot, the next
  steps with their keys, then, when tall, the checks, the kernels and the newest snapshots. Long values wrap under their
  label (`workspace-inspector-wrap`); the classic menu cuts them with `…`.

At 80 columns, as over a serial line, the sidebar becomes one line of what needs attention above the path, and the
inspector goes below the two columns (`workspace-80x24`, `workspace-80x24-ascii`):

```text
 [1 Boot] 2 Snapshots  3 Pools  4 Log            tank DEGRADED · 3/3 open · ! 6
 Booting zroot/ROOT/void in 10 s. Press any key to choose another.
 ! tank DEGRADED   ! tank: sdb2 UNAVAIL   ! tank: checkpoint         3 Pools
 zroot › zroot/ROOT/void
┌ BOOT ENVIRONMENTS ──────────────────────────── 4 ┐┌ KERNELS ────────────── 3 ┐
│   zroot · ONLINE                                 ││   6.12.9_1               │
│ ▶ zroot/ROOT/void                        default ││   6.12.8_1               │
```

At 52×20 the deepest column and the inspector alone, and the tab bar counts what needs attention.

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
