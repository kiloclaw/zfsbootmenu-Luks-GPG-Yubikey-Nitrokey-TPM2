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
| `pools` | name, health, read-only or read/write, host ID, whether imported, whether it has a checkpoint, problems | `zbm-pool`, `zbm-zfs` |
| `environments` | name, pool, default or not, encryption (none, locked, unlocked), kernels with the default marked, the command line with its origin, the number of snapshots and the newest, the checks of whether it will boot (name, verdict, what was found, the fix), its boot tries left of the total | `zbm-boot`, `zbm-kcl`, `zbm-zfs` |
| `fallback` | when the default used up its boot tries: which, after how many, and the last environment that marked a boot good, which boots instead; without one, nothing boots on its own | `zbm-boot`, an EFI variable |
| `next_boot` | the environment the running system asked to boot once | an EFI variable |
| `volumes` | LUKS volumes: name, device, state (locked, open, failed, skipped), the key methods that apply, the pool it holds, the PCRs its TPM2 key is sealed to and those that changed since, with what the event log says | `zbm-luks`, `zbm-crypt` |
| `keyboard` | the layouts typing can be translated into, the console's own first, and whether keys come from the machine's keyboard or a serial terminal | `zbm-config` |
| `log` | the number of warnings and errors since start | `zbm-trace` |
| `offers` | the kinds of action the machine performs now (`Offer`): the menu shows and sends only these, and a key of another kind says so in the status line (ADR 0016, point 2); none when not given | `zbm-app` |

## Actions

Each action names its target by value (`DatasetName`, `PoolName`, a volume name), never by a list index, so an action
stays right when the snapshot changes under it.

| Action | Parameters | Used by |
| --- | --- | --- |
| `Boot` | environment, kernel (or its default), command line (or its own) | main, kernels, command line, palette |
| `BootSnapshot` | snapshot, the new environment's name | snapshots, palette: the snapshot cloned under that name, as `Clone` does, then booted |
| `SetDefaultEnvironment` | environment | main |
| `SetDefaultKernel`, `InheritDefaultKernel` | environment, kernel | kernels |
| `LoadSnapshots` | environment, sort order | snapshots |
| `CreateSnapshot` | environment, name | main, snapshots |
| `Rollback` | snapshot, whether to take a safety snapshot first | snapshots |
| `Clone`, `CloneAndPromote`, `Duplicate` | snapshot, new name | snapshots |
| `Diff` | snapshot, and a second snapshot or the live filesystem | snapshots |
| `RestoreFiles` | snapshot, the paths | changes, from a diff against the live filesystem |
| `ResetBootTries` | environment | palette |
| `Reseal` | volume | re-seal alert, palette |
| `Chroot` | environment or snapshot | main, snapshots |
| `Import` | pool, `ImportOptions`: read-only or read/write, the policy (this host, its last host, forced), whether to rewind to the checkpoint; an imported pool is exported first | main, pools, import problem |
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
| `SnapshotsLoaded` | environment, the snapshots (name, creation, used, referenced) | the snapshots view |
| `DiffReady` | the two ends, the changes (path, kind, new name of a rename, a preview of the difference), the packages that changed (name, kind, versions, their changed files) with the package manager, or none without a database | the changes view |
| `ActionStarted`, `ActionProgress`, `ActionFinished` | the action, progress as text and a fraction where known, the outcome (done, or failed with the error's cause chain and the next steps) | the notification line, the error dialog |
| `LogLoaded` | the log entries | the log view |
| `Leaving` | why the menu ends (booting, shell, reboot) | the last frame |

## What the prototype simplifies

`menu::world` follows these lists with a few shortcuts that R4 does not take:

- the snapshot holds every snapshot of every environment and the log entries, so there is no `LoadSnapshots`,
  `LoadLog`, `SnapshotsLoaded` or `LogLoaded`; the performance test of R5 (10,000 snapshots) is where loading on
  demand earns its place;
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
