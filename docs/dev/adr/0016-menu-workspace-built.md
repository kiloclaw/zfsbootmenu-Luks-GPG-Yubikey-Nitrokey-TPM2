# ADR 0016: The rest of the menu, built as the workspace

- **Status:** accepted, 3 November 2026 (proposed the same day)
- **Decides:** item UI10 of [ROADMAP.md](../../../ROADMAP.md): what the menu does on a real machine, how the workspace
  of [ADR 0007](0007-menu-workspace.md) reaches it, and the parts that record left open; it changes ADR 0007's
  decision 1 (the tab bar) and fills its decision 6 (review pages)
- **Governs:** `zbm-tui`'s menu, `zbm-app`'s `model` (the `Snapshot`, `Action`, `Event` and `Plan` vocabulary) and its
  use cases, the menu's wiring in `zbm` (`menu.rs`, `remote.rs`), `zbm.menu`, `fuzz/fuzz_targets/menu_names.rs`,
  `xtask gen-docs`, `docs/man/zfsbootmenu.7.rst`, `docs/dev/menu-design/` and `CHANGES-FROM-LEGACY.md`

## Context

ADR 0007 decided the workspace: four tabs, drill-down columns, an inspector, what needs attention in a sidebar, review
pages for what cannot be undone, and keys that hold on every keyboard. UI13 built it as a prototype,
`zbm-tui --demo --workspace`, for the usability sessions (UI11), which have not run. Decision 8, the key map itself,
stays open until they do.

On a real machine the menu is still the classic one:

- **The map.** `Model::new` sets `Map::Classic`, and only the demo calls `set_map`. `zbm.menu` chooses between the
  screen and the line view (UI8), not between maps.
- **What the machine does.** `App::apply` performs the boot, the snapshot changes (new default, snapshot, rollback,
  clone, clone and promote, duplicate), the diff, the shell and the power actions. The others the menu can send end
  in "not available in this program yet" (`application::unavailable`): booting a snapshot, setting and inheriting the
  default kernel, restoring files, the chroot, the read/write import, the rewind to a checkpoint, an import with any
  policy, and, from the unlock dialog, unlocking, skipping and resealing a volume, and resetting boot tries. The
  menu's `App` has no LUKS session (`App::new(..)` without `with_luks`, `zbm/src/menu.rs` and `remote.rs`), so no
  volume shows and the unlock dialog cannot open; with no boot tries read, their reset is never offered either. The
  keyholder unlocks every volume (`UnlockAll`) or one (`Open`), and asks for secrets through the front end, which
  answers with `Unlock`; it has no request for one volume with a secret typed beforehand.
