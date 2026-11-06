# What the menu needs from `zbm-app`

The menu talks to `zbm-app` only: it reads a `Snapshot` of the machine, sends `Action`s, and receives `Event`s. These
are the lists the design needs, handed to R4, which builds them in `zbm-app`. The prototype defined them and answered
them with a fake machine, so their shape was tried before R4 committed to it; they are now `zbm_app::model`, which the
prototype imports (`zbm_tui::menu::world` re-exports it), and `App::snapshot` reads the real machine into them (step 1
of R4 in ROADMAP.md says what it fills so far).

## The snapshot

What every view reads, without asking for it. Large or slow facts (snapshots, logs, diffs) are not in it: an action
loads them and an event delivers them.

| Field | Holds | Read from |
| --- | --- | --- |
| `startup` | the stages of start-up, each pending, running, done, failed with its reason, or skipped | `zbm-startup` |
| `timeout` | the countdown: seconds, or none (wait), or skip (boot at once) | `zbm-config` |
| `pools` | name, identifier, health, read-only or read/write, whether imported, the host (ID and name) that used it last when another system did, whether it has a checkpoint (unknown when not imported), its devices as a tree with their states (online, degraded, offline, removed, unavailable, faulted), error counts and notes, and what `zpool` says is wrong and what to do; the imported pools from `zpool list` and one `zpool status`, those that could be imported from the last scan (`ScanPools`) | `zbm-zfs` |
| `environments` | name, pool, default or not, encryption (none, locked, unlocked), kernels with the default marked, the command line with its origin, the number of its own snapshots and the newest (one `zfs list -t snapshot -d 1` each, nothing more kept), the checks of whether it will boot (name, verdict, what was found, the fix), its boot tries left of the total | `zbm-boot`, `zbm-kcl`, `zbm-zfs` |
| `fallback` | when the default used up its boot tries: which, after how many, and the last environment that marked a boot good, which boots instead; without one, nothing boots on its own | `zbm-boot`, an EFI variable |
| `next_boot` | the environment the running system asked to boot once | an EFI variable |
| `volumes` | LUKS volumes: name, device, state (locked, open, failed, skipped), the key methods that apply, the pool it holds, the PCRs its TPM2 key is sealed to and those that changed since, with what the event log says | `zbm-luks`, `zbm-crypt` |
| `keyboard` | the layouts typing can be translated into, the console's own first, and whether keys come from the machine's keyboard or a serial terminal | `zbm-config` |
| `log` | the warnings and errors since start, counted apart | `zbm-trace` |
| `offers` | the kinds of action the machine performs now (`Offer`): the menu shows and sends only these, and a key of another kind says so in the status line (ADR 0016, point 2); none when not given | `zbm-app` |
| `loaded` | every snapshot (name, creation, used) of the column open: the environment whose snapshots `LoadSnapshots` read last, read again with each snapshot of the machine; none until a column opens, or once its environment is gone | `zbm-zfs` |

## Actions

Each action names its target by value (`DatasetName`, `PoolName`, a volume name), never by a list index, so an action
stays right when the snapshot changes under it.

