# Wireframes

Every view and the main dialogs of the menu, drawn by the `--demo` prototype on its fake machine. They are the
snapshot tests in `crates/zbm-tui/src/menu/view/wireframes.rs`, so they always show what the code draws.

Each file is one screen as text, after the scenario named in `wireframes.rs` (the keys pressed from power-on):

| Name | What it shows |
| --- | --- |
| `startup` | start-up with its stages |
| `main` | the boot environments, the countdown and the detail pane |
| `main-filter` | the main view filtered by `ta` |
| `kernels` | the kernels of the default environment |
| `snapshots` | 400 snapshots, newest first |
| `snapshot-actions` | what Enter offers on a snapshot |
| `rollback-confirm` | a rollback, waiting for the environment's name to be typed |
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

Every name comes at three sizes: `-120x40`, `-80x25` and `-52x20`, the smallest the menu lays out for. The `-ascii`
files are the main view and dialogs at 80×25 for terminals without line drawing or UTF-8 (`--theme ascii`). Colour
is not in the text: colour and monochrome draw the same characters, which a test checks, so nothing depends on
colour alone.

## Changing them

Change a view, then:

```sh
cargo test -p zbm-tui wireframes      # fails, showing the difference
cargo insta review                    # or: cargo xtask bless
```

and review the new files in the diff like any other change of the design.
