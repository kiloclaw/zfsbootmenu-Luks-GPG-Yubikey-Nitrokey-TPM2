# The menu design

The design of the menu of ZFSBootMenu, which replaces the `fzf` menu: the menu as a workspace for people who know ZFS,
without losing those who do not ([ADR 0007](../adr/0007-menu-workspace.md), ROADMAP.md *The menu: a workspace*). Eight
mockups of 27 October 2026 drew it: the boot workspace, drill-down columns for kernels, the snapshot timeline, the
command line with a dry run, the rollback review, pools, the start-up timeline, and the same workspace at 80×24 in
ASCII. These pages turn them into rules, and UI10 builds them in `zbm-tui`.

Two key maps exist until the usability sessions (UI11) choose between them ([decision 8](keymap.md)): the workspace's,
with its tabs, which the menu uses, and the classic map, which `zbm.menu=classic` keeps for one release. The four rules
of keys that work on every keyboard (decision 18) hold for both.

| Page | What it settles |
| --- | --- |
| [Apple's design rules, adapted](apple-hig.md) | the Human Interface Guidelines translated to the console: navigation and tab bars, grouped lists, inspector, alerts, action sheets, toolbar, glyphs |
| [Users, tasks and scenarios](users-and-tasks.md) | who uses the menu, the core tasks with their keystroke budgets and their measures in both maps, the scenarios the sessions and the VM tests follow |
| [Information architecture](information-architecture.md) | the workspace: its frame, tabs, columns, inspector, what needs attention, the command line and review pages; the views and dialogs |
| [The visual system](visual-system.md) | colour roles, monochrome and ASCII forms, terminal capabilities, layout by size |
| [The key map](keymap.md) | the four rules of decision 18, the workspace's map, the classic map, the `fzf` aliases (`MOD+key`) |
| [Boot health](boot-health.md) | will it boot (checks), boot counting with a fallback, boot once, restoring files from a snapshot, the packages an upgrade changed |
| [Unlocking](unlocking.md) | why the TPM kept a key and sealing it again, the keyboard layout of a passphrase |
| [The menu's text](text.md) | the Fluent catalogue every string comes from, titles and plurals, the lint, the pseudo-language, adding a language |
| [What the menu needs from `zbm-app`](interfaces.md) | the `Snapshot`, `Action` and event lists handed to R4 |
| [Usability sessions](usability.md) | who, how, the task script in both maps, what to record |
| [Findings](findings.md) | what the sessions found, and what was done about it |
| [Wireframes](wireframes/) | every view at 120×40, 80×25 and 52×20, and at 80×24 in colour and ASCII as over a serial line but for the four prototypes of ADR 0016 point 18, drawn by the prototype |

## The prototype

```sh
cargo run -p zbm-tui -- --demo               # the workspace: its tabs and keys
cargo run -p zbm-tui -- --demo --classic     # the classic map, as zbm.menu=classic keeps it
```

runs the menu on a fake machine, with every core task; `--failing` and `--next-boot` show the fallback and the
one-time boot of [boot health](boot-health.md), and `--tpm-changed` a TPM that kept a key ([unlocking](unlocking.md)).
The menu is the workspace (the tab bar, the sidebar of pools, volumes and what needs attention, the Boot tab's
columns with the inspector and its next steps, the Snapshots tab's timeline, the Pools tab's tree) and reads keys with
the workspace's map; `--classic` draws the classic views with the classic map, and `--workspace` is accepted and
changes nothing. `--theme ascii --size 80x24` shows it as over a serial line. Its code is the start of R5: `theme.rs` and
`keymap/` hold the visual system and both key maps, `menu::world` the lists for `zbm-app`, and the wireframes are
its snapshot tests, so the design and the prototype cannot drift apart.

What the prototype does not have yet, and which UI14 builds: the command line with its dry run (`:` opens the command
palette for now). Keys by position on the display console are built (UI15, rule 3 of [the key map](keymap.md)), on the
console that starts with the image (`zbm.console=drm`); the demo's own display (`--drm`) does not name the layout of its
keys, so there they count as typed. The review pages are built (UI10d, ADR 0016 points 12 to 15): the rollback, the
destroy, the forced import and the rewind each open one, and the demo answers their plans. Start-up is the Boot tab
under the workspace map: its stages where the columns go, the tab bar above them, and a stage that failed among what
needs attention; the person may look at the other tabs while it runs
([information-architecture.md](information-architecture.md), start-up).
