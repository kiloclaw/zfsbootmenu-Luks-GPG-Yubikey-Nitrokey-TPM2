# What the menu needs from `zbm-app`

The menu talks to `zbm-app` only: it reads a `Snapshot` of the machine, sends `Action`s, and receives `Event`s. These
are the lists the design needs, handed to R4, which builds them in `zbm-app`. The prototype defines them in
`zbm_tui::menu::world` and answers them with a fake machine, so their shape is tried before R4 commits to it; R4 moves
them into `zbm-app` and the prototype then imports them from there.

## The snapshot

What every view reads, without asking for it. Large or slow facts (snapshots, logs, diffs) are not in it: an action
loads them and an event delivers them.

| Field | Holds | Read from |
| --- | --- | --- |
| `startup` | the stages of start-up, each pending, running, done, failed with its reason, or skipped | `zbm-startup` |
| `timeout` | the countdown: seconds, or none (wait), or skip (boot at once) | `zbm-config` |
| `pools` | name, health, read-only or read/write, host ID, whether imported, whether it has a checkpoint, problems | `zbm-pool`, `zbm-zfs` |
| `environments` | name, pool, default or not, encryption (none, locked, unlocked), kernels with the default marked, the command line with its origin, the number of snapshots and the newest | `zbm-boot`, `zbm-kcl`, `zbm-zfs` |
| `volumes` | LUKS volumes: name, device, state (locked, open, failed, skipped), the key methods that apply, the pool it holds | `zbm-luks` |
| `log` | the number of warnings and errors since start | `zbm-trace` |

## Actions

Each action names its target by value (`DatasetName`, `PoolName`, a volume name), never by a list index, so an action
stays right when the snapshot changes under it.

| Action | Parameters | Used by |
| --- | --- | --- |
| `Boot` | environment, kernel (or its default), command line (or its own) | main, kernels, command line, palette |
| `BootSnapshot` | snapshot | snapshots: a temporary clone, booted |
| `SetDefaultEnvironment` | environment | main |
| `SetDefaultKernel`, `InheritDefaultKernel` | environment, kernel | kernels |
| `LoadSnapshots` | environment, sort order | snapshots |
| `CreateSnapshot` | environment, name | main, snapshots |
| `Rollback` | snapshot, whether to take a safety snapshot first | snapshots |
| `Clone`, `CloneAndPromote`, `Duplicate` | snapshot, new name | snapshots |
| `Diff` | snapshot, and a second snapshot or the live filesystem | snapshots |
| `Chroot` | environment or snapshot | main, snapshots |
| `ImportReadWrite`, `RewindCheckpoint` | pool | main, pools |
| `Import` | pool, policy (host ID, force), read-only or not | pools, import problem |
| `Unlock` | volume, method, the secret (a `Secret`, never a `String`) | unlock dialog, pools |
| `SkipUnlock` | volume | unlock dialog |
| `AnswerImportProblem` | pool, the choice | import problem dialog |
| `LoadLog` | — | log |
| `Shell`, `FirmwareSetup`, `Reboot`, `PowerOff` | — | recovery, power |

## Events

| Event | Carries | Shown as |
| --- | --- | --- |
| `StageStarted`, `StageFinished` | the stage, its outcome | the start-up view |
| `UnlockRequested` | volume, method, how long to wait for a device | the unlock dialog (the `Prompter` port of `zbm-luks`) |
| `UnlockFinished` | volume, outcome (opened, wrong secret with tries left, locked out, skipped) | the dialog, then a notification |
| `ImportProblem` | pool, the reason, the choices that apply | the import problem dialog (the `Operator` port of `zbm-pool`) |
| `SnapshotChanged` | the new `Snapshot` | every view redraws |
| `SnapshotsLoaded` | environment, the snapshots (name, creation, used, referenced) | the snapshots view |
| `DiffReady` | the changes (path, kind) | the diff view |
| `ActionStarted`, `ActionProgress`, `ActionFinished` | the action, progress as text and a fraction where known, the outcome (done, or failed with the error's cause chain and the next steps) | the notification line, the error dialog |
| `LogLoaded` | the log entries | the log view |
| `Leaving` | why the menu ends (booting, shell, reboot) | the last frame |

## The ports of R3

Start-up runs inside the menu: `zbm-luks`'s `Prompter` and `zbm-pool`'s `Operator` turn into `UnlockRequested` and
`ImportProblem` events, and the menu answers with `Unlock`/`SkipUnlock` and `AnswerImportProblem`. Without a terminal
that can run the menu, the plain console prompts of today answer the same ports.
