# The menu design

The design of the new menu of ZFSBootMenu (REFACTOR-ROADMAP.md, phase D), which R5 builds on Ratatui and
`nucleo-matcher` in `zbm-tui`. It replaces the `fzf` menu; the principles are in the roadmap's section
*The menu redesign*.

| Page | What it settles |
| --- | --- |
| [Users, tasks and scenarios](users-and-tasks.md) | who uses the menu, the core tasks with their keystroke budgets, the scenarios the sessions and the VM tests follow |
| [Information architecture](information-architecture.md) | the frame, the views and dialogs, the main view, navigation |
| [The visual system](visual-system.md) | colour roles, monochrome and ASCII forms, terminal capabilities, layout by size |
| [The key map](keymap.md) | one meaning per key, the bindings, the classic `MOD+key` aliases (decision 8) |
| [What the menu needs from `zbm-app`](interfaces.md) | the `Snapshot`, `Action` and event lists handed to R4 |
| [Usability sessions](usability.md) | who, how, the task script, what to record |
| [Findings](findings.md) | what the sessions found, and what was done about it |
| [Wireframes](wireframes/) | every view at 120×40, 80×25 and 52×20, drawn by the prototype |

## The prototype

```sh
cargo run -p zbm-tui -- --demo
```

runs the menu on a fake machine, with every core task. Its code is the start of R5: `theme.rs` and `keymap.rs` hold
the visual system and the key map, `menu::world` the lists for `zbm-app`, and the wireframes are its snapshot tests,
so the design and the prototype cannot drift apart.
