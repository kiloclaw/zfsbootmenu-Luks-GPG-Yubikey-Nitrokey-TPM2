# Wireframes

Every view and the main dialogs of the menu, drawn by the `--demo` prototype on its fake machine. They are the
snapshot tests in `crates/zbm-tui/src/menu/view/wireframes.rs`, so they always show what the code draws.

Each file is one screen as text, after the scenario named in `wireframes.rs` (the keys pressed from power-on). The
menu is the workspace, whose wireframes are named `workspace-*`, `log-history-*` and `log-no-history-*`; the others,
`log` among them, are the classic menu's, which `zbm.menu=classic` keeps for one release (`--demo --classic`), and
keep their names:

| Name | What it shows |
| --- | --- |
| `startup` | start-up with its stages |
| `main` | the navigation bar, the countdown banner, the boot environments and the inspector, which knows each environment's count of snapshots and its newest until its snapshots are read |
| `main-filter` | the search field, filtered by `ta` |
| `kernels` | the kernels of the default environment |
| `snapshots` | 400 snapshots, newest first, read when the view opened |
| `snapshot-actions` | the action sheet of a snapshot |
| `rollback-confirm` | the rollback alert, its button disabled until the name is typed |
| `command-line` | the command line edited for one boot |
| `pools` | pools and LUKS volumes |
| `unlock-pin` | a TPM2 PIN asked at start-up, two digits typed and not shown |
| `import-problem` | a pool of another host, and what can be done |
| `import-failed` | the error after a plain retry: what failed, why, what next |
| `palette` | the command palette finding snapshots by date, once the snapshots view read them, under the heading of its actions |
| `recovery` | the recovery tools |
| `log` | the log since start |
| `help` | help for the main view, which scrolls |
| `power` | reboot, power off, firmware setup |
| `notification` | the outcome of setting a default, on the notification line |
| `main-fallback` | the default used up its boot tries, and the fallback the countdown boots (`--failing`) |
| `main-next-boot` | a boot once, asked by the running system (`--next-boot`) |
| `may-not-boot` | booting an environment whose checks found a failure |
| `diff` | the changes since a snapshot, with a preview |
| `diff-marked` | two changes marked to restore |
| `restore-confirm` | the restore alert, its button disabled until the name is typed |
| `diff-packages` | the packages segment of the changes, with the preview of a removed package |
| `unlock-tpm-changed` | the TPM kept a key after a firmware update: the passphrase is asked, and why (`--tpm-changed`) |
| `unlock-wrong` | a wrong passphrase, and the keyboard layout as a likely cause |
| `unlock-layout` | typing with the German layout, `Ctrl-K` to switch to it; start-up never shows what is typed |
| `reseal-confirm` | the alert that seals the TPM2 key again, once the volume is open |
| `pools-tpm-changed` | a volume whose TPM2 key is out of date, in the pools view |
| `workspace-startup` | start-up as the Boot tab: the tab bar, the sidebar from 110 columns, what needs attention as one line at 80 columns, and the stages where the columns go; only `?` in the toolbar, as start-up binds only the tabs, moving and help |
| `workspace-import-problem` | the pool start-up could not import, another host's, its choices over start-up as the Boot tab |
| `workspace-force-import-review` | `Force Import…` there: the review page of the forced import of `backup`, which loses nothing, its step and command, the export that undoes it, and who used the pool last (host ID, name, when it was last written); `back` typed |
| `workspace-rollback-review` | `2`, down three and `r`: the review page of the rollback of `zroot/ROOT/void` to the daily snapshot of 20 October: the newer snapshots and what was written since, their sizes right-aligned under their total, the space freed, the snapshots and the two older bookmarks kept, the step with its command, no undo, the clone that is safer, then the field with the environment's name, `void`, typed in part and the buttons |
| `workspace-rollback-refused` | `2`, down ten and `r`: the rollback to the daily snapshot of 17 October, which the clone of that of 18 October and the hold of that of 19 October stop: the page names them first, has no field and no safer action, and its **OK** closes it |
| `workspace-snapshot-actions` | `2`, down three and `Enter`: the actions of the daily snapshot of 20 October, the workspace's ending with `Destroy It…`, in red as it opens a review page |
| `workspace-destroy-review` | `2`, down three, `Space`, up one, `Space`, then `Delete`: the review page of the destroy of the two marked snapshots, of 20 and 21 October, oldest first, with their sizes under their total, the space freed, the environment's two bookmarks kept, the step with its command, no undo and nothing safer, then the field with the environment's name, `void` (for several snapshots; one snapshot's page asks for its short name), typed in part and the **Destroy** button |
| `workspace-destroy-refused` | `2`, down eleven and `Delete`: the destroy of the daily snapshot of 18 October, which its clone stops: the page names it first, has no field, and its **OK** closes it |
| `workspace-rewind-review` | `i` on `tank` in the Pools tab, then `Rewind…`: the review page of the rewind to its checkpoint: every write since it, the snapshot taken since, the export and the import with their commands, the read-only import that is safer, and this host, which has it imported |
| `workspace-dry-run` | `:`, `snapshot`, `Enter` on the first action, `Take a Snapshot of zroot/ROOT/void`, then the name `before-upgrade` and `Enter`: the dry run of the snapshot, the review page without its field (ADR 0016, point 12): nothing has changed yet, it loses nothing, its steps and command, the destroy that undoes it, in the normal frame with **Run** |
| `workspace` | the workspace's Boot tab: the tab bar, with the host ID and the time at 120 columns, the sidebar of pools, volumes and what needs attention, the path, the boot environments grouped by pool, the kernels of the selection beside them from 80 columns, the inspector with its next steps |
| `workspace-kernels` | `l` from the Boot tab: the kernels column of the second environment, its kernel in the path and the inspector |
| `workspace-columns` | `Right` into the kernels of `tank/ROOT/arch`, the first environment of the second pool, and down one: the path `tank › tank/ROOT/arch › 6.10.10-arch1-1` |
| `workspace-inspector-wrap` | the kernels column of `tank/ROOT/arch` with a long command line: the inspector wraps it under its label |
| `workspace-snapshots` | `2` and `End`: the Snapshots tab, the environments beside the timeline of the default's 400 snapshots: today, the days of this week, the weeks of this month (the first from 1 October) and the months, folded, each under a header with its count and the space it holds; the oldest month selected, its count and space in the inspector, which offers no snapshot's action |
| `workspace-snapshots-reading` | `2` before the machine sent the snapshots: their column says it reads them |
| `workspace-pools` | `3` and down to `tank`: the Pools tab as the tree of the pools, their vdevs and devices and the volumes, `zroot` a mirror and a device below it, `tank` a RAID-Z with a disk missing and one with checksum errors; `tank`'s health, what `zpool` says and its next steps in the inspector |
| `workspace-pools-foreign` | once the scan found the pools start-up left alone, `archive` selected in the Pools tab: another host's, not imported, its host in its row and the inspector |
| `workspace-help` | `?` on the Boot tab: the version of the ZFS module on the right of its header, and the `MOD+key` aliases of the `fzf` menu |
| `workspace-attention` | once the scan after the first frame found the pools start-up left alone, one of them another host's: what needs attention, worst first, each with the tab that shows it |
| `workspace-device-log` | `Enter` on `tank`'s lost disk `sdb2` in the Pools tab: the kernel's messages about it and its disk below the tree, the escape bytes in them drawn as text; the path `tank › raidz1-0 › sdb2` |
| `workspace-filter` | `/` and `ta` on the Boot tab: the search field at the top of the column, two environments of four left |
| `workspace-command-line` | `e` and ` systemd.unit=rescue.target` typed: the command line edited for one boot, under the tab bar |
| `workspace-palette` | `2`, `:` and `2026-10`: the command palette over the Snapshots tab, finding the snapshots of October by date |
| `workspace-commands` | `:` and `pools` on the Boot tab: the command line's sections, actions, items and help, each under its heading |
| `workspace-power` | `x`: power off, reboot, firmware setup, over the Boot tab |
| `workspace-recovery` | `!`: the recovery tools, under the tab bar |
| `workspace-notification` | down one and `d`: the outcome of setting a default, on the notification line, `zroot/ROOT/ubuntu` the new default |
| `workspace-help-snapshots` | `2` and `?`: help for the Snapshots tab, its keys and the `MOD+key` alias it keeps |
| `workspace-log-startup` | `Shift-Tab` during start-up, which goes round to the Log tab, as the digits are not bound then: the stages so far, the log shown once start-up is over |
| `workspace-import-choices` | `i` on `archive` in the Pools tab, another host's pool the scan found: its imports, with this host's ID, with its last host's and forced |
| `workspace-pool-details` | `Enter` on `archive` there: the details of the pool, its health, mode and last host, and what can be done to it |
| `workspace-import-failed` | `Retry`, `Enter`, on the pool start-up could not import: the error after it, what failed, why and what next, over start-up as the Boot tab |
| `workspace-clone-name` | `2`, down three, `c` and `-2`: naming the clone of the daily snapshot of 20 October |
| `workspace-snapshot-name` | `n` on the Boot tab: naming a new snapshot of the default environment, the time as its name |
| `workspace-promote-confirm` | `2`, down three, `m`, `-2` and `Enter`: the second question of Clone and Promote, the environment's last component to type |
| `workspace-diff` | `2`, down three and `f`: the changes since the daily snapshot of 20 October, with a preview, in the Snapshots tab |
| `workspace-diff-marked` | two changes marked to restore, `Space` on each |
| `workspace-restore-confirm` | `Enter` then: the restore alert, its button disabled until the name is typed, `vo` typed |
| `workspace-unlock-pin` | a TPM2 PIN asked over start-up as the Boot tab, two digits typed and not shown (`--locked`) |
| `workspace-unlock-wrong` | a wrong passphrase, and the keyboard layout as a likely cause |
| `workspace-unlock-layout` | typing with the German layout, `Ctrl-K` to switch to it; start-up never shows what is typed |
| `workspace-unlock-tpm-changed` | the TPM kept a key after a firmware update: the passphrase is asked, and why (`--tpm-changed`) |
| `workspace-reseal-confirm` | the alert that seals the TPM2 key again, once the volume is open |
| `workspace-pools-tpm-changed` | the alert declined, `3` and down to `luks-zroot-a`: the volume whose TPM2 key is out of date, `TPM2 changed` in its row after `Volume` |
| `log-history` | `4`: the Log tab with the boot history `zbm.save_logs` kept above the log since start: a measured boot, one saved without a record, one whose record a newer runtime wrote, and one whose kernel's name holds escape bytes, drawn as text |
| `log-no-history` | `4` without `zbm.save_logs`: the Log tab says no boot is kept, and where the option is documented |

Every name comes at three sizes: `-120x40`, `-80x25` and `-52x20`, the smallest the menu lays out for. Every name
comes at 80×24 too, as over a serial line, in colour and in ASCII for terminals without line drawing or UTF-8
(`-80x24`, `-80x24-ascii`, `--theme ascii`), except the four prototypes of items ADR 0016 point 18 leaves out of the
menu: `main-fallback`, `main-next-boot`, `may-not-boot` and `diff-packages`, which, as prototypes of point 18's
items, have no `workspace-*` wireframe either. The main view and dialogs come in ASCII at 80×25 as well
(`-80x25-ascii`). Colour is not in the text: monochrome draws the same characters as colour, and a test draws every
name at 80×24 in monochrome and in ASCII and fails if any cell carries a colour, so nothing depends on colour alone. Another test reads the Unicode tables of the image's console fonts and fails if a wireframe uses a
glyph they lack. The design rules they follow are in [../apple-hig.md](../apple-hig.md).

## Changing them

Change a view, then:

```sh
cargo test -p zbm-tui wireframes      # fails, showing the difference
cargo insta review                    # or: cargo xtask bless
```

and review the new files in the diff like any other change of the design.
