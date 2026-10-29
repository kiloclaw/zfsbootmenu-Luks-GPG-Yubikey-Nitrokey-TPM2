# The key map

One table, `keymap.rs` of `zbm-tui`, turns key presses into intents, writes the toolbar and the help view, and will
generate the key binding section of `zfsbootmenu(7)`. It holds two maps until the usability sessions (UI11) choose one
([decision 8](../../../ROADMAP.md#decisions)): the **workspace's** ([ADR 0007](../adr/0007-menu-workspace.md)), and the
**current** one, which the menu uses today. `zbm-tui --demo --workspace` reads keys with the first, `--demo` with the
second.

## Keys on every keyboard

Decision 18, for both maps:

1. **No action needs one character only.** The arrows, `Enter`, `Esc`, `Tab` and the command line reach every action:
   each action bound to a character is also a command and an entry of the `Enter` menu of what it applies to. `j` `k`
   `h` `l` move as the arrows do; the digits only shorten the way to a tab or a start-up choice.
2. **No action depends on case.** A letter and its capital are one key; Shift and Caps Lock change no action. Risky
   actions open their review page instead of hiding behind Shift.
3. **Keys by position on the display console.** A character the map does not bind counts as the character the same key
   types on the US layout, at the same level, through the layout's `Positions` (UI1): `й` is `q`, AZERTY's `&` is `1`,
   `Ctrl` with the key labelled `с` is `Ctrl-C`. Characters the map binds count as typed. Text fields take characters
   as typed. Over a serial line or SSH only characters arrive, and rule 1 covers them.
4. **The command line knows every language.** It matches the command names of the catalogue in the menu's language
   and the English names.

A test checks rule 2 and one meaning per key on both tables. Rule 1's commands and rule 4 come with the command line
(UI14), rule 3 with UI15.

## The workspace's map

One meaning per key in the whole workspace, no capital letters, and the classic `MOD+key` aliases below.

| Where | Keys |
| --- | --- |
| Everywhere | `Enter` the primary action; `Esc` back, out of a column, closes a page; arrows and `j` `k` `h` `l` move, `h` `l` out of and into a column; `PgUp` `PgDn` `Home` `End`; `Tab` and `Shift-Tab` the next and previous tab, `1` to `4` a tab; `/` filter; `:` command line; `?` and `F1` help; `!` a shell: into the selection (read-only for a snapshot), or the recovery tools; `x` power; `Ctrl-L` redraw |
| Boot | `b` boot the selection; `e` edit the command line once; `d` make it the default; `s` its snapshots; `n` a new snapshot |
| Snapshots | `Enter` what to do with it; `b` boot it as a clone; `c` clone…; `m` clone and promote…; `y` duplicate…; `r` roll back…; `f` the files and packages changed since, or between the marked ones, and restoring them; `Space` mark; `z` fold or unfold; `o` sort; `n` a new snapshot |
| Pools | `i` import…: read-only, read/write, as the host that owns it, at its checkpoint, or forced; `w` read/write…; `u` unlock…; `Enter` on a device: its kernel messages |
| Start-up | `1` to `9` a choice, as do `j` `k` and `Enter` |
| Text fields | `Tab` completes; `Ctrl-W` deletes a word; `Ctrl-R` searches the history (the command line) or shows what is typed (the unlock dialog); `Ctrl-K` the next keyboard layout |

Against the current map: `j` `k` `h` `l` move, so chroot leaves `j` for `!`, the kernels leave `k` for `l`, the rewind
leaves `h` for the import page, and the log leaves `l` for its tab; the pools are tab 3, so `p` is free; `Tab` goes to
the next tab, so `Space` alone marks.

**In the prototype** (`keymap/workspace.rs`): its bindings come before the current map's, whose bindings of the same
keys and `p` it drops: `j` `k` `h` move, `l` goes into the kernels, `1` to `4`, `Tab` and `Shift-Tab` choose the tab.
The other keys are the current map's until UI10: `!` opens the recovery tools, and the pools keep `i`, `u` and `w`;
the rewind, which the current map has on `h`, waits for the import page of UI10.

## The current map

The menu's map today, which the sessions compare with the workspace's.

### Its rules

1. **One meaning per key.** A key does the same thing, or nothing, in every view. Where the `fzf` menu gave `MOD+D`
   three meanings, the new map gives each action its own key.
2. **Letters for actions, `/` for the filter.** Lists do not filter as you type: a single letter runs an action, which
   is shown in the footer. `/` starts the filter, `Esc` ends it. This trades the `fzf` habit of typing at once for
   one-key actions that a footer can teach.
3. **Nothing destructive on one key.** A key may open a destructive action; the confirmation needs the target's name
   typed.
4. **Any key stops the countdown; only `Enter` (or `b`) boots.**
5. **Keys every terminal sends.** Plain letters, `Enter`, `Esc`, `Tab`, arrows, and `Ctrl` or `Alt` with a letter.
   `F`-keys and `Shift` with arrows are extras, never the only way. `Ctrl-S` and `Ctrl-Q` are flow control on serial
   lines, `Ctrl-J` and `Ctrl-M` are `Enter`, `Ctrl-H` is backspace on some terminals: none of them is bound to an
   action.
6. **Keys are the characters on their labels.** After `Ctrl-K` chose another keyboard layout, every key is translated
   by its position, view keys too: the key labelled `/` filters wherever the layout puts it
   ([unlocking](unlocking.md)).

### Every view

| Key | Intent |
| --- | --- |
| `Enter` | the primary action of the view |
| `Esc`, `Ctrl-C` | back: close the dialog, end the filter, or return to the main view |
| `Up`, `Down`, `Ctrl-P`, `Ctrl-N` | move the selection |
| `PgUp`, `PgDn`, `Home`, `End` | move by a page, to the first, to the last |
| `/` | filter the list (in lists) |
| `:` | the command palette |
| `?`, `F1` | help |
| `Ctrl-L` | redraw the screen (for consoles that garbled it) |
| `Ctrl-K` | in a text field: the next keyboard layout ([unlocking](unlocking.md)) |
| `Ctrl-R` | in the unlock dialog: show what is typed, or hide it (not during start-up) |

### Actions

| Key | Intent | Views |
| --- | --- | --- |
| `b` | boot (the selection, or a clone of the selected snapshot) | main, kernels, snapshots |
| `k` | the kernels of the selected environment | main |
| `s` | the snapshots of the selected environment | main |
| `e` | edit the command line once | main, kernels |
| `d` | make the selection the default (environment of its pool, kernel of its environment); again: inherit | main, kernels |
| `p` | pools and volumes | main |
| `w` | import the pool of the selection read/write | main, pools |
| `j` | chroot into the selection (snapshots: read-only) | main, snapshots |
| `o` | sort order: name, creation, size | snapshots |
| `l` | the log | every view |
| `!` | recovery tools | every view |
| `x` | power: power off, reboot, firmware setup | every view |
| `r` | roll back to the selected snapshot | snapshots |
| `c` | clone the selected snapshot | snapshots |
| `m` | clone and promote (moves the snapshots to the clone) | snapshots |
| `y` | duplicate the selected snapshot (an independent copy, as `zfs send` and `zfs recv` make) | snapshots |
| `n` | new snapshot of the environment | main, snapshots |
| `f` | the changes since the snapshot, or between two marked snapshots | snapshots |
| `Space`, `Tab` | mark the selection (snapshots for `f`, files to restore) | snapshots, changes |
| `Enter` | restore the marked files, or the selected one; on a package, show its files | changes |
| `Left`, `Right` | the other segment: files or packages | changes |
| `u` | unlock the selected volume | pools |
| `i` | import the selected pool | pools |
| `h` | rewind the pool to its checkpoint | pools |

## The classic bindings

[Decision 8](../../../ROADMAP.md#decisions) asked whether to offer a second profile close to the `fzf`
menu. The proposal: no profile to choose, but every `MOD+key` binding of the `fzf` menu (`MOD` is `Alt` or `Ctrl`)
keeps working, with its old meaning in its old view, as an alias that the help view lists under *classic*:

| Classic | Main | Snapshots | Kernels | Pools |
| --- | --- | --- | --- | --- |
| `MOD+K` | kernels | | | |
| `MOD+S` | snapshots | | | |
| `MOD+D` | set default | diff | set default | |
| `MOD+E` | edit command line | | edit command line | |
| `MOD+T` | revert the edited command line | | | |
| `MOD+P` | pools | | | |
| `MOD+R` | recovery shell | roll back | | rewind checkpoint |
| `MOD+J` | chroot | chroot | | |
| `MOD+W` | import read/write | | | |
| `MOD+O` | | sort order | | |
| `MOD+L` | log | log | log | log |
| `MOD+X` | power | clone and promote | | |
| `MOD+C` | | clone | | |
| `MOD+N` | | new snapshot | | |
| `MOD+U` | | | inherit the default | |

The `fzf` menu's `MOD+O` also sorted the main list; the environments carry no creation time or size to sort by,
so here it sorts the snapshots view only.

So people who know the `fzf` menu keep their habits, and new users learn the letters from the footer. The new
letters hold no modifier, so the two maps meet only where the new map gives `Ctrl` a meaning of every view (`Ctrl-C`
back, `Ctrl-L` redraw, `Ctrl-N` and `Ctrl-P` down and up): there the new meaning wins, and the classic binding works
with `Alt` only. `Ctrl-S` and `Ctrl-J` may not reach the menu on serial lines (flow control, newline): their `Alt`
forms do. The sessions confirm or reject this before R5.

## The toolbar and the help view

The toolbar (the last line, [apple-hig.md](apple-hig.md) rule 9) shows, for the focused element, the bindings marked
for it in the order of the table, as `Enter Boot`: at most five, whole ones only, as many as fit, then `? More` on the
right, which opens help. The help view lists every binding of the view it was opened from, then the global ones, then
the classic aliases. In alerts and action sheets `Esc` reads *Cancel*.
