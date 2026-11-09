# The key map

One table, `keymap/` of `zbm-tui`, turns key presses into intents, writes the toolbar and the help view, and
generates the key binding section of `zfsbootmenu(7)` (`keymap/manual.rs`, through `cargo xtask gen-docs`). It holds
two maps until the usability sessions (UI11) choose one ([decision 8](../../../ROADMAP.md#decisions)): the
**workspace's** ([ADR 0007](../adr/0007-menu-workspace.md)), the menu's since UI10c, and the **classic** one, which
`zbm.menu=classic` keeps for one release. `zbm-tui --demo` reads keys with the first, `zbm-tui --demo --classic` with
the second.

## Keys on every keyboard

Decision 18, for both maps, built by UI15 ([ADR 0007](../adr/0007-menu-workspace.md), its UI15 note):

1. **No action needs one character only.** The arrows, `Enter`, `Esc`, `Tab` and the command line reach every action:
   each action bound to a character is also a command and an entry of the `Enter` menu of what it applies to. `j` `k`
   `h` `l` move as the arrows do; the digits only shorten the way to a tab or a start-up choice. Between start-up's
   choices, on the Boot tab, no character but `?` is bound: one typed there is the first key of a passphrase. As built:
   in each view, the palette (`:`, the command line for now) lists each action that only a character or a chord reaches
   there, as *action · keys*, where the toolbar would show its key, and choosing it does what the key does. Not listed:
   what `Enter`, an arrow, `Tab`, `Esc`, `Delete`, a page key, `Home`, `End` or a function key reaches; what the palette
   has entries of its own for (a boot, the views, the recovery shell, power); and what an `Enter` menu reaches (a
   snapshot's roll back, clone, clone and promote, duplicate and chroot, a volume's unlock; a snapshot's diff and a
   pool's rewind are listed, as their keys do more than the menus). The filter is listed in every list, the redraw and
   the digits are not, and a text field's chords have none. The digits choose a tab, but no start-up choice yet, in no
   item.
2. **No action depends on case.** A letter and its capital are one key; Shift and Caps Lock change no action. Risky
   actions open their review page instead of hiding behind Shift. As built: a character is looked up by its lower
   case, but a plain character in a text field, which is typed as is; `Ctrl` and `Alt` with a capital reach the
   table's binding before the `fzf` aliases. The command line's query ignores case, so Caps Lock changes nothing it
   finds; a list's filter keeps its smart case, as a query is text.
3. **Keys by position on the display console.** A character the map does not bind counts as the character the same key
   types on the US layout, at the same level, through the layout's `Positions` (UI1): `й` is `q`, AZERTY's `&` is `1`,
   `Ctrl` with the key labelled `с` is `Ctrl-C`. Characters the map binds count as typed. Text fields take characters
   as typed. Over a serial line or SSH only characters arrive, and rule 1 covers them. As built:
   - "Does not bind" means binds nowhere in the map, with these modifiers: a character bound in any view counts as
     typed in every view, so `de`'s `z` folds in the Snapshots tab and does nothing elsewhere, Dvorak's letters keep
     their meaning where the map binds them, and AZERTY's `!` opens the recovery tools.
   - The layout is the one the key was typed on, which the console on the display sends with each key, or the one
     `Ctrl-K` chose when it translated the key ([unlocking](unlocking.md)); the US layout's places come from the
     decoder's own tables when the image lacks `us`.
   - It applies outside text fields (the filter, the palette, a name, a passphrase), review pages and start-up's view,
     where a character is the first key of a passphrase; a keypad character counts as typed. It does not apply to the
     console's serial terminal, SSH and the kernel console (`zbm.console=kernel`, whose keymap the menu is not told):
     their characters count as typed, though on the kernel console `Ctrl-K` translates once it chose a layout
     ([unlocking](unlocking.md)).
   - A character is found by its lower case, then by itself (Caps Lock on the French digit row types `É`), at the
     level Shift gives: Shift keeps its level and Caps Lock does not change it. Of two keys that type it, the one at
     the press's level counts, else the first of the main block, so key 86 (the ISO key beside the left Shift) comes
     before the Z row. `AltGr` levels and the keys outside the main block are not found, and a composed character,
     such as a dead key's `ê`, counts as typed and does nothing. A dead key's accent comes as its own press first.
   - What every key of the main block types and does in the Boot, Snapshots and Pools tabs, plain and with Shift, is in
     a chart for each layout tested: [us](layouts/us.snap), [de](layouts/de.snap), [fr](layouts/fr.snap),
     [gb](layouts/gb.snap), [ru](layouts/ru.snap) and [us(dvorak)](layouts/us-dvorak.snap). On Latin layouts a key whose
     character the map does not bind gets the meaning of its US place: Dvorak's `.` edits as `e`, its `q` is power as
     `x`; AZERTY's `,` is `m`; `de`'s `-` is `/`. The risky ones open their review page. On a Dvorak ISO keyboard Shift
     with key 86 (`>`) edits too, as Shift with Dvorak's `.` does: the menu gets the character, not the key; in no item
     yet.
4. **The command line knows every language.** It matches the command names of the catalogue in the menu's language
   and the English names. As built: the palette finds each entry by its label in the menu's language and by its
   English one, whichever matches better; the line view takes its words in English too.

The tests: rule 2 on both tables, that no binding is a capital and that a capital does what its letter does in every
view (`keymap/case_tests.rs`), and one meaning per key, in the whole classic map and in each view of the workspace's
(`keymap/tests.rs`); rule 1's palette entries (`keymap/commands_tests.rs`, `update/commands_tests.rs`); rule 3 on
hand-made layouts (`keymap/position_tests.rs`), and on the six layouts above compiled from the host's XKB data
(`keymap/layouts_tests.rs`): every character binding of both maps typed by its label and by its place, plain, with
Caps Lock, with Shift and after `Ctrl-K`, through the model's messages; four keys through the console's cells; every
character as a terminal's bytes; the charts; and 14 wireframes replayed on five layouts
(`view/wireframes/layouts.rs`). Rule 4 in a language of look-alike letters compiled in tests only
(`update/palette_language_tests.rs`). The VM test `menu-keys` types by position on the six layouts on the console
on the display ([testing](../../luks/testing.rst)). UI14 builds the command names, their completion and the `Enter`
menus' entries of rule 1.

## The workspace's map

One meaning per key in each view, no capital letters, and the `fzf` aliases below. Only the arrows mean
two things: `Right` and `Left` go into and out of the kernels in the Boot tab, as `l` and `h` do, and from one segment
to the other in the changes.

| Where | Keys |
| --- | --- |
| Everywhere | `Enter` the primary action; `Esc` back, out of a column, closes a page; arrows and `j` `k` `h` `l` move, `h` `l` out of and into a column; `PgUp` `PgDn` `Home` `End`; `Tab` and `Shift-Tab` the next and previous tab, `1` to `4` a tab; `/` filter; `:` command line; `?` and `F1` help; `!` the recovery tools, the shell first (a shell into the selection, read-only for a snapshot, is the chroot: in the palette, and in a snapshot's `Enter` menu); `x` power; `Ctrl-L` redraw |
| Boot | `b` boot the selection; `e` edit the command line once; `d` make it the default; `s` its snapshots; `n` a new snapshot |
| Snapshots | `Enter` what to do with it, `Destroy It…` last; `b` boot it as a clone; `c` clone…; `m` clone and promote…; `y` duplicate…; `r` roll back…; `f` the files and packages changed since the selection or the one marked, or between the two marked (the older first, as ZFS took them), and restoring them; with three or more marked, or while their list is not read, `f` says so and compares nothing; `Space` marks or unmarks, as many as wanted (a set: no mark is dropped); `Delete` destroys the marked snapshots, the oldest first (in the order of their names while their list is not read), or else the selection, after its review page, which asks for the snapshot's short name, or the environment's last component for several (not in the toolbar; the palette over the tab has it too, for the selection and for the marked ones; the classic map has no destroy); `z` fold or unfold; `o` sort; `n` a new snapshot |
| Pools | `i` import…: read-only, read/write, as the host that owns it, at its checkpoint, or forced; `w` read/write…; `u` unlock…; `Enter` on a device: its kernel messages |
| Start-up | `j` `k`, the arrows and `Enter` a choice (the digits choose none yet, in no item); between choices, as the Boot tab, only `Tab` and `Shift-Tab` choose a tab (`Tab` passes over the Snapshots tab, which has no boot environment yet), the arrows, `Ctrl-P` and `Ctrl-N` move, `?` and `F1` open help; any other key, the digits and `j` `k` included, starts typing ahead of the next dialog, and the tab bar names no digit. On another tab during start-up, `1` to `4` choose a tab |
| Review page | every character, the letters, digits and `?` too, types the target's name, `Backspace` and `Ctrl-U` erase it; `Enter` the change once it is planned and named (until then the status line says what is missing), or the safer action when it has the focus; `Tab` and `Shift-Tab` move among the field, the safer action and the change; the arrows, `Ctrl-P` `Ctrl-N`, `PgUp` and `PgDn` scroll what is lost; `Esc` closes the page and sends nothing. On a page that refuses its change, which has no field, `Enter` closes it and the arrows scroll |
| Text fields | `Backspace` erases a character, `Ctrl-U` all of them; `Ctrl-R` shows what is typed (the unlock dialog); `Ctrl-K` the next keyboard layout, which then translates the unlock dialog and the keys of actions, but no other field ([unlocking](unlocking.md)). In the command line (`:`), `Tab` completes the query to the selected entry, `Ctrl-W` deletes its last word, `Ctrl-R` shows only the entries chosen before (at most 20, in memory for the menu's life), which the query filters, and `Ctrl-K` does nothing |

Against the classic map: `j` `k` `h` `l` move, so chroot leaves `j` for `!`, the kernels leave `k` for `l`, the rewind
leaves `h` for the import page, and the log leaves `l` for its tab; the pools are tab 3, so `p` is free; `Tab` goes to
the next tab, so `Space` alone marks.

**In the code** (`keymap/workspace.rs`): its bindings come before the classic map's, whose bindings of the same
keys in a view where both work, and `p`, it drops: `j` `k` `h` move, `l` and `Right` go into the kernels and `Left`
out of them, `1` to `4`, `Tab` and `Shift-Tab` choose the tab; the changes keep the classic map's `Left` and
`Right`. `WORKSPACE_LAST` binds `z` and `Delete` (`Intent::Destroy`, `Footer::Hidden`) on the Snapshots tab alone.
`Map::is_bound` narrows start-up's view (`Context::Startup`) to `Tab`, `Shift-Tab`, the moves up and down (the arrows,
`Ctrl-P`, `Ctrl-N`) and help, with no character but `?`, so that a key typed ahead of a passphrase's dialog waits for
it (`StartupView`'s `typed_ahead`); the tab bar takes its keys from `Map::binding_in` there, so it names none of the
digits.
The review page of a change is its own context, `Context::Review`, while it shows its field (a page that refuses its
change is `Context::Choose`): `input` reads its keys as a text field's, `Scope::Views` leaves it out, as it does text
fields, so no letter or `Home` `End` acts there, and `WORKSPACE` binds `Tab` and `Shift-Tab` in it alone; `PgUp`,
`PgDn` and the arrows reach it as they reach every view. The classic map has no review page.
The other keys are the classic map's: `!` opens the recovery tools, the shell first, and the pools keep `i`, `u` and
`w`; the rewind, which the classic map has on `h`, is among the imports of `i`.

**In the code** of decision 18, for both maps: `keymap::input` folds a capital to its letter (rule 2);
`keymap/position.rs`'s `input_on` asks it first and then looks a character bound nowhere up by its place (rule 3), with
the layout `Model::typing` gives (`model/typing.rs`: the one the key was typed on, from `TypedOn`, or the one `Ctrl-K`
translated it into), and `update/mod.rs` is its only caller, after `Ctrl-K`'s translation (`update/unlock.rs`,
`Model::translation`). `keymap/commands.rs` lists what only a character or a chord reaches in a view, which the palette
shows (rule 1; `ENTER_REACHES` names what an `Enter` menu reaches instead), and `filter::rank_named` ranks a palette
entry by its label and its English name, which the palette builds a second time with `text::in_language` when the menu's
language is not English (rule 4).

## The classic map

The map of the classic menu, which `zbm.menu=classic` keeps for one release, and which the sessions compare with the
workspace's.

### Its rules

1. **One meaning per key.** A key does the same thing, or nothing, in every view. Where the `fzf` menu gave `MOD+D`
   three meanings, the new map gives each action its own key. The command line, which is no view, has keys of its own
   in both maps (`keymap/command_line.rs`, which `Map::table` adds to each): there `Tab` completes rather than marks,
   and `Ctrl-R` shows the commands chosen before rather than what is typed.
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
   ([unlocking](unlocking.md)). Not in text fields: the filter, the palette, a name and a typed confirmation take
   characters as typed, and only the unlock dialog's are translated. Rules 2 and 3 of decision 18, above, hold for
   this map too.

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
| `Ctrl-K` | in a text field but the command line: the next keyboard layout ([unlocking](unlocking.md)) |
| `Ctrl-R` | in the unlock dialog: show what is typed, or hide it (not during start-up); in the command line: show only the commands chosen before, or all again |
| `Tab` | in the command line: complete the query to the selected command |
| `Ctrl-W` | in the command line: delete the last word of the query |

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
| `f` | the changes since the snapshot or the one marked, or between the two marked, the older first (two taken in the same second as ZFS took them, whatever the sort); none with three or more marked, or while their list is not read, which the status line says | snapshots |
| `Space`, `Tab` | mark or unmark the selection (snapshots for `f`, as many as wanted, none dropped; files to restore) | snapshots, changes |
| `Enter` | restore the marked files, or the selected one; on a package, show its files | changes |
| `Left`, `Right` | the other segment: files or packages | changes |
| `u` | unlock the selected volume | pools |
| `i` | import the selected pool | pools |
| `h` | rewind the pool to its checkpoint | pools |

## The `fzf` aliases

[Decision 8](../../../ROADMAP.md#decisions) asked whether to offer a second profile close to the `fzf`
menu. The proposal: no profile to choose, but every `MOD+key` binding of the `fzf` menu (`MOD` is `Alt` or `Ctrl`)
keeps working, with its old meaning in its old view, as an alias that the help view of the workspace lists under
*fzf aliases*, and that of the classic menu under *classic bindings*:

| Alias | Main | Snapshots | Kernels | Pools |
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
back, `Ctrl-L` redraw, `Ctrl-N` and `Ctrl-P` down and up): there the new meaning wins, and the alias works
with `Alt` only. `Ctrl-S` may not reach the menu on serial lines (flow control), and a terminal (a serial line, SSH)
sends `Ctrl-J` as a newline, which the menu reads as `Enter`: their `Alt` forms work, and `zfsbootmenu(7)` says so.
The sessions confirm or reject this before R5.

## The toolbar and the help view

The toolbar (the last line, [apple-hig.md](apple-hig.md) rule 9) shows, for the focused element, the bindings marked
for it in the order of the table, as `Enter Boot`: at most five, whole ones only, as many as fit, then `? More` on the
right, which opens help. The help view lists every binding of the view it was opened from, then the global ones, then
the `fzf` aliases. In alerts, action sheets and review pages `Esc` reads *Cancel*; on a review page `Enter` reads
*Confirm*, or *Close* on one that refuses its change, `Tab` *Next*, and the arrows *Scroll*.