| Action | Parameters | Used by |
| --- | --- | --- |
| `Boot` | environment, kernel (or its default), command line (or its own) | main, kernels, command line, palette |
| `BootSnapshot` | snapshot, the new environment's name | snapshots, palette: the snapshot cloned under that name, as `Clone` does, then booted |
| `SetDefaultEnvironment` | environment | main |
| `SetDefaultKernel`, `InheritDefaultKernel` | environment, kernel | kernels |
| `LoadSnapshots` | environment | snapshots, as its column opens or its tab's key is pressed on it, and again when a snapshot of the machine came without them after a cancel, not after a failure: every snapshot of the environment, which `SnapshotsLoaded` brings; a read, quiet and free under the lock, never sent while the countdown runs or once the environment is gone; over SSH, refused for a dataset that is no boot environment of the machine |
| `CreateSnapshot` | environment, name | main, snapshots |
| `Rollback` | snapshot, whether to take a safety snapshot first | snapshots |
| `Clone`, `CloneAndPromote`, `Duplicate` | snapshot, new name | snapshots |
| `Diff` | snapshot, and a second snapshot or the live filesystem | snapshots |
| `RestoreFiles` | snapshot, the paths | changes, from a diff against the live filesystem |
| `ResetBootTries` | environment | palette |
| `Reseal` | volume | re-seal alert, palette |
| `Chroot` | environment or snapshot | main, snapshots |
| `Import` | pool, `ImportOptions`: read-only or read/write, the policy (this host, its last host, forced), whether to rewind to the checkpoint, and the pool's identifier when another pool has its name; an imported pool is exported first | main, pools, import problem |
| `Export` | pool | pools (`Enter` on an imported pool), palette: `zpool export` through the ZFS port, quietly; a busy pool fails with the cause `zpool` gives; the pool of the default environment too, after which the snapshot has no default; kept by the lock; the pools are scanned again after it, so the pool is listed to import |
| `ScanPools` | — | the menu, once its first frame is drawn, and the palette: the pools that could be imported, which `PoolsScanned` brings; a read (`Action::is_read`), after which the snapshot is not read again, free under the lock |
| `LoadHistory` | — | the workspace, once its main view is drawn and no countdown runs, and its Log tab when the history did not come: the boots `zbm.save_logs` kept, which `HistoryLoaded` brings; a read, quiet and free under the lock, never sent while the countdown runs; without `zbm.save_logs` the ESP is not read |
| `LoadDeviceLog` | a disk of a pool, as the pool names it | the workspace's pool dialog: the kernel's messages about it and what it stands on, which `DeviceLogLoaded` brings; a read, quiet and free under the lock; over SSH, refused for a device that is no disk of a pool the machine shows |
| `Open` | the volume's device, and the name it was opened as before, if it has one | pools: a keyholder opens it, and asks in the unlock dialog (ADR 0016, point 1) |
| `Unlock` | volume, method, the secret (a `Secret`, never a `String`) | unlock dialog: an answer, which goes to the question asked, not to the machine's queue |
| `SkipUnlock` | volume | unlock dialog: an answer, as `Unlock` |
| `AnswerImportProblem` | pool, the choice | import problem dialog |
| `LoadLog` | — | log |
| `Shell`, `FirmwareSetup`, `Reboot`, `PowerOff` | — | recovery, power |

## Events

