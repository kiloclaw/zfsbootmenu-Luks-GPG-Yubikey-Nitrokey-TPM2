# The menu design

The design of the menu of ZFSBootMenu, which replaces the `fzf` menu: the menu as a workspace for people who know ZFS,
without losing those who do not ([ADR 0007](../adr/0007-menu-workspace.md), ROADMAP.md *The menu: a workspace*). Eight
mockups of 27 October 2026 drew it: the boot workspace, drill-down columns for kernels, the snapshot timeline, the
command line with a dry run, the rollback review, pools, the start-up timeline, and the same workspace at 80×24 in
ASCII. These pages turn them into rules, and UI10 builds them in `zbm-tui`.

Two key maps exist until the usability sessions (UI11) choose between them ([decision 8](keymap.md)): the current map,
which the menu uses today, and the workspace's, with its tabs. The four rules of keys that work on every keyboard
(decision 18) hold for both.

| Page | What it settles |
| --- | --- |
| [Apple's design rules, adapted](apple-hig.md) | the Human Interface Guidelines translated to the console: navigation and tab bars, grouped lists, inspector, alerts, action sheets, toolbar, glyphs |
| [Users, tasks and scenarios](users-and-tasks.md) | who uses the menu, the core tasks with their keystroke budgets and their measures in both maps, the scenarios the sessions and the VM tests follow |
| [Information architecture](information-architecture.md) | the workspace: its frame, tabs, columns, inspector, what needs attention, the command line and review pages; the views and dialogs |
| [The visual system](visual-system.md) | colour roles, monochrome and ASCII forms, terminal capabilities, layout by size |
| [The key map](keymap.md) | the four rules of decision 18, the workspace's map, the current map, the classic `MOD+key` aliases |
| [Boot health](boot-health.md) | will it boot (checks), boot counting with a fallback, boot once, restoring files from a snapshot, the packages an upgrade changed |
| [Unlocking](unlocking.md) | why the TPM kept a key and sealing it again, the keyboard layout of a passphrase |
| [The menu's text](text.md) | the Fluent catalogue every string comes from, titles and plurals, the lint, the pseudo-language, adding a language |
| [What the menu needs from `zbm-app`](interfaces.md) | the `Snapshot`, `Action` and event lists handed to R4 |
| [Usability sessions](usability.md) | who, how, the task script in both maps, what to record |
| [Findings](findings.md) | what the sessions found, and what was done about it |
| [Wireframes](wireframes/) | every view at 120×40, 80×25 and 52×20, and the workspace at 80×24 as over a serial line, drawn by the prototype |

## The prototype

```sh
cargo run -p zbm-tui -- --demo               # the current map
cargo run -p zbm-tui -- --demo --workspace   # the workspace: its tabs and keys
```

runs the menu on a fake machine, with every core task; `--failing` and `--next-boot` show the fallback and the
one-time boot of [boot health](boot-health.md), and `--tpm-changed` a TPM that kept a key ([unlocking](unlocking.md)).
`--workspace` draws the Boot tab of the workspace (the tab bar, the sidebar of pools, volumes and what needs attention,
the inspector with its next steps) and reads keys with the workspace's map; the other tabs show the current views for
now. `--theme ascii --size 80x24` shows it as over a serial line. Its code is the start of R5: `theme.rs` and
`keymap.rs` hold the visual system and both key maps, `menu::world` the lists for `zbm-app`, and the wireframes are
its snapshot tests, so the design and the prototype cannot drift apart.

What the prototype does not have yet, and which UI10, UI14 and UI15 build: the snapshot timeline and the columns of
the other tabs, the boot history, the command line with its dry run (`:` opens the command palette for now), the
review pages, keys by position on the display console, and the start-up view as the Boot tab.
