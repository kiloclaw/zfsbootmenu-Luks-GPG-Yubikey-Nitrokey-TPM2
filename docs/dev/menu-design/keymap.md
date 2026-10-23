# The key map

One table, `keymap.rs` of `zbm-tui`, turns key presses into intents, writes the footer and the help view, and will
generate the key binding section of `zfsbootmenu(7)` in R5. This page is the design of that table.

## Rules

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

## Every view

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

## Actions

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
| `o` | sort order: name, creation, size | lists |
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
| `Enter` | restore the marked files, or the selected one | changes |
| `u` | unlock the selected volume | pools |
| `i` | import the selected pool | pools |
| `h` | rewind the pool to its checkpoint | pools |

## The classic bindings

[Decision 8](../../../REFACTOR-ROADMAP.md#decisions) asked whether to offer a second profile close to the `fzf`
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
| `MOD+O` | sort order | sort order | | |
| `MOD+L` | log | log | log | log |
| `MOD+X` | power | clone and promote | | |
| `MOD+C` | | clone | | |
| `MOD+N` | | new snapshot | | |
| `MOD+U` | | | inherit the default | |

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