| Event | Carries | Shown as |
| --- | --- | --- |
| `StageStarted`, `StageFinished` | the stage, its outcome | the start-up view |
| `UnlockRequested` | volume, method, how long to wait for a device, a notice: the last secret was wrong, or why this method instead of another | the unlock dialog (the `Prompter` port of `zbm-luks`); in the menu, it comes with the reply that takes the answer |
| `Withdrawn` | — | the unlock dialog or question closes unanswered: the device came, or the action that asked ended |
| `ResealOffered` | volume: it was opened without its TPM2 key, which the TPM kept | the re-seal alert |
| `UnlockFinished` | volume, outcome (opened, wrong secret with tries left, locked out, skipped) | the dialog, then a notification |
| `ImportProblem` | pool, the reason, the choices that apply | the import problem dialog (the `Operator` port of `zbm-pool`) |
| `SnapshotChanged` | the new `Snapshot` | every view redraws |
| `PoolsScanned` | every pool, imported and not, which replace the snapshot's | the pools view, the workspace; the countdown goes on, as it does not after `Changed` |
| `DeviceLogLoaded` | the device, the kernel's names of it and of what it stands on (`sda2`, `sda`; none for a device that is no block device the kernel lists or would name, as a link gone with its device), the newest 500 messages about them, untrusted | the workspace's Pools tab, below the pools, until Back; what the classic menu shows does not change |
| `SnapshotsLoaded` | environment, its snapshots (name, creation, used), newest first; a list longer than half a remote message (`MAX_LENGTH`), on the console too, ends as a failed load that says why | the snapshots view ("reading" until it comes, or until the machine is done with all it was sent; "not read" after a cancel or a failure), the rollback's newer snapshots, the name check of a new snapshot or clone, the palette, the inspector's newest snapshots |
| `HistoryLoaded` | the history: off (no `zbm.save_logs`), unavailable (why, and the error's code: no ESP, several, no such partition, not mountable), or the saved boots, newest first, each with its name, its files and their sizes, and its record (`boot.json`), none, or why it is not read; every name and text untrusted | the workspace's Log tab, above the log since start, and its inspector's "Last Boot", from the newest record of the environment; what the classic menu shows does not change |
| `SnapshotsUnread` | environment, whose snapshots a load could not read, before the `Finished` that says why | the snapshots view ("not read", asked again only when its column opens again) |
| `DiffReady` | the two ends, the changes (path, kind, new name of a rename, a preview of the difference), the packages that changed (name, kind, versions, their changed files) with the package manager, or none without a database | the changes view |
| `ActionStarted`, `ActionProgress`, `ActionFinished` | the action, progress as text and a fraction where known, the outcome (done, or failed with the error's cause chain and the next steps) | the notification line, the error dialog |
| `LogLoaded` | the log entries | the log view |
| `Leaving` | why the menu ends (booting, shell, reboot) | the last frame |

The front end owns what the scan finds (`zbm_app::Seen`) for as long as its menu runs, and lends it to its `App`;
the scan runs on the service's worker once the main view is drawn and no countdown runs (a key that stops the
countdown starts it), so neither the first frame nor the boot the countdown ends with waits for it, and `zdb -e -C`
runs only for a pool that another system used last. A scan that cannot read the imported pools changes nothing, and
is logged. Over SSH, `ScanPools` and `PoolsScanned` cross the session's socket as
any action and event; the protocol is at version 2 (`zbm_app::remote::VERSION`) since the pools carry their devices,
hosts and identifiers. What needs attention (the devices a pool lost, worst first, its checkpoint, the log's errors
and warnings apart) is drawn in the workspace only until UI10c; a device taken offline is shown in its pool, not
listed there.

The snapshots of an environment are read when its column opens (ADR 0016, point 6): `LoadSnapshots` records the
column in the front end's `Seen`, in place of the last one, and each snapshot of the machine read after it carries
its list again in `loaded`, so that what the menu shows of it, the rollback's newer snapshots among them, never comes
from an older read. At start, and in every snapshot, each environment has only its count of snapshots and its newest,
from one listing of its own snapshots (`zfs list -H -p -t snapshot -o name,creation,used -d 1 -s creation`), which
replaces the two listings of it and its descendants. The menu sorts a list once, as it comes or when the order
changes, ranks it again only when it, the focus or the filter changed, builds the rows of the lines shown alone, and
makes the palette's entries as it opens and when the machine sends snapshots, not on each key. The palette finds the
snapshots read, and a name is known to be taken only among those: a snapshot of a column never opened is refused by
`zfs`, whose message the error says. Each snapshot of the machine lists the column's environment once: its count and
newest come from its list. Over SSH, `LoadSnapshots`, `SnapshotsLoaded` and `SnapshotsUnread` cross as any action and
event, at version 2: no release has shipped version 2 yet, and both ends ship in one image, whose versions must be
equal.
The measures of `cargo run --release -p zbm-tui --example snapshots` (B5, in process) are in ROADMAP.md's
decision 7.

A device's messages are read from `/dev/kmsg` (`dmesg` where it cannot be read): those the kernel tagged with the
numbers of the device or of what it stands on (`DEVICE=b8:18`), and those whose text names one of them as a word, so
`sda` is not `sda1`. The device is resolved through `/sys/class/block`: a partition stands on its disk, a mapping on its
slaves, and a name that is empty, `.` or `..` or holds a `/` is refused. A pool not imported names its devices without
the directory `zpool import` found them in (`virtio-d0` for `/dev/disk/by-id/virtio-d0`), which are searched in its
order; a path that leads outside `/dev`, as a file's, leads to no device. A device the kernel no longer lists is known
only by a name the kernel gives disks and partitions, `sdb2` then its disk `sdb`, whose messages say why it went; any
other word, as a label's `error`, could be any message's, and names nothing. Messages that name only a port (`ata3.00:`,
`usb 1-2:`) are not among them, which the panel says. The text is the kernel's, which devices fill: it is drawn through
`Printable`, an escape byte as `\x1b`.

The boot history is read from the ESP only with `zbm.save_logs` (ADR 0016, point 7); without it the history is off, and
the Log tab names the option and `zfsbootmenu(7)`. The partition is the one the option names, found as a save finds it,
mounted read-only (`mount -t vfat -o ro,nosuid,nodev,noexec`) at a mount point of its own, `/run/zbm-saved-logs-read`,
so a read never meets a save or `zbm debug saved` on the same directory. The mount runs on the action's runner, which
Escape cancels; the unmount runs on the front end's runner, which nothing cancels, so a read cancelled once the
partition is mounted still unmounts it, as does a mount that failed but mounted. A read cancelled that failed, as when
Escape killed the mount or the search for the partition, sends no history, so the Log tab asks again. The kernel refuses
to mount a partition read-write while it is mounted read-only, so a read-only mount an earlier read left (the menu ended
during it, or its unmount failed) is unmounted before the next read, which fails when it cannot be, and before a save.
The read is bounded: the newest 16 boots (`READ_BOOTS`), the first 32 files of each by name (`FILES_READ`), the only
ones whose size is looked at, records of 64 KiB at most (`BootRecord::MAX_LEN`), checked by size before they are read;
at those bounds, every character escaped six times, the event stays under a remote message (`MAX_LENGTH`). Directories
are ordered newest first by their time, then by the number a name taken twice was given (`.10` after `.9`); `.tmp` files
are left out. A boot without `boot.json` is listed by its name, said to be saved without a record; a record too long, of
another schema, or not JSON is listed as unreadable with why, which never quotes the record. Over SSH, `LoadHistory` and
`HistoryLoaded` cross as any action and event, at version 2, and the session serves the read as the console does.

## What the prototype simplifies

`menu::world` follows these lists with a few shortcuts that R4 does not take:

- the snapshot holds the log entries, so there is no `LoadLog` or `LogLoaded`; the snapshots are loaded on demand,
  as the machine loads them (`LoadSnapshots`, ADR 0016 point 6), from lists the fake machine keeps apart;
- one `Changed` event stands for `SnapshotChanged` and the stage events, and `Finished` with an outcome for
  `ActionStarted`, `ActionProgress` and `ActionFinished`: the fake machine answers at once;
- the import problem is answered with `Import` and a policy rather than `AnswerImportProblem`, and `UnlockRequested`
  carries the tries left, which `UnlockFinished` will carry in R4;
- the checks are computed once by the fake machine, where R4 runs them again when a pool is imported or a kernel
  changes.

## The ports of R3

Start-up runs inside the menu: `zbm_platform::Interaction` and `zbm-pool`'s `Operator` turn into `UnlockRequested` and
`ImportProblem` events, and the menu answers with `Unlock`/`SkipUnlock` and `AnswerImportProblem`. Without a terminal
that can run the menu, the plain console prompts of today answer the same ports.

The menu asks the same way while a keyholder opens a volume (ADR 0016, point 1). The machine's service performs
`Open` on its worker, and the menu's LUKS session asks through `zbm_app::service::asker::Asker`, which shares start-up's
dialogs (`asker::Dialogs`, behind the `Asks` trait that start-up's front end implements too). A question reaches the
menu as `Report::Ask`, an event with a one-shot reply: the menu sends its answer (`Action::is_answer`: `Unlock`,
`SkipUnlock`, `Answer`) to that reply, never to the service's queue, which the action that asks holds. A question
the device answered, or whose action was cancelled or whose menu went, ends with `Report::Withdraw`, which the menu
sees as `Withdrawn`. A cancelled action asks nothing more, and an answer that comes once it is cancelled is none;
its keyholder is killed unless it said how it ended, and a volume it opened meanwhile is said to be open. While the
machine works, as between two questions, the menu does not send `Open` again. Over SSH, a question comes as
`ToMenu::Ask`, numbered, and the menu sends its answer to the session as `ToSession::Answer` with that number, its
secret as a packet of its own, and counts it as no action sent; the session sends it to the reply only when the number
is that of the question asked now, so that an answer to a question withdrawn meanwhile goes to nobody. The session
opens only a volume its volumes list, under the name it lists, and refuses `UnlockAll`, `OpenWithKeyFile`, `Close`
and an answer sent as an action. Its keyholder's request says that the person is remote (`"remote":true`), so that
the volume is recorded as opened remotely, which opens nothing the locked menu keeps. So is a volume that start-up,
run from the menu over SSH, opens: the session tells the programs of a remote menu's terminal that the person is over
SSH (`ZBM_REMOTE`), and a menu or `zluks` run in an SSH login knows it by `SSH_CONNECTION`.

Not done: the keyholder lends a key program no console from the menu, so a crypttab key script or `fido2-assert`
asking for its PIN fails there; resealing a TPM2 key (`Reseal`) and resetting the boot tries (`ResetBootTries`) are
not offered.
