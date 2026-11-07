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

- **Tab bar** (always shown, start-up being the Boot tab until it is over): the program's name, the four tabs with
  their digits, the current one in brackets so that it shows without colour, and the status of the machine on the
  right: the worst pool and its health, the LUKS volumes open, the count of what needs attention (`! 4`), and from 120
  columns the host ID and the time in UTC. From 120 columns, when they do not all fit, the name goes first, then the
  host ID. Below 120 columns neither the host ID nor the time shows, and the name has room again. Narrower still, the
  name goes, then the status becomes the count alone, then the tabs take their short names (`2 Snap`), then their
  digits.
- **Banner**: the countdown while it runs; any key stops it.
- **Notification line**: the outcome of the last action, with a glyph and a word (`√ Done`, `! Warning`, `× Failed`);
  it stays until the next key press, and the log keeps it.
- **Command hint**: from 24 rows, how to reach the command line, the filter and every key.
- **Toolbar**: up to five actions of the focus, from the key map in order of importance, and `? More`.

The classic map draws the same frame with a navigation bar instead of the tab bar: the back label (`‹` and the view
`Esc` returns to), the title of the view, and the same status ([apple-hig.md](apple-hig.md), rule 1).

## The tabs

| Tab | Columns | Inspector | Today's prototype |
| --- | --- | --- | --- |
| `1` Boot | pool › environment › kernels | will it boot, the checks, the kernel, the boot tries, the pool, the volumes it needs, the snapshots, the command line, the next steps | the environments and their kernels (`l`), the sidebar, the inspector |
| `2` Snapshots | environment › its snapshots as a timeline, newest first, grouped by day, week and month, older months folded, with the space each group holds; `z` folds or opens a group (#23) | the snapshot: when it was taken, the space it holds, its environment, the next steps; what changed since it (`f`) | the timeline beside the environments, the inspector |
| `3` Pools | pools › their vdevs and devices, and the LUKS volumes of each pool, as a tree | the pool's health, mode, last host and what `zpool` says is wrong and what to do; a vdev's kind; a device's path, note and error counts; a volume's state; the next steps | the tree, the inspector, a device's kernel messages |
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
┌ ATTENTION ────────── 6 ┐│                               ││                          ││               pre-upgrade, 1  │
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

## The Snapshots tab

The environments, beside the snapshots of the one the Boot tab selected, as a timeline: today, then each earlier day
of this week, each earlier week of this month (weeks start on Monday, as ISO 8601 says; the first week of a month
starts on its first day, as the days before it are in the month before), then each month, the newest first, in UTC as
every time of the menu. A snapshot taken after now, by a clock ahead, is today's. Each group has a header that is
neither numbered nor a stop for the arrows, with how many snapshots it holds and the space they hold; `o` sorts the
snapshots within each group. The months are folded: a folded group is its header alone, which is then its one stop.
`z` folds the group of the selection, or opens it, and `Enter` on a folded group opens it too; there the keys of a
snapshot's actions are not shown, and say to open the group, but `f` diffs the snapshots marked. A group stays folded
or open until midnight, when the tab bar's clock moves the groups, even before the machine says its time again; a
refresh keeps the snapshot selected by name. While a filter ranks the snapshots by how well each name matches, they
show without groups. From 120 columns the inspector is beside them (`workspace-snapshots-120x40`, at `End`):

```text
 zroot › zroot/ROOT/void
┌ ENVIRONMENTS ───── 4 ┐┌ SNAPSHOTS BY NAME ────────────────────────────────── 400 ┐┌ September 2025 ──────────────────┐
│   zroot · ONLINE     ││   Week of 12 October               7 snapshots · 301 MiB ││    Snapshots  12 snapshots       │
│ ▶ zroot/ROOT/void    ││     autosnap_2026-10-12_daily        11 d ago     56 MiB ││         Used  555 MiB            │
│   zroot/ROOT/ubuntu  ││     autosnap_2026-10-13_daily        10 d ago     43 MiB ││                                  │
│   tank · DEGRADED    ││     autosnap_2026-10-14_daily         9 d ago     30 MiB ││ NEXT STEPS                       │
```

Narrower the inspector is below them, and under 80 columns the snapshots stand alone. The line view prints the
headers as lines without a number, and numbers the snapshots, and the folded groups, which are stops.

## The Pools tab

Each pool, then its vdevs and their devices as `zpool status` prints them: those of the data first, then those of each
other class (`dedup`, `special`, `logs`, `cache`, `spares`) under a heading of its class, which is no stop; a spare in
use shows twice, below the vdev it stands in for and among the spares. Then the LUKS volumes of the pool: a volume is
listed under the imported pool of its name, or else under the first pool of its name; the volumes of no pool listed
come last, under a header that is neither numbered nor a stop. Each row is indented by its depth, and says its health
on the right: a pool its mode and, when another host used it last, that host; a device what `zpool` says after its
state. A long note or host name is cut before the name or the health is. A device is named by the last part of its
path, or of the path it had, as ZFS names a device it lost by its GUID. `Enter` on a pool, or on a vdev, opens the
pool's details; on a device nothing is below, it shows the kernel's messages about it below the tree, until Back, and
the toolbar says so; on a volume, it opens it. The other keys of a vdev or a device act on its pool, and a volume's
keys show on a volume alone. The path above reads pool › class › vdev › device, and a rescan keeps the row selected by
name and class, or the pool of a device that is gone. From 120 columns the inspector is beside the tree
(`workspace-pools-120x40`):

```text
 tank
┌ POOLS AND LUKS VOLUMES ────────────────────────────────────────────────── 13 ┐┌ tank ────────────────────────────────┐
│   zroot                                                   read-only · ONLINE ││       Health  DEGRADED, read-only    │
│     mirror-0                                                          ONLINE ││       Status  One or more devices    │
│       nvme0n1p2                                                       ONLINE ││               could not be opened.   │
│       nvme1n1p2                                                       ONLINE ││               Sufficient replicas    │
│     cache                                                                    ││               exist for the pool to  │
│       nvme2n1                                                         ONLINE ││               continue functioning   │
│     luks-zroot-a                                               Volume · open ││               in a degraded state.   │
│     luks-zroot-b                                               Volume · open ││       Action  Attach the missing     │
│ ▶ tank                                                  read-only · DEGRADED ││               device and online it   │
│     raidz1-0                                                        DEGRADED ││               using 'zpool online'.  │
│       sda2                                                            ONLINE ││  Data Errors  0                      │
│       sdb2                                             cannot open · UNAVAIL ││                                      │
│       sdc2                                        too many errors · DEGRADED ││ NEXT STEPS                           │
│     luks-tank                                                  Volume · open ││   i import         w read/write      │
```

Narrower the inspector is below the tree, and a device's messages take its place. The line view prints the tree as
indented lines, numbers its stops as the arrows go, and says a device's messages after it, unnumbered.

## The command line and review pages

The review pages are built under the workspace map (UI10d); the command line comes with UI14.

- **`:` opens the command line** with fuzzy completion, grouped as actions, items and help, each with its key and what
  it costs, and a history (`Ctrl-R`). Before anything runs it shows the dry run: the steps of the action's plan, the
  command each runs (`zfs snapshot …`, `zfs rollback …`), the effect, the undo, and what the plan needs (a pool
  writable, a key). The prototype's `:` opens the command palette, which finds environments, snapshots and actions.
- **A review page** for a rollback, a destroy of snapshots, a forced import or a rewind: the whole view in the danger
  colour, which opens and asks the machine for the change's plan (`Plan`), and says so until it comes. Then, from the
  top: the title; what stops the change (a clone or a hold of a snapshot it destroys, a host that has the pool
  imported now), first, in place of the field: the page refuses it, and `Enter` closes it; what is lost, which scrolls
  (`PgUp`, `PgDn`, the arrows), its sizes right-aligned under their total; the space destroying them frees and what is
  kept (a destroy keeps every bookmark); the steps, their commands muted; the undo, or that there is none; the safer
  action (a clone of the snapshot, named first, for a rollback; the read-only import for a rewind; none for a forced
  import or a destroy); who else may use the pool (its last host's ID and name and when it was last written, or this
  host); and at the bottom the field that wants the target's name (the last component of the environment rolled back,
  as the classic map asks; the short name of the one snapshot destroyed, or the last component of their environment
  for several; the pool's name) and the buttons. `Tab` and `Shift-Tab` go from the field to the safer action and the
  change, the button `Enter` presses shown as such; the change is sent only with the plan, nothing that stops it, and
  the name typed as it is; `Esc` closes the page and sends nothing, and cancels the plan if the machine reads nothing
  else meanwhile. A change of the machine while it is open plans it again. The line view says the page as lines, the
  first 20 rows of what is lost and how many more, takes the name as a field does, and cannot scroll or take the safer
  action, as its help line says; `Announce` says what the page asks when it opens and when its plan comes. They
  replace the alerts below for these actions under the workspace; the classic map keeps the alerts. No safety snapshot
  is taken before a rollback.
- **Start-up decides nothing silently**: a problem stops one step of the start-up timeline and offers choices with the
  commands they run (`1` to `9`), and a safe default after a time-out, which any key holds, such as continuing
  without a foreign pool that no boot environment needs.

### Start-up as the Boot tab

Under the workspace map start-up is the Boot tab until it is over (ADR 0016, point 9): the tab bar, the stages where
the columns go, beside the sidebar from 110 columns or under what needs attention narrower, and a stage that failed
among what needs attention. Between its questions the start-up view reads keys: `Tab` and `Shift-Tab` choose a tab,
the arrows, `Ctrl-P` and `Ctrl-N` move, `?` and `F1` open help, and no other key is bound, so none says that what it
asks for is not offered. No character but `?` is bound there, not the digits nor `j` `k`: a passphrase may begin with
any of them. So the tab bar and what needs attention name no digit there (`[Boot] Snapshots  Pools  Log`,
`workspace-startup-80x24`). The other tabs show what start-up knows, and their digits choose a tab and show in the
tab bar; the Log tab shows the stages so far, and says that the
log since start shows once start-up is over. The Snapshots tab has no boot environment yet: `Tab` and `Shift-Tab`
pass over it, and `2` on another tab says that its snapshots show once start-up is over. Nothing is read from the
machine before then: the menu reads it.

On the Boot tab, a key it does not bind starts typing ahead, as a passphrase typed before its prompt: that key and
every key after it wait for the next dialog, which reads them first, as the classic map's dialogs read what was typed
ahead. If start-up ends with no dialog, they go nowhere: the menu never reads them, so they stop no countdown.

The countdown after start-up runs as before, unless the person left the Boot tab during start-up, outside a dialog:
then someone is there, looking at something, and the menu opens on their tab without a countdown, the Boot tab's
main view behind it. Keys typed into a dialog or ahead of one, such as a passphrase or a PIN, and noise on a serial
line never count, so the unattended boot is never held otherwise. A passphrase typed ahead is kept only from its first
key the Boot tab does not bind: one that begins with a digit, `j` or `k` waits whole for its dialog, but one that
begins with `?` opens help, and the keys after it are the help's and then those of the tab a digit chooses, which hold
no keys typed ahead: that passphrase is lost and typed again into its dialog. A secret typed during start-up is never shown, whatever the tab.

Not built yet: the frame between the end of start-up and the menu's first frame, which the setup hooks and the
machine's first read leave blank; start-up over SSH, which the session's socket yields, so the SSH menu never shows
the stages; the line view, whose stages stay on the plain console; and `--resume`, which opens the menu at once. The
classic map's start-up view is unchanged, and reads no key between its questions.

## Views

The views the tabs show, which the classic map opens as screens of their own:

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
- **Confirm** (an alert): for destructive actions of the classic map (rollback, force import, rewind), and of both maps
  for a promotion, a re-seal and a restore of files: what will happen, what will be lost (snapshots newer than the
  target, with their sizes), and a field that wants the short name of the target typed. Only the restore of files
  takes a safety snapshot first, and says so; a rollback takes none. Under the workspace a rollback, a rewind and a
  forced import open their review page instead.
- **Choose** (an action sheet, with **Cancel** set apart): the actions for a snapshot (boot a clone, roll back, clone, duplicate, clone and promote, diff, chroot, and under the workspace, last, destroy it).
- **May not boot** (an action sheet): booting an environment whose checks found a failure says what failed and
  what to do, with **Boot Anyway** and **Cancel** ([boot health](boot-health.md)).
- **Error**: what failed, why (the cause chain of the error, in words), and the next steps offered.

## The main view of the classic map

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
