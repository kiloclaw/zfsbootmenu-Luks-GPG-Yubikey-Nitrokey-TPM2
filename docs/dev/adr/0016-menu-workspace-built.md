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
   open, and a change to it is a change of `keymap.rs` alone.
10. **The classic menu stays one release.** `zbm.menu=classic` draws the current screens with the current map; the
    line view still shows by itself where it does now. It goes with RL2, as the legacy runtime does. The line view (UI8) follows the workspace: it prints the tab's column, and
    the tabs are words (`boot`, `snapshots`, `pools`, `log`), since a number chooses an item there.
11. **The tab bar** is the wireframes': the tabs, the worst pool, the open volumes and what needs attention; the host
    and the time join it from 120 columns. This changes ADR 0007's decision 1, which also named the ZFS version: it
    moves to the help view's header.

### Review pages

12. **A review page is a view of the plan.** `Action::Plan` returns a `Plan` with, besides its steps and their
    commands, what the action loses (snapshots with their sizes, files), what it keeps, its undo if it has one, and a
    safer action if there is one, all computed by `zbm-app` from the machine. The page shows them, then asks for the
    target's name. UI14's dry run is the same view without the confirmation.
13. **Rollback.** `zfs rollback -r` destroys the newer snapshots, and nothing taken first could undo it, so the page
    lists those snapshots with their sizes, says it has no undo, and offers cloning the snapshot as the safer action.
    When a newer snapshot has a clone, `zfs rollback -r` refuses, and the page says so and names the clone instead of
    asking. The design pages lose the safety snapshot.
14. **Destroy.** `Action::Destroy(Vec<SnapshotName>)` destroys snapshots: the selection, or the marked ones, which is
    also how #15 reclaims space. Marks become a set; `f` diffs when one or two are marked and says so otherwise. The
    page lists each snapshot with its size, the space they free together (`zfs destroy -nv`), and what stops it: a
    clone or a hold, either of which makes the page refuse and name it. Bookmarks of a destroyed snapshot stay. It is
    an entry of the snapshot's `Enter` menu and of the palette (rule 1), and `Delete` where the terminal sends it; no
    letter, while decision 8 is open. Destroying an environment is not in UI10.
15. **The forced import and the rewind** have pages too: who else may be using the pool (its host ID and host name,
    when it was last imported), and, for the rewind, what is lost since the checkpoint.

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
