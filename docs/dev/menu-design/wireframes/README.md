# Wireframes

Every view and the main dialogs of the menu, drawn by the `--demo` prototype on its fake machine. They are the
snapshot tests in `crates/zbm-tui/src/menu/view/wireframes.rs`, so they always show what the code draws.

Each file is one screen as text, after the scenario named in `wireframes.rs` (the keys pressed from power-on):

| Name | What it shows |
| --- | --- |
| `startup` | start-up with its stages |
| `main` | the navigation bar, the countdown banner, the boot environments and the inspector |
| `main-filter` | the search field, filtered by `ta` |
| `kernels` | the kernels of the default environment |
| `snapshots` | 400 snapshots, newest first |
| `snapshot-actions` | the action sheet of a snapshot |
| `rollback-confirm` | the rollback alert, its button disabled until the name is typed |
| `command-line` | the command line edited for one boot |
| `pools` | pools and LUKS volumes |
| `unlock-pin` | a TPM2 PIN asked at start-up, two digits typed and not shown |
| `import-problem` | a pool of another host, and what can be done |
| `import-failed` | the error after a plain retry: what failed, why, what next |
| `palette` | the command palette finding snapshots by date |
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
| `unlock-layout` | typing with the German layout, what is typed shown with `Ctrl-R` |
| `reseal-confirm` | the alert that seals the TPM2 key again, once the volume is open |
| `pools-tpm-changed` | a volume whose TPM2 key is out of date, in the pools view |

Every name comes at three sizes: `-120x40`, `-80x25` and `-52x20`, the smallest the menu lays out for. The `-ascii`
files are the main view and dialogs at 80×25 for terminals without line drawing or UTF-8 (`--theme ascii`). Colour
is not in the text: colour and monochrome draw the same characters, which a test checks, so nothing depends on
colour alone. Another test reads the Unicode tables of the image's console fonts and fails if a wireframe uses a
glyph they lack. The design rules they follow are in [../apple-hig.md](../apple-hig.md).

## Changing them

Change a view, then:

```sh
cargo test -p zbm-tui wireframes      # fails, showing the difference
cargo insta review                    # or: cargo xtask bless
```

and review the new files in the diff like any other change of the design.
