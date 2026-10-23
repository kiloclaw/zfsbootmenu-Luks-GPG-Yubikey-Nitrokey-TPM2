# The menu design

The design of the new menu of ZFSBootMenu (REFACTOR-ROADMAP.md, phase D), which R5 builds on Ratatui and
`nucleo-matcher` in `zbm-tui`. It replaces the `fzf` menu; the principles are in the roadmap's section
*The menu redesign*.

| Page | What it settles |
| --- | --- |
| [Apple's design rules, adapted](apple-hig.md) | the Human Interface Guidelines translated to the console: navigation bar, grouped lists, inspector, alerts, action sheets, toolbar, glyphs |
| [Users, tasks and scenarios](users-and-tasks.md) | who uses the menu, the core tasks with their keystroke budgets, the scenarios the sessions and the VM tests follow |
| [Information architecture](information-architecture.md) | the frame, the views and dialogs, the main view, navigation |
| [The visual system](visual-system.md) | colour roles, monochrome and ASCII forms, terminal capabilities, layout by size |
| [The key map](keymap.md) | one meaning per key, the bindings, the classic `MOD+key` aliases (decision 8) |
| [Boot health](boot-health.md) | will it boot (checks), boot counting with a fallback, boot once, restoring files from a snapshot |
| [What the menu needs from `zbm-app`](interfaces.md) | the `Snapshot`, `Action` and event lists handed to R4 |
| [Usability sessions](usability.md) | who, how, the task script, what to record |
| [Findings](findings.md) | what the sessions found, and what was done about it |
| [Wireframes](wireframes/) | every view at 120×40, 80×25 and 52×20, drawn by the prototype |

## The prototype

```sh
cargo run -p zbm-tui -- --demo
```

runs the menu on a fake machine, with every core task; `--failing` and `--next-boot` show the fallback and the
one-time boot of [boot health](boot-health.md). Its code is the start of R5: `theme.rs` and `keymap.rs` hold
the visual system and the key map, `menu::world` the lists for `zbm-app`, and the wireframes are its snapshot tests,
so the design and the prototype cannot drift apart.
