# ADR 0016: The rest of the menu, built as the workspace

- **Status:** proposed, 3 November 2026
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
  clone, clone and promote, duplicate), the diff, the shell and the power actions. Eleven actions the menu offers end
  in "not available in this program yet" (`application::unavailable`): booting a snapshot, setting and inheriting the
  default kernel, restoring files, resetting boot tries, resealing, the chroot, the read/write import, the rewind to a
  checkpoint, an import with any policy, and unlocking a volume. The menu's `App` has no LUKS session
  (`App::new(..)` without `with_luks`, `zbm/src/menu.rs` and `remote.rs`), so no volume shows, and the Pools view's
  unlock dialog asks for a passphrase that the machine then refuses.
- **What the machine shows.** `Snapshot` has only imported pools, no devices or vdevs, no foreign host IDs; its checks
  and boot tries are empty (features #6 and #7 are prototypes); the log is the journal's warnings and errors; the
  snapshots of every environment are read when the menu starts, two listings each.
- **What is undefined.** Destroy has no action, key or vocabulary entry, though ADR 0007 gives it a review page. The
  boot history (#9) is one cell of a table: the saved logs (`zbm_boot::saved_logs`) hold the journal, the crash
  reports and the kernel log, but nothing says which environment and kernel a saved boot booted. The review page's
  parts beyond the plan's steps (what is lost, the undo, a safer alternative) have no data. The rollback's
  confirmation in `information-architecture.md` speaks of a safety snapshot, which the menu does not take
  (`CHANGES-FROM-LEGACY.md`, "Rollback has no safety snapshot"). `Import(pool, ImportPolicy)` cannot say read-only or
  read/write, or a rewind. The tab bar's contents differ between ADR 0007 (host, ZFS version, what is degraded, the
  time) and the wireframes (the worst pool, the open volumes, what needs attention).
- **The exit.** No VM test drives a snapshot change, a diff, the palette, a kernel, an import or an unlock through
  the menu; the hostile-names fuzz target draws seven classic screens; the man page has no key section; the
  10,000-snapshot test and the budget B5 do not exist, and ROADMAP decision 7 (Nucleo or its matcher) is open.

## Decision

**The menu becomes the workspace on every machine, with everything it offers performed by the machine; what the
machine cannot do is not offered; the classic menu stays one release behind `zbm.menu=classic`.**

### What the machine does

1. **Every action the menu offers has a use case in `zbm-app`,** or is not offered:
   - **kernels:** `SetDefaultKernel` and `InheritDefaultKernel` write or inherit `org.zfsbootmenu:kernel`, as a
     change (the pool read/write first, as the other changes);
   - **snapshots:** `BootSnapshot` clones the snapshot to a new environment, as `Clone` does, and boots it;
     `RestoreFiles` copies the chosen files from the snapshot, mounted read-only, into the environment, as a change;
   - **the chroot:** `Chroot` runs `zfs-chroot` on the console for the selection, read-only for a snapshot, until
     SB6 makes it an applet;
   - **pools:** one `Import` (decision 3) for every way to import, and the export;
   - **LUKS volumes:** the menu's `App` has a LUKS session, on the console and over SSH, and unlocking one volume
     (`Unlock`) goes to the keyholder as start-up's does (SB10), so the menu never holds the key it opens.
2. **The model says what the machine can do.** `Snapshot` gains `offers`, the kinds of action this machine performs
   now (none of the volumes' without a LUKS session, no reseal without a TPM2 token, no boot tries before #7). The
   menu shows only what is offered: in the toolbar, the `Enter` menus, the palette and the help, and a key bound to
   something not offered says why in the status line rather than opening a dialog. `unavailable` stays for actions
   that come over the wire from an older menu.
3. **One import.** `Action::Import(PoolName, ImportOptions)`, where `ImportOptions` says read-only or read/write, the
   `ImportPolicy` (as this host, as the host that owns it, forced), and whether to rewind to the checkpoint. It
   replaces `Import(pool, policy)`, `ImportReadWrite` and `RewindCheckpoint`; the import dialog's choices are its
   values, and each that cannot be undone (forced, the rewind) opens its review page.
4. **A diff needs no writable pool.** It mounts the snapshots read-only and loads their key if needed, but no longer
   imports the pool read/write (`Diff` leaves `is_change`).

### What the machine shows

5. **Pools as they are.** `Snapshot` lists the pools that can be imported as well as the imported ones, with the host
   that owns each and whether it is foreign, their vdevs and devices with their states and errors, and the checkpoint.
   `Enter` on a device shows its kernel messages.
6. **Snapshots on demand.** The snapshots of an environment are read when its column opens (`LoadSnapshots`,
   `SnapshotsLoaded`), not at start, and kept until a change touches them. The 10,000-snapshot test measures it.
7. **The boot history (#9).** Each saved boot gets a record of what was booted, `boot.json` beside its logs: the
   time, the environment, the kernel and initramfs, the command line, whether it was measured, and whether `kexec`
   failed (written again by `update_logs`). The Log tab lists the saved boots, newest first, with their records and
   their files; the inspector of an environment says when it last booted, and with which kernel. Whether a boot
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
10. **The classic menu stays one release.** `zbm.menu=classic` draws the current screens with the current map; it goes
    with RL2, as the legacy runtime does. The line view (UI8) follows the workspace: it prints the tab's column, and
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
    `information-architecture.md` loses the safety snapshot.
14. **Destroy.** `Action::Destroy(Vec<SnapshotName>)` destroys snapshots: the selection, or the snapshots marked with
    `Space`, which is also how #15 reclaims space. The page lists each with the space it frees and what depends on it
    (clones, holds, bookmarks, a default environment's), and refuses a snapshot that a clone depends on. The key is
    `Delete`, and `:destroy` on the command line (ADR 0007, rule 1); no letter, while decision 8 is open. Destroying
    an environment is not in UI10.
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
  behind the PIN) are what stand between a key and a lost pool. `Plan` stays free under the lock, so a locked menu
  shows what an action would do; this record makes that explicit.
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
- **Destroy on a letter.** `x` is power and `d` the default; the free letters (`a`, `g`, `q`, `t`, `v`) mean nothing
  for it, and the map is open. `Delete` and the command line are rule 1's way.
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

## Questions for the maintainer

1. **The workspace before the sessions.** Proposed: it becomes the menu in UI10c, with `zbm.menu=classic` for one
   release, and the sessions run on it. The alternative keeps the classic menu until UI11 has run and builds only
   UI10a and UI10b now.
2. **What the machine cannot do.** Proposed: not offered at all. The alternative shows it, marked unavailable.
3. **Destroy.** Proposed: snapshots only, the selection or the marked ones, on `Delete` and `:destroy`, refused when a
   clone depends on one. The alternatives also destroy an environment, or put destroy on a letter.
4. **The boot history.** Proposed: a `boot.json` beside each saved boot's logs, the Log tab's list, the last boot in
   the inspector, "loaded" until #7 can say "booted". The alternative reads the journal of each saved boot.
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