- **What the machine shows.** `Snapshot` has only imported pools, no devices or vdevs, no foreign host IDs; its checks
  and boot tries are empty (features #6 and #7 are prototypes); the log is the journal's warnings and errors; the
  snapshots of every environment are read when the menu starts, two listings each.
- **What is undefined.** Destroy has no action, key or vocabulary entry, though ADR 0007 gives it a review page; the
  menu keeps two marks, the operands of a diff. The boot history (#9) is one cell of a table: the saved logs
  (`zbm_boot::saved_logs`) hold the journal, the crash reports and the kernel log, and for a measured boot the TPM
  event log, which names the kernel and the command line; for other boots nothing says what was booted, as the
  journal names the environment at the debug level only. Nothing writes a saved boot after a failed `kexec`. The review page's
  parts beyond the plan's steps (what is lost, the undo, a safer alternative) have no data. The rollback's
  confirmation in `information-architecture.md`, `Rollback` in `interfaces.md`, `apple-hig.md` and the tasks of
  `users-and-tasks.md` speak of a safety snapshot, which the menu does not take (`CHANGES-FROM-LEGACY.md`, "Rollback
  has no safety snapshot"). `Import(pool, ImportPolicy)` cannot say read-only or
  read/write, or a rewind. The tab bar's contents differ between ADR 0007 (host, ZFS version, what is degraded, the
  time) and the wireframes (the worst pool, the open volumes, what needs attention).
- **The exit.** No VM test drives a snapshot change, a diff, the palette, a kernel, an import or an unlock through
  the menu; the hostile-names fuzz target draws seven classic screens; the man page has no key section; the
  10,000-snapshot test and the benchmark that measures the budget B5 do not exist, and ROADMAP decision 7 (Nucleo or
  its matcher) is open.

## Decision

**The menu becomes the workspace on every machine, with everything it offers performed by the machine; what the
machine cannot do is not offered; the classic menu stays one release behind `zbm.menu=classic`.**

### What the machine does

1. **Every action the menu offers has a use case in `zbm-app`,** or is not offered:
   - **kernels:** `SetDefaultKernel` and `InheritDefaultKernel` write or inherit `org.zfsbootmenu:kernel`, as a
     change (the pool read/write first, as the other changes);
   - **snapshots:** `BootSnapshot(SnapshotName, DatasetName)` clones the snapshot to a new environment of that name,
     as `Clone` does, and boots it; the name is asked for as `Clone`'s is, and `interfaces.md`'s temporary clone goes;
     `RestoreFiles` copies the chosen files from the snapshot, mounted read-only, into the environment, as a change;
   - **the chroot:** `Chroot` runs `zfs-chroot` on the console for the selection through ADR 0010's `ShellGate`,
     read-only for a snapshot, until SB6 makes it an applet; for an environment on a read-only pool, `zfs-chroot`
     still asks itself whether to import it read/write;
   - **pools:** one `Import` (decision 3) for every way to import, and the export;
   - **LUKS volumes:** the menu's `App` has a LUKS session, on the console and over SSH. Unlocking a volume sends
     `Open` for its device to the keyholder (SB10), which asks for the secret through the menu as it asks start-up:
     the unlock dialog opens on the keyholder's request (`UnlockRequested`) and answers it with `Unlock` or
     `SkipUnlock`, so the menu holds a secret only while it is typed, and the keyholder protocol gains nothing.
2. **The model says what the machine can do.** `Snapshot` gains `offers`, the kinds of action this machine performs
   now (none of the volumes' without a LUKS session, no reseal without a TPM2 token, no boot tries before #7). The
   menu shows only what is offered: in the toolbar, the `Enter` menus, the palette and the help, and a key bound to
   something not offered says why in the status line rather than opening a dialog. `unavailable` stays as `App`'s
   answer to an action it was not offered; a test checks that the menu never sends one.
3. **One import.** `Action::Import(PoolName, ImportOptions)`, where `ImportOptions` says read-only or read/write, the
   `ImportPolicy` (as this host, as the host that owns it, forced), and whether to rewind to the checkpoint. It
   replaces `Import(pool, policy)`, `ImportReadWrite` and `RewindCheckpoint`; the import dialog's choices are its
   values, and each that cannot be undone (forced, the rewind) opens its review page. The locked menu keeps the
   read/write, forced and rewinding imports, as ADR 0010 says (`is_kept_by_lock`).
4. **A diff of two snapshots needs no writable pool.** It mounts the filesystem read-only and loads its key if
   needed, but no longer imports the pool read/write. A diff against the filesystem still does, as `zfs diff` takes a
   temporary snapshot of it; that one stays a change.

### What the machine shows

5. **Pools as they are.** `Snapshot` lists the pools that can be imported as well as the imported ones, with the host
   that owns each and whether it is foreign, their vdevs and devices with their states and errors, and the checkpoint.
   `Enter` on a device shows its kernel messages.
6. **Snapshots on demand.** At start each environment has its count of snapshots and its newest, which the
   inspector shows; the snapshots themselves are read when its column opens (`LoadSnapshots`, `SnapshotsLoaded`), and
   read again after a change, with the environments, for the columns open. The palette finds the snapshots read. The
   10,000-snapshot test measures it.
7. **The boot history (#9).** Each saved boot gets a record of what was booted, `boot.json` beside its logs: the
   time, the environment, the kernel and initramfs, the command line and whether it was measured, written with the
   logs before the teardown hooks. A `kexec` that fails after it shows in the journal of the next saved boot, not in
   the record. The Log tab lists the saved boots, newest first, with their records and their files; the inspector of an environment says when it last booted, and with which kernel. Whether a boot
   reached a login needs the booted system to say so, which is #7's `zbm bless`; until then the history says
   "loaded", not "booted". Without `zbm.save_logs` there is no history, and the Log tab says how to keep one.
8. **What needs attention, from what exists.** The sidebar lists degraded pools, missing devices, foreign pools,
   checkpoints, volumes left locked and the log's errors. The checks of #6 and the boot tries of #7 join it with their
   features; UI10 does not build them.

### The workspace

9. **The workspace is the menu.** The real menu draws the workspace of ADR 0007 at every size: the four tabs, with
   start-up as the Boot tab until it ends (ADR 0007, answer 5); the columns pool › environment › kernels or snapshots
   with their path above them; the inspector, whose long values wrap; the Snapshots tab as the timeline grouped by day,
   week and month; the Pools tab as pools › vdevs › devices, and the volumes. The key map is
   [keymap.md](../menu-design/keymap.md)'s workspace map, which the sessions (UI11) may still change: decision 8 stays
   open, and a change to it is a change of `keymap/` and the strings that name keys.
10. **The classic menu stays one release.** `zbm.menu=classic` draws the classic screens, with no new view, with the
    classic map; the line view still shows by itself where it does now. It goes with RL2, as the legacy runtime does.
    The line view (UI8) follows the workspace: it prints the tab's column, and the tabs are words (`boot`,
    `snapshots`, `pools`, `log`), since a number chooses an item there.
11. **The tab bar** is the wireframes': the tabs, the worst pool, the open volumes and what needs attention; the host
    and the time join it from 120 columns. This changes ADR 0007's decision 1, which also named the ZFS version: it
    moves to the help view's header.

### Review pages

12. **A review page is a view of the plan.** `Action::Plan` returns a `Plan` with, besides its steps and their
    commands, what the action loses (snapshots and bookmarks with their sizes, the bytes written since), what it keeps,
    its undo if it has one, and a safer action if there is one, all computed by `zbm-app` from the machine. The page
    shows them, then asks for the target's name. UI14's dry run is the same view without the confirmation.
13. **Rollback.** `zfs rollback -r` destroys the newer snapshots and bookmarks, and nothing taken first could undo it,
    so the page lists them with their sizes, says it has no undo, and offers cloning the snapshot as the safer action.
    When a newer snapshot has a clone or a hold, `zfs rollback -r` refuses, and the page says so and names it instead
    of asking. The design pages lose the safety snapshot.
14. **Destroy.** `Action::Destroy(Vec<SnapshotName>)` destroys snapshots: the selection, or the marked ones, which is
    also how #15 reclaims space. Marks become a set; `f` diffs when one or two are marked and says so otherwise. The
    page lists each snapshot with its size, the space they free together (`zfs destroy -n -v -p`), and what stops it: a
    clone or a hold, either of which makes the page refuse and name it. Bookmarks of a destroyed snapshot stay. It is
    an entry of the snapshot's `Enter` menu and of the palette (rule 1), and `Delete` where the terminal sends it; no
    letter, while decision 8 is open. Destroying an environment is not in UI10.
15. **The forced import and the rewind** have pages too: who else may be using the pool (its host ID and host name,
    when it was last written), and, for the rewind, what is lost since the checkpoint.

### The exit

16. **Checks.** VM tests drive each core task through the menu, on the display console and on a serial console: boot
    with a kernel and an edited command line, each snapshot change, a diff and a restore, destroy, an import of each
    kind, unlocking a volume, the palette's entries; the hostile-names fuzz target draws every view and dialog of the
    workspace and the classic menu with hostile environment, pool, snapshot, path and device names; wireframes of every
    view at 120×40, 80×24 in colour, monochrome and ASCII, and 52×20 in the pseudo-language; the 10,000-snapshot test
    against B5, which also settles ROADMAP decision 7.
17. **Docs.** `xtask gen-docs` writes the key section of `zfsbootmenu.7` from `keymap.rs`, through an example of
    `zbm-tui` (xtask depends on no workspace crate): the workspace map, and the classic aliases in a table of their
    own. `docs/online/`, the help pages of the legacy menu, stay for the legacy runtime until RL2; the Rust menu's help
    is its help view. `CHANGES-FROM-LEGACY.md` becomes the migration guide: a table from the `fzf` keys to the new
    ones, then what each tab does differently.
18. **Out of UI10:** the package changes of a diff (#21 needs a reader for each distribution's package database), the
    checks of #6 and the boot tries of #7, the comparison of kernels (#22), the space by month (#23 beyond the
    timeline's headings), destroying an environment, and a persistent command-line edit (writing
    `org.zfsbootmenu:commandline`), which becomes a command of UI14; `e` stays "edit once".

### Steps

19. UI10a, what the machine does (points 1 to 4); UI10b, what it shows (5 to 8); UI10c, the workspace (9 to 11);
    UI10d, the review pages and destroy (12 to 15); UI10e, the exit (16 and 17), the docs and the VM handoff. UI10a and
    UI10b change no view but the unlock dialog's routing and the hidden actions, so they do not wait for the sessions.

## Consequences

- No key leads to "not available": what the menu offers, the machine does, and the unlock dialog no longer asks for a
  secret it cannot use.
- The workspace is what users see from UI10c on, before the sessions have run. If they reject it, `zbm.menu=classic`
  is there, and the risk the roadmap names (phase 3 building the current design) shrinks to the key map, which is one
  table.
- Two menus to keep for a release: the classic screens stay drawn and tested until RL2, but get no new views.
- The menu imports, rewinds, destroys and unlocks: the review pages and the locked menu (SE9, which keeps every change
  ADR 0010 names behind the PIN, and destroy, which this record adds to them) are what stand between a key and a lost pool. `Plan` stays free
  under the lock, so a locked menu shows what an action would do.
- Saved boots carry a small record of what they booted, written to the ESP with the logs, only with `zbm.save_logs`.
- The model's vocabulary changes (`Import`, `Destroy`, `Plan`, `offers`, the pools' devices, `LoadSnapshots`): the
  wire of the menu over SSH changes with it, and both ends ship in one image.

## Alternatives considered

- **Wait for the sessions before the workspace.** Safer for the layout, but UI11 needs participants and the
  maintainer's time, and phase 3's other work (UI14, UI15) builds on the workspace. The machine's half (UI10a, UI10b)
  does not wait either way.
- **Keep both menus for good.** Every view twice, and the classic map cannot follow ADR 0007's rules (its capitals, its
  `MOD+key` actions).
- **Show what the machine cannot do, greyed.** It tells users what exists, but every unavailable entry is noise on a
  boot menu, and a greyed action at 80×24 in monochrome looks like any other.
- **A safety snapshot before a rollback.** It cannot undo `zfs rollback -r`, which destroys the snapshots newer than
  the target, including any safety snapshot; a clone first is what keeps the current state, and it is the page's
  safer action.
- **Destroy on a letter.** `x` is power and `d` the default; the free letters (`a`, `g`, `p`, `q`, `t`, `v`) mean
  nothing for it, and the map is open. The `Enter` menu and the palette are rule 1's way, and `Delete` a shortcut.
- **The boot history from the journal alone.** The journal names the boot environment only at the debug level, and a
  saved boot's journal can be cut short; a record written at the save is one read.
- **Package changes now.** A preview from `xbps`, `apk`, `dpkg`, `pacman` and `rpm` databases is a feature of its own
  (#21), with its own parsers to fuzz.

## How it is checked

- `zbm-app`'s tests on `FakeSystem` and `FakeZfs` for each new use case, each import option and destroy, with their
  plans; a test that every action the menu can send is either performed or not offered, so `unavailable` is reached
  only from the wire.
- The view's tests and wireframes for the workspace's tabs, columns, timeline, review pages, the boot history and the
  attention sidebar at the three sizes, in colour, monochrome and ASCII, and in the pseudo-language at 52×20.
- The fuzz target over every view and dialog, and the 10,000-snapshot test against B5.
- The VM tests of point 16, in the lanes of VE5.
- `xtask gen-docs --check` for the man page's key section.

## Where the work stands

Points 1 to 4 built from 4 to 6 November 2026 (UI10a-1 to UI10a-5), as decided but for what follows. On 5 November the
maintainer decided four details the record left open: restoring files first takes a safety snapshot, the countdown no
longer falls back to a snapshot, a passphrase typed over SSH opens nothing the lock keeps, and the export on its own
comes with UI10b.

- **Point 1:**
  - **kernels:** as decided. Setting the default writes `org.zfsbootmenu:kernel` as `/<file>$`, with the regular
    expression's metacharacters escaped, so that it matches only that kernel; inheriting asks for no key.
  - **boot a snapshot:** as decided. The name is prefilled `<name>_NEW`, as in the legacy menu; the clone is booted
    only once it is made, and a failed boot leaves it, to boot or destroy. The countdown boots only environments
    (`Target::Snapshot` is gone): a default that used up its tries with no good environment to fall back to never
    boots on its own again, and the banner says what failed.
  - **restore files:** offered only from a diff against the filesystem. It first takes the safety snapshot
    `<environment>@before-restore-<UTC time>`, unique to each restore, copies each path beside itself and renames it
    into place, and deletes nothing.
  - **the chroot:** as decided. It starts from `programs::interactive_shell(runner, ShellRoot::Chroot)`, so the
    shell-start metric counts it; it is offered only when the image has `/bin/zfs-chroot`, and its nonzero exit is a
    failure.
  - **pools:** the import as point 3. The export is not offered on its own yet: it comes with UI10b.
  - **LUKS volumes:** as decided, but the keyholder protocol gains one field. `UnlockRequest` says whether the person
    who answers is over SSH, and a volume opened so is recorded as `<name>\tremote` in `luks-unlocked`, neither
    attended nor unattended, so it opens neither the locked menu nor the recovery shell. The session says so for the
    programs of a menu it serves over SSH (`ZBM_REMOTE`), and `zluks` and the menu that runs itself over SSH see
    `SSH_CONNECTION`. The keyholders of the menu are quiet; their questions reach the menu through
    `zbm_app::service::asker::Asker`, which start-up's dialogs share (`Dialogs`), and the answers go to the question
    waiting, never through the service's queue. Over SSH each question has a number, and an answer reaches only the
    question asked now. Cancelling the action, or another instance taking over, ends the wait and kills the
    keyholder; a volume that opened as its action was cancelled says it is open. Not built: the reseal, the boot
    tries (#7), and lending the console to a key program, so that a crypttab key script or `fido2-assert` that asks
    for a PIN on the terminal fails from the menu.
- **Point 2:** as decided. `OpenVolume` is offered only with a LUKS session and `cryptsetup`.
- **Point 3:** as decided. Every import runs on the console. The lock keeps every import but a read-only one as this
  host: the read/write, forced and rewinding ones, and a read-only one as its last host, which adopts that host's ID.
  Under `zbm.import_policy=force`, which comes from the image, the menu's change between read-only and read/write
  forces the import, as the legacy `set_ro_pool` did.
- **Point 4:** as decided. A read-only pool takes the console only for an action that writes it.
- **Checks:** `zbm-app`'s tests on `FakeSystem` and `FakeZfs` for each use case, import option and plan; the test that
  every kind offered is performed and no other; the menu's tests that it sends only what is offered; the asker's and
  the session's tests of a question cancelled, withdrawn, answered late or answered over SSH, and that an SSH unlock
  leaves the lock shut. The VM run of UI10a, on rebuilt images, is the maintainer's.

Points 5 to 8 built on 6 November 2026 (UI10b-1 to UI10b-6), as decided but for what follows. On 6 November the
maintainer decided four details: until UI10c the device messages, the boot history and the new attention items are
shown in the workspace only, the classic menu gaining only the export and the pool rows it already draws; exporting
the pool of the default environment is allowed; the boot history and a device's messages need no PIN under the lock,
and the export is kept by it; and without `zbm.save_logs` the menu does not read the ESP.

- **Point 5:**
  - **pools:** as decided. `zbm-zfs` reads `zpool status`, `zpool import` and `zdb -e -C <id>`, in `LC_ALL=C`, into
    typed vdevs; each `Pool` has its id, devices with their states and errors, its host and its checkpoint. `zdb`
    runs only for a foreign pool. The scan of importable pools runs after the first frame, and only once no
    countdown runs, so a slow or hung `zpool import` never delays an unattended boot; it answers `PoolsScanned`,
    never `Changed`. It runs again after an import, an export, an unlock and a chroot. Pools with the same name are
    told apart by their id: the import, and the re-import of a pool made read/write, go by it.
  - **the export,** left over from point 1: `Export` in the pool dialog and the palette, through the port's
    export. Afterwards the pool is listed to import, and a default on it is dropped.
  - **a device's messages:** not on `Enter`, which waits for UI10c's device column, but as a choice of the pool
    dialog in the workspace, shown in a panel under the pools. They are the kernel's lines tagged with the device's
    numbers or naming it, a partition's disk and a mapping's slaves included, the newest 500. A lost device known
    only by a `/dev/disk/by-id/` link it no longer has cannot be traced to a kernel name, and the panel says so.
- **Point 6:** as decided. One column is open at a time, the one read last. A list too long to send (about 70,000
  snapshots) fails on every menu, not only over SSH, so the two menus agree. At 10,000 snapshots every key takes
  less than a frame once the rows are built only for the visible lines; the numbers are in decision 7.
- **Point 7:** as decided, with no `origin` field. `boot.json` is schema 1 and is written last, through a `.tmp` and a
  rename, as are the logs written again before `kexec`, so a directory with a record holds its boot's logs whole.
  `measured` says that the measured copies were the files loaded: the TPM may hold the events of a boot whose
  copies could not be written, and its event log says so. The history mounts the ESP read-only at its own mount
  point and lists the newest 16 boots; a record that is missing or does not parse leaves its boot listed. The Log
  tab and the inspector's "Last Boot" show it in the workspace.
- **Point 8:** as decided, in the workspace: degraded and foreign pools, pools not imported, devices unavailable,
  removed or faulted, worst first, checkpoints, volumes left locked, and the log's errors and warnings counted apart.
  An offline device shows in its pool, not in the sidebar. The status line's worst pool puts an imported one first.
- **Checks:** `zbm-zfs`'s parsers on recorded and written output, fuzzed; `zbm-app`'s tests on `FakeSystem` and
  `FakeZfs` for the scan, the export, the loads and the history, and their `calls()`; the countdown's tests that it
  neither waits for nor sends a read; the wireframes `workspace-attention`, `workspace-device-log`, `log-history` and
  `log-no-history` at every size; the 10,000-snapshot test and `examples/snapshots.rs`. The VM run of UI10b, on
  rebuilt images, is the maintainer's; `runtime-rust-saved` checks `boot.json`, and the recorded fixtures of
  `zpool status -j`, `zpool import` and `zdb` wait for `xtask record-zfs`.

Points 9 to 11 built on 7 November 2026 (UI10c-1 to UI10c-7), as decided but for what follows. On 7 November the
maintainer decided the details the record left open: the default flips last, in UI10c-7, so no half-built tab ships;
a person who left the Boot tab during start-up, outside a dialog, gets no countdown, while keys typed in a dialog and
noise on a serial line never count; the host in the tab bar is the host ID; the Log tab during start-up shows the
stages; the timeline is newest first with `o` sorting within a group; the ZFS version comes from
`/sys/module/zfs/version`; the time shows in the tab bar even with `zbm.console_status=on`; `zbm.menu` takes one
value; and the classic wireframes keep their names.

- **Point 9:** the workspace is the menu (`Map`'s default), on the console, over SSH, in the start-up view and in the
  line view.
  - **the Boot tab:** the boot environments grouped by pool under headers that are neither numbered nor stops, in the
    defaults-first order within each pool, the kernels column beside them (`l` or `Right`, back with `h`, `Left` or
    `Escape`), the path row above them, and the inspector of the deepest column, whose long values wrap under their
    label. From 120 columns the sidebar, both columns and the inspector stand side by side; at 80 the inspector goes
    below; at 52×20 one column stands alone, the deepest, and the path row costs one of the 20 rows. "At every size"
    holds in that sense only.
  - **the Snapshots tab:** the timeline of the open environment, today, the days of this week, the weeks of this
    month and the months, newest first, each group with its count and space; older months start folded, `z` folds,
    and `o` sorts within each group. Only the visible rows are built.
  - **the Pools tab:** the tree of the pools, their vdevs and devices, the special devices under their headings, and
    the volumes under their pool or no pool; `Enter` on a device shows its kernel messages, and the pool dialog's
    choice of a device is gone.
  - **start-up as the Boot tab:** the tab bar above the stages, which stand where the columns go, a failed stage among
    what needs attention, and `Tab`, `Shift-Tab`, the arrows and help bound between questions, with no character but
    `?`. It holds only where the start-up view runs: not in the line view, not over SSH, not after `--resume`, not when
    the view fails to open, and not in the gap of the setup hooks between the view's end and the menu's first frame.
    The keys are the 7 November decision (UI10d-1), a narrowing of the accepted "only `Tab` and `Shift-Tab`": UI10c-6
    bound the digits and `j` `k` there too, so a passphrase typed before its prompt lost its first key when it was one
    of them, and a digit left the Boot tab and held the countdown. Now the digits and `j` `k` are unbound on start-up's
    view of the Boot tab (`Context::Startup`) and wait with the rest of the passphrase for its dialog; `Tab`,
    `Shift-Tab`, the arrows and `?` stay, and the digits still choose a tab on the other tabs during start-up, which
    hold no keys typed ahead. The tab bar and what needs attention name the keys of the view they are on, so start-up's
    view of the Boot tab shows no digit (`[Boot] Snapshots  Pools  Log`; the 12 `workspace-startup-*` and
    `workspace-import-problem-*` wireframes changed). A passphrase that begins with `?` still opens help, whose keys
    then read the rest of it, as a digit chooses a tab: it is lost, and typed again into its dialog.
  - **the keys:** the map spans `keymap/` (`workspace.rs`, `classic.rs`, `labels.rs`, `offers.rs`), and keys are also
    named by strings: the line view's words and hints, and the hint of an empty Boot tab, whose key comes from the
    map. A change of decision 8 is a change of `keymap/` and those strings. `!` still opens the recovery tools, the
    shell first.
- **Point 10:** as decided, but for "the current screens", which the point said before: UI10b had already added the
  export and the pool rows to the classic menu's screens, so what holds is that the classic menu gains no new view,
  as the point now says. `zbm.menu` takes `lines`, `screen` or `classic`; `classic` gives the classic map and leaves
  the choice of the lines to `TERM` and `speakup`, so the classic menu shows as lines where it did. The line view of
  the workspace prints the tab's column, its path in the title, and takes the tabs' names as words. The demo draws
  the workspace, `--classic` the classic menu; `--workspace` is accepted and changes nothing. The classic wireframes
  are drawn with the classic map and did not change.
- **Point 11:** as decided, with the host as the host ID: the tab bar shows the tabs, the worst pool, the open
  volumes and what needs attention, and from 120 columns the host ID and the time. The ZFS version, read from
  `/sys/module/zfs/version` through the files port, is in the help view's header.
- **Checks:** the wireframes of each tab at every size, at 80×24 in colour and ASCII, and the classic ones unchanged;
  the tests that pinned the classic map now name it; the offers test for each map alone; the `menu_names` fuzz target
  over both maps; the VM scenarios now expect the workspace (`runtime-rust-drm` waits for `BOOT ENVIRONMENTS`,
  `runtime-rust-console`, `-console-serial`, `-console-start` and `-console-comfort` check the tab bar), and
  `runtime-rust-console-lines` boots `zbm.menu=classic` with `TERM=dumb` too. The VM run of UI10c, on rebuilt
  images, is the maintainer's.

Points 12 to 15 built on 7 November 2026 (UI10d-1 to UI10d-6), as decided but for what follows. On 7 November the
maintainer decided the details the record left open: the digits and `j` `k` are unbound on start-up's view of the
Boot tab alone, and `?` stays bound there (point 9's record); destroy is the workspace's alone, and the classic menu
gains nothing; marks are a set in both maps; "who else" may use a pool rewound is this host when it has the pool
imported, and otherwise the host the pool's configuration records; only snapshots of boot environments are
destroyed; the wire stays at `VERSION` 2, its new fields defaulted; the name typed is the snapshot's short name for
the destroy of one, the environment's last component for several and for a rollback, as the classic menu asks, and
the pool's name for a rewind or a forced import; a hold stops a rollback as a clone does; over SSH the plans of what
the menu does not show are refused; the forced import offers nothing safer; and what `zfs`, `zpool` and `zdb` print
that no recording shows yet is checked in the maintainer's VM run and by `xtask record-zfs`.

- **Point 12:**
  - **the machine:** `Action::Plan` answers `Event::Planned` with the steps and their commands, how the change would
    end, and its review (`Plan.review`), which `zbm-app` reads from the machine after running the change against the
    dry run: what it loses (snapshots and bookmarks with their sizes, what was written since, what a rewind
    discards), the space destroying them frees, what it keeps, its undo, a safer action, what stops it (a clone, a
    hold, a host that has the pool imported now) and who else may use the pool. A read that fails leaves its part
    unknown; only a stop fails the plan, as ZFS would refuse the change, and the change checks the same stops itself
    before it imports anything read/write. Every name and text a pool gives (clones, hold tags, hosts) is untrusted.
    The review is a field the wire defaults, so `VERSION` stays 2, which no release has shipped.
  - **"files" lost,** which the point said before: the files a rollback or a rewind loses would take `zfs diff`, which
    mounts the filesystem and, against the live one, takes a snapshot of it on a writable pool, which a plan must not.
    The page gives the bytes written since the snapshot (`written@<snapshot>`), and `f` shows the files.
  - **a plan never acts:** since PM8 (7b963be6, 29 October 2026, in no release; DM8 kept it), `Action::Plan` of an
    action that has no plan performed the action, though the plan of a change never did: `Plan(Close)`,
    `Plan(UnlockAll)` or `Plan(OpenWithKeyFile)` closed or opened LUKS volumes past the SSH session's refusals and the
    locked menu's lock, which leaves plans free. Fixed in UI10d-3: the plan of anything but a change or the boot of a
    snapshot fails and runs nothing, and so does the diff's, a change that only reads; a plan is a read, neither
    recorded among the actions nor followed by a second read of the machine; and the SSH session refuses the plan of
    what the menu does not show: an action that is no change and boots no snapshot, a change of a filesystem that is no
    boot environment, or of a pool the machine does not list.
  - **the page:** under the workspace map, `r`, `Rewind to the Checkpoint…` and `Force the Import…` open a review page
    in place of the typed-name alert, and so does every destroy; the classic map keeps its alerts as they were. The page
    is the whole view in the danger colour and asks the machine for the plan as it opens: what stops the change, first
    and in place of the field; what is lost, which scrolls, with its sizes under their total; the space freed and what
    is kept; the steps, their commands muted; the undo, or that there is none; the safer action; who else may use the
    pool; and the field and the buttons at the bottom. It is a context of its own (`Context::Review`), in which letters
    type, and `Tab` and `Shift-Tab` move among the field, the safer action and the change. The change is sent only with
    its plan, nothing that stops it, an outcome that did not fail, and the name typed as it is; `Escape` sends nothing,
    and cancels the plan only when the machine reads nothing else meanwhile, as a cancel reaches every action not done.
    The line view prints the page as lines, the first 20 losses and how many more, and cannot scroll or take the safer
    action, as its help line says. The demo answers plans, with a clone and a hold on its snapshots.
  - **a page can go stale,** which the point did not say: it shows the machine as it was planned, and a change of the
    machine while it is open plans it again. A rollback destroys what is newer when it runs, which may be more than
    the page listed when something else took a snapshot since; a destroy names its snapshots, and destroys no other.
- **Point 13:** as decided, but for holds. The page lists the snapshots and bookmarks newer than the target by
  `createtxg`, so that two taken in the same second keep the order ZFS took them in, each snapshot with its size, the
  space destroying them frees, and what was written since; the older snapshots and bookmarks kept; no undo; and
  cloning the snapshot as the safer action, whose name the menu asks for. A clone or a hold of a newer snapshot stops
  it, named: the point named clones only before, and the 7 November decision widened it, as `zfs rollback -r` refuses
  a hold too, but only once it has destroyed the other newer snapshots and bookmarks, so the change checks both before
  it imports the pool read/write. No safety snapshot is taken, and the design pages no longer speak of one.
- **Point 14:** as decided, in the workspace alone (point 10), and for the snapshots of boot environments alone.
  `Action::Destroy` destroys the snapshots of one boot environment in one `zfs destroy <environment>@a,b,c`, all or
  none, which keeps the bookmarks. It drops repeated names and refuses, before the pool is imported read/write, an
  empty list, snapshots of several filesystems, a filesystem that is no boot environment, and any clone or hold of
  one of them. The lock keeps it; over SSH it is refused, and so is its plan, unless every snapshot is of a boot
  environment of the machine. Its page lists each snapshot with its size, the space they free together, the
  bookmarks kept, no undo and nothing safer. `Delete` on the Snapshots tab destroys the marked snapshots, the oldest
  first, or else the selection; `Destroy It…`, last in a snapshot's `Enter` sheet, destroys that snapshot alone; the
  palette over the Snapshots tab has an entry for the selection and one for the marked snapshots. `zfs destroy -nv`
  prints the space freed rounded to units, and only `-p` gives the exact bytes the review reads; it covers the
  snapshots of one filesystem a command, as the destroy does; the point now says `-n -v -p`. Marks are a set in both
  maps, with no cap: the classic menu no longer drops the first of three either. `f` compares the selection or the
  one marked with now, and two marked the older against the newer; with three or more, or marks whose list is not
  read, it compares nothing and says so; a list read again drops the marks it no longer has. The classic map offers
  no destroy, and the offers test leaves `Offer::Destroy` out for it.
- **Point 15:** as decided, but for "when it was last imported", which the point said before: what the pool records
  is the time of its last uberblock (`zdb -e -C -u`), when it was last written, not imported, and the page says so.
  The forced import's page names the last host's ID and name and that time; a pool another system has imported now
  stops it, and the import refuses such a pool too; its undo is the export; nothing is safer, as importing as the
  last host is not. The rewind's page names what is lost since the checkpoint: when it was taken and the space it
  holds (`zpool status -p`), the filesystems and snapshots made since, by their creation time, and every other write;
  the read-only import without the rewind is safer. For an imported pool "who else" is this host; for one that is
  not imported it is the host the pool records, and the checkpoint's time, and so what was made since, are unknown.
- **Checks:** `zbm-zfs`'s parsers of `zfs list` with `createtxg`, `userrefs` and `clones`, `zfs holds`, `zfs destroy
  -n -v -p`, the checkpoint of `zpool status -p` and the uberblock of `zdb -e -C -u` on written fixtures, fuzzed in
  `program_output` with seeds; `FakeZfs` refusing as ZFS does (a clone stops `rollback -r` before anything is
  destroyed, a hold only after the rest is); `zbm-app`'s tests of each review, stop and refusal on `FakeZfs` and
  `FakeSystem`, a plan that performs nothing, the SSH refusals and the wire; the menu's tests that a change is sent
  only once planned and named, that `Escape` sends nothing and that the classic map opens its alerts; the offers test
  for each map alone; 42 new wireframes (`workspace-rollback-review`, `-rollback-refused`, `-rewind-review`,
  `-force-import-review`, `-snapshot-actions`, `-destroy-review` and `-destroy-refused`, at every size), the 12 of
  `workspace-startup` and `workspace-import-problem` changed by point 9's keys, and the classic ones unchanged; the
  `menu_names` fuzz target drawing the pages with hostile clone, hold, host and filesystem names; the palette's
  destroy entries within B5 at 10,000 snapshots. No VM scenario rolls back, imports, rewinds or destroys through the
  menu: that is UI10e's. The VM run of UI10d, on rebuilt images, is the maintainer's, and the recordings of the new
  outputs wait for `xtask record-zfs`.
- **Not built:** UI14's dry run, the same view without the field; destroying an environment (point 18); under
  `zbm.import_policy=force`, the menu's change between read-only and read/write forces the import without a page, as
  point 3 says; and an `Escape` or `Enter` typed ahead on start-up's Boot tab reaches the next dialog, as in the
  classic map.

What remains of UI10 is UI10e, the exit (points 16 and 17): the VM tests that drive each core task through the menu,
these pages among them, the hostile-names fuzz target over every view of both menus, the wireframes of point 16 that
are missing (every view has its 52×20 in the pseudo-language, but the classic views have none at 80×24), the key
section of the man page generated from `keymap/`, and `CHANGES-FROM-LEGACY.md` as the migration guide.

## Questions for the maintainer

1. **The workspace before the sessions.** Proposed: it becomes the menu in UI10c, with `zbm.menu=classic` for one
   release, and the sessions run on it. The alternative keeps the classic menu until UI11 has run and builds only
   UI10a and UI10b now.
2. **What the machine cannot do.** Proposed: not offered at all. The alternative shows it, marked unavailable.
3. **Destroy.** Proposed: snapshots only, the selection or the marked ones, from the `Enter` menu, the palette and
   `Delete`, refused when a clone or a hold depends on one. The alternatives also destroy an environment, or put
   destroy on a letter.
4. **The boot history.** Proposed: a `boot.json` beside each saved boot's logs, the Log tab's list, the last boot in
   the inspector, "loaded" until #7 can say "booted". The alternative reads the journal and the TPM event log of each
   saved boot, which name what was booted only at the debug level or for a measured boot.
5. **The review page and the dry run.** Proposed: one view of the plan, the review page adding what is lost and the
   typed confirmation, with `zbm-app` computing what is lost, kept, the undo and the safer action. The alternative is a
   page of its own in the view, which computes those.
6. **The import.** Proposed: one `Import(pool, ImportOptions)` for every way to import, the forced import and the
   rewind behind review pages. The alternative keeps three actions.
7. **Out of UI10.** Proposed: package changes (#21), the checks (#6), boot tries (#7), kernel comparison (#22),
   destroying an environment, and a persistent command-line edit (UI14). The alternative brings package changes in,
   for the distributions of the VM tests.
8. **The tab bar.** Proposed: the wireframes' (pools, volumes, attention), with the host and the time from 120
   columns, and the ZFS version in the help. The alternative is ADR 0007's as written.

## Answers of the maintainer

Accepted as proposed, on 3 November 2026:

1. **The workspace before the sessions:** it becomes the menu in UI10c, with `zbm.menu=classic` for one release, and
   the sessions run on it.
2. **What the machine cannot do:** not offered.
3. **Destroy:** snapshots only, the selection or the marked ones, from the `Enter` menu, the palette and `Delete`,
   refused when a clone or a hold depends on one.
4. **The boot history:** a `boot.json` beside each saved boot's logs, listed in the Log tab, the last boot in the
   inspector, "loaded" until #7 can say "booted".
5. **The review page and the dry run:** one view of the plan, `zbm-app` computing what is lost, kept, the undo and the
   safer action; the review page adds the typed confirmation.
6. **The import:** one `Import(pool, ImportOptions)`.
7. **Out of UI10:** package changes (#21), the checks (#6), boot tries (#7), kernel comparison (#22), destroying an
   environment, and a persistent command-line edit (UI14).
8. **The tab bar:** the wireframes', with the host and the time from 120 columns, and the ZFS version in the help;
   ADR 0007's decision 1 changes so.
