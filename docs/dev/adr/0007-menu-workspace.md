# ADR 0007: The menu as a workspace, and keys that work on every keyboard

- **Status:** accepted, 29 October 2026 (proposed the same day)
- **Decides:** decision 18 of [ROADMAP.md](../../../ROADMAP.md#decisions) (keys that work on every keyboard), and the
  design of UI13; proposes the key map of decision 8, which the usability sessions (UI11) decide
- **Governs:** UI10, UI11, UI14, UI15; the key map of `zbm-tui` (`menu/keymap.rs`), UI6's `Key` message

## Context

The menu is a set of separate screens: the main list, then the kernels, the snapshots or the pools, each a view of its
own that `Esc` leaves, so a selection loses its context on the way. Problems found at start-up stop it with a prompt
or decide silently. The key map ([keymap.md](../menu-design/keymap.md)) gives one letter to each action, with `j` for
chroot, `k` for the kernels, `h` for rewinding a pool and `l` for the log.

Eight mockups of 27 October 2026 redesign the menu as a workspace, for people who know ZFS: the boot workspace, the
kernels as drill-down columns, the snapshot timeline, the command line with a dry run, the rollback review, pools with
their vdevs and LUKS volumes, the start-up timeline with an import problem, and the same workspace at 80×24 in ASCII
over a serial line. They move with `j` `k` `h` `l`, which the current map gives to actions, and two of their actions
are capitals (`I` forces an import, `R` rewinds a pool).

Keys also fail on keyboards that are not US-like:

- on a Cyrillic, Greek or Hebrew layout, the letter keys type no letter the map binds, so no action has a key;
- on AZERTY the digit row types `&é"'(` without Shift, so a digit binding needs Shift;
- with Caps Lock on, or a layout that puts a letter on Shift, a map that binds `r` and `R` to two actions does the one
  the user did not mean;
- over a serial line or SSH only characters arrive; the menu never learns which key was pressed.

On the display console the menu knows the layout typing uses, and UI1 gave it each layout's key positions
(`zbm_display::layout::Positions`, compiled from the image's XKB keymaps): what a key types plain and with Shift.

## Decision

**The menu is one workspace of tabs and columns, and its keys follow four rules that hold on every keyboard.**

### The workspace

1. **Tabs instead of screens.** Four numbered tabs, Boot, Snapshots, Pools and Log, with the header naming the host,
   the ZFS version, what is degraded, and the time. The start-up timeline is the Boot tab until start-up ends.
   (Superseded in part by [ADR 0016](0016-menu-workspace-built.md), decision 11: the tab bar shows the worst pool,
   the open volumes and what needs attention, the host and the time from 120 columns, and the ZFS version moves to the
   help view.)
2. **Drill-down columns.** In a tab, columns go from the general to the particular (pool › environment › kernels or
   snapshots); moving in and out keeps the selection of each column, and the path is shown above them.
3. **An inspector.** The selection's details, the checks of whether it will boot, and the next steps with their keys,
   on the right or below; long values wrap instead of being cut.
4. **What needs attention, in view.** A sidebar lists degraded pools, missing devices, foreign pools and failing
   checks, each with the key that leads to it; the boot history (#9) shows which kernel last reached a login.
5. **A command line with a dry run (UI14).** `:` opens a line with fuzzy completion, grouped as actions, items and help,
   each with its key and what it costs, and a history. Before any change runs it shows the plan: the steps, the command
   each runs, the effect, the undo, and what the plan needs (a pool writable, a key). The plan is PM8's plan of steps,
   whose adapters describe their commands (SB2, SB3), so the dry run is what will run. (Narrowed on 9 November 2026,
   UI14: "before anything runs" became "before any change runs"; a boot, a chroot, a re-seal and the reset of boot
   tries run without a dry run, and their plan is ROADMAP.md's UI16.)
6. **Review pages for what cannot be undone.** A rollback, a forced import, a destroy or a rewind opens a whole page:
   what is lost, what is kept, the steps with their commands, the undo, a safer alternative, and a confirmation by
   typing the target's name.
7. **Start-up decides nothing silently.** A problem stops one step of the timeline and offers choices with the
   commands they run, and a safe default that runs after a time-out, which any key holds.
8. **The same workspace at every size.** At 120×40 the sidebar, the list and the inspector side by side; at 80×24 the
   attention as one line above the list and the inspector below it, in ASCII and reverse video without colour when
   the terminal has none; at 52×20 one column at a time, the attention as a count in the header.

### The keys (decision 18)

1. **No action needs one character only.** The arrows, `Enter`, `Esc`, `Tab` and the command line reach every action:
   each action bound to a character is also a command of the command line and an entry of the `Enter` menu of what it
   applies to. `j` `k` `h` `l` move as the arrows do; the digits only shorten the way to a tab or a start-up choice.
   (Amended by the maintainer on 9 November 2026, UI14: where `Enter` boots, on an environment or a kernel, the other
   actions are reached through `:` and the inspector's next steps, not an `Enter` menu; what applies to the view
   rather than an item, such as the order, the folds, the marks and the filter, is a command of the command line
   only. The classic map keeps the palette's reach until RL2.)
2. **No action depends on case.** A letter and its capital are one key: Shift and Caps Lock change no action. Risky
   actions open their review page instead of hiding behind Shift.
3. **Keys by position on the display console.** When a key types a character the map does not bind, the menu looks it
   up in the `Positions` of the layout typing uses, and the character that key types on the US layout, at the same
   level, counts. So `й` is `q`, AZERTY's `&` is `1`, and `Ctrl` with the key labelled `с` is `Ctrl-C`. Characters
   the map binds count as typed, so a layout's own `/` and `:` work where its labels say. Text fields (the filter,
   the command line, a passphrase, a typed confirmation) take characters as typed; the unlock dialog keeps UI1's
   translation. Over a serial line or SSH only characters arrive, and rule 1 covers them.
4. **The command line knows every language.** It matches the command names of the catalogue in the menu's language
   (Fluent, [text.md](../menu-design/text.md)) and the English names, whichever is typed.

### The key map proposed (decision 8)

One table, `keymap.rs`, stays the source of the toolbar, the help view and the man page. One meaning per key in the
whole workspace; no capital letters; the `MOD+key` aliases of the `fzf` menu stay as in
[keymap.md](../menu-design/keymap.md#the-fzf-aliases).

| Where | Keys |
| --- | --- |
| Everywhere | `Enter` the primary action; `Esc` back, out of a column, closes a page; arrows and `j` `k` `h` `l` move, `h` `l` out of and into a column; `PgUp` `PgDn` `Home` `End`; `Tab` and `Shift-Tab` the next and previous tab, `1` to `4` a tab; `/` filter; `:` command line; `?` and `F1` help; `!` a shell: into the selection (read-only for a snapshot), or the recovery tools; `x` power; `Ctrl-L` redraw |
| Boot | `b` boot the selection; `e` edit the command line once; `d` make it the default; `s` its snapshots; `n` a new snapshot |
| Snapshots | `Enter` what to do with it; `b` boot it as a clone; `c` clone…; `m` clone and promote…; `y` duplicate…; `r` roll back…; `f` the files and packages changed since, or between the marked ones, and restoring them; `Space` mark; `z` fold or unfold; `o` sort; `n` a new snapshot |
| Pools | `i` import…: read-only, read/write, as the host that owns it, at its checkpoint, or forced; `w` read/write…; `u` unlock…; `Enter` on a device: its kernel messages |
| Start-up | `1` to `9` a choice, as do `j` `k` and `Enter` |
| Text fields | `Tab` completes; `Ctrl-W` deletes a word; `Ctrl-R` searches the history (the command line) or shows what is typed (the unlock dialog); `Ctrl-K` the next keyboard layout |

Against the mockups: `l` moves into a column, so the log is the Log tab (`4`) and an entry's kernel messages are its
`Enter`; `i` imports, so looking inside a snapshot is `!`; restoring a file is in the changes (`f`), so `u` only
unlocks; the forced import and the rewind are choices of the import page, and commands (`:import -f`, `:rewind`).

## Consequences

- A selection keeps its context: drilling into kernels or snapshots and back loses nothing, and the inspector says
  what booting it would do before it is booted.
- The current map changes where the workspace needs its letters: chroot leaves `j` for `!`, the kernels leave `k` for
  `l`, the rewind leaves `h` for the import page, and the log leaves `l` for its tab. The sessions (UI11) compare this
  map with the current one, which is why decision 8 stays open after this record.
- Users of every layout reach every action on the display console without switching to a US layout; over a serial
  line, the arrows and the command line do.
- Actions are plans (PM8) before they are keys: the command line, the dry run, the `Enter` menus and the review pages
  are views of the same plan, and a key only names one.
- `KeyPress` gains nothing: rule 3 needs the layout typing uses, which the console reports (PM7's `Status`, then
  UI6's `Key` message), and its `Positions`, which the image already has.
- `docs/dev/menu-design/` is rewritten from the mockups, and the wireframes grow the workspace's views at the three
  sizes; `zbm-tui --demo` shows the Boot tab and the serial form for the sessions (UI13).

## Alternatives considered

- **Keep the separate screens and the current map.** It works, but the context is lost at every screen, and the
  mockups' movement keys cannot be added without moving four actions.
- **Capitals for the risky variants** (`I`, `R`, as in the mockups, or `vi`'s `D`). One key fewer, but Caps Lock or a
  layout's Shift then runs another action, and the review page is the guard that matters.
- **Function keys for actions,** as Midnight Commander does. Serial terminals and some keyboards do not send them; they
  stay extras, never the only way.
- **Positions everywhere.** Terminals send characters, so it can only hold on the display console. Guessing a serial
  user's layout from the image's (`zbm.keymap`) and translating back would turn a character that does nothing into an
  action, and `b` boots.
- **A profile to choose between the old and the new map.** Two maps to test and document; the aliases keep old habits
  without one.

## How it is checked

- A test over `KEYMAP`: no binding on a capital; every action bound to a character has a command name and an `Enter`
  menu entry; no key has two meanings in two tabs. (Met for the workspace by UI14, with rule 1 as amended: an
  `Enter`-menu entry where `Enter` opens a menu, the command line where it boots, a command for the view. The classic
  map keeps the palette's reach until RL2.)
- UI15's tests type every binding on `us`, `de`, `fr`, `gb`, `ru` and Dvorak: by position through the console's cell
  surface and its layout, and by character on the terminal backend, and compare the intents with the US layout's.
- The command line's matcher test finds every command by its name in each language of the catalogue and in English.
- Wireframe snapshots of the workspace at 120×40, 80×24 in ASCII and 52×20 in the pseudo-language; the
  10,000-snapshot test (decision 7) on the timeline.
- The usability sessions (UI11): at least five users, one on a serial console and one with a non-US layout, on the
  prototype, against the task budgets of [users-and-tasks.md](../menu-design/users-and-tasks.md).

## Notes from the implementation

- **UI13** (29 October 2026): `zbm-tui --demo --workspace` is the prototype for the sessions. The key map has two
  tables, `Map::Classic` and `Map::Workspace` (`keymap/workspace.rs`): the workspace's bindings come first, and the
  current map's bindings of the same keys, and `p`, are dropped; a test checks one meaning per key and no capital in
  both. It has `j` `k` `h` `l`, `l` into the kernels, the digits, `Tab` and `Shift-Tab`; the other keys are the
  current map's until UI10. The Boot tab draws the tab bar, the sidebar (pools, volumes, what needs attention), the
  environments and the inspector with its next steps from 110 columns; the attention line above the list and the
  inspector below it at 80×24, in ASCII over a serial line; the list and the inspector at 52×20, with the attention
  counted in the tab bar. The other tabs show the current views. The measures of the core tasks are the same in both
  maps. The boot history, the grouping by pool, the timeline, the command line with its dry run, the review pages and
  rule 3 are UI10, UI14 and UI15.
- **UI15** (8 November 2026, UI15-1 to UI15-8): the four rules of decision 18, in both maps; how each is built, and what
  it leaves, is in [keymap.md](../menu-design/keymap.md), *Keys on every keyboard* and *In the code*. The classic map
  gets rules 2 and 3 through the shared lookup, `keymap::input`, and rule 1's entries through the shared palette, and no
  new view ([ADR 0016](0016-menu-workspace-built.md), point 10): under `zbm.menu=classic` a volume's row lists the
  import, read/write and rewind in the palette, as its toolbar does, though their keys do nothing there, until the
  classic map goes (RL2). The maintainer answered the plan's six questions on 8 November 2026, each as proposed: UI15
  builds rule 1's palette entries, for reach only, and rule 4's English names in today's palette, and UI14 the command
  names, their grouping and costs, the `Enter`-menu entries and a query that Caps Lock does not change; a character the
  map does not bind is one it binds nowhere, so `de`'s `z` stays the fold and does nothing outside the Snapshots tab;
  once `Ctrl-K` chose a layout, its translation runs in the unlock dialog and for action keys only, never in the filter,
  the palette or a typed confirmation, as rule 3 says, and Super+Space alone translates nothing; rule 3 runs on the
  console on the display only, as the kernel console (`zbm.console=kernel`) does not tell the menu its keymap: there
  characters count as typed until `Ctrl-K` chose a layout, which then translates as UI1 did, without rule 3; the two
  wrong layout names are fixed, so a keyboard with a layout of its own sends its keys under its own name, and the serial
  terminal the console serves (UI9) its characters under none, which nothing translates, as over SSH; and the VM test is
  `menu-keys`, in the `menu` group, one boot with Super+Space between the layouts, whose run is the maintainer's. Of
  "How it is checked", the first check is half met, as the `Enter`-menu entries are UI14's; the second types every
  character binding of both maps on `us`, `de`, `fr`, `gb` (written "uk" here before: XKB has no `uk`), `ru` and
  `us(dvorak)`, compiled from the host's XKB data, through the model's messages, four keys through the console's cells,
  and every character as a terminal's bytes; the third runs in a language of look-alike letters compiled in tests only.
  Left, in no item yet: the unlock dialog names a new layout only from the first key typed on it, as the console sends
  nothing for Super+Space; after `Ctrl-K`, a character the typed-on layout gives with `AltGr` is looked up in the chosen
  layout, where it may sit on a key of the main block; on a Dvorak ISO keyboard Shift with key 86 (`>`) edits, as the
  menu gets the character, not the key; rule 3 on the demo's display (`zbm-tui --drm`); a VM test of the keys over SSH.
  Seen in the runs, older than UI15 and in no item: `zbm-sys`'s test of a listener passed on failed once with
  `AddrInUse`; `zbm`'s test of a console that ends and starts again failed once, and ten supervisor tests with it
  through a poisoned lock; and `zbm-app`'s `keyholder.rs:84`, the empty default `Keyholder::kill`, is covered only when
  a cancellation wins a race.
- **UI14** (11 November 2026, UI14-1 to UI14-9): point 5, and what decision 18 left to it. The command line's keys
  are in [keymap.md](../menu-design/keymap.md), its views in
  [information-architecture.md](../menu-design/information-architecture.md); the edit saved for every boot is
  [ADR 0016](0016-menu-workspace-built.md)'s point 18, and the lock marks are [ADR 0010](0010-locked-menu.md)'s.
  - **Point 5** is built as ADR 0016's point 12 says, in the workspace: a change chosen on the command line is first
    sent as `Action::Plan`, and its plan shows on the review page without the field. `Model::dry_run` marks the
    command's choice and lasts while the dialogs it opened, a name or a choice, are open; `update/dry_run.rs` then
    sends the plan in place of the change. The mark ends there, when no dialog is left, when a review page opens, or
    when the machine opens a dialog of its own, so no later key is caught. The page has a title for each kind of
    change, the steps with the command each runs, quoted as a shell reads it (`zbm_platform::shell::word`), what is
    lost or that nothing is, the undo `zbm-app` gives each change (`plan/review/undo.rs`), and "Nothing has changed
    yet: Enter runs it, Esc leaves it". `Enter` runs it once. What a plan needs shows in its steps (a pool imported
    read/write first) and its costs (a key). Over SSH a plan is asked for as on the console; under the lock it is
    free, and the PIN is asked when the change runs. A view's key acts as before, the review pages keep their typed
    name, and the classic map shows no dry run (ADR 0016, point 10). A boot, a chroot, a re-seal and the reset of
    boot tries have no plan, so point 5 says "before any change runs".
  - **The sections:** Recent, the history; Actions, those of each environment, pool and volume and of the view under
    the line, with one entry for each action of the Snapshots tab and one `Boot a clone of …` for each snapshot read;
    Items, an environment, a pool or a volume selected on its tab, the views, and each environment's kernels and
    snapshots; Help, each view's keys and the command line's own, which `F1` opens there too, `Esc` going back to the
    query. The entries are ranked together, then grouped by section in that order; the headings are drawn and said
    but never chosen.
  - **Names in two languages:** an entry is found by its label and by its command's name and target
    (`rollback zroot/ROOT/void@pre-upgrade`), the `command-*` messages of the catalogue, in the menu's language and in
    English, through `filter::rank_named`. The row shows the label, `Tab` completes to the name, and the command
    line's help and the section "Command Names" of `zfsbootmenu(7)` list the names.
  - **Costs:** on the right of each row, muted, the keys that do its action in the view under the line, from
    `update/action_of.rs`, the one source of what a key does, which the views' handlers follow; then what it costs:
    `PIN` (an action the locked menu keeps, or the shell), `read/write` (a pool imported read-only that the action
    writes), `key` (an environment whose key is not loaded, its boot and chroot among them) and `review` (a page that
    asks for a typed name). A narrow row cuts the keys and the other costs before the `PIN`. Time is no cost, and
    neither is leaving the menu.
  - **The history** keeps at most 20 entries chosen, newest first, and no query or line typed. It lives in memory for
    the menu's life: nothing is written to disk.
  - **The case:** the command line's query ignores case, so Caps Lock changes nothing it finds. `Tab`, `Ctrl-W` and
    `Ctrl-R` are keys of its own context, `Context::Command`, in both maps. A list's filter keeps its smart case. The
    line view takes whole lines, so these three keys have no form there.
  - **The `Enter` menus:** a snapshot's sheet, a pool's details and a volume's unlock reach what their keys do
    (`keymap/commands.rs`, `Reach` and `ENTER_REACHES`). Where `Enter` boots, the inspector's next steps list an
    environment's `l`, `s`, `e` and `d`, and a kernel's `e`, `d`, `/` and `:`; the command line has the rest. The
    entry that drops the line edited for this boot is listed only when one was edited, so `MOD+T` (and the classic
    map's `t`) with none does nothing and says nothing, where it said the line was reverted before
    (`the_revert_key_says_nothing_when_no_command_line_was_edited`).
  - **The answers:** the maintainer answered the plan's eight questions on 9 November 2026, each as recommended. A
    snapshot keeps one entry, `Boot a clone of …`, and no item. Rule 1 is amended as above. The lock mark is the
    translatable word `PIN` after the label. An inherited command line opens in the editor with `%{parent}` expanded
    once. The snapshot of the machine carries the seconds the PIN's proof has left, and the menu counts them down and
    marks again at 0. UI14 stays whole, at L. There is no dry run of a boot: point 5 is narrowed as above, and the
    boot's plan is a row of its own, UI16. The history lives in memory.
  - **The checks:** of "How it is checked", the first is met for the workspace
    (`every_action_bound_to_a_character_has_an_entry_in_the_enter_menu_of_what_it_applies_to`), and the third covers
    the command names. B5 holds with the command line: no key near a frame, the worst 7.50 ms, a dry run's two keys
    9.09 ms ([testing](../../luks/testing.rst), *Benchmarks*). The VM runs, the group `menu` with the saved command
    line and the dry runs and `integrity-locked` with its mark, are the maintainer's.
  - **Found on the way:** an import as the last host lists the step that takes that host's ID, which the machine
    takes only when the plain import fails, so the step reads "if the import fails"; 18 wireframes changed in UI14-6
    from its five new entries alone (`workspace-commands-*`, `workspace-palette-*`, `workspace-commands-locked-*`),
    and the classic ones but `palette-*`, which gained the headings and the costs, are unchanged; the `menu_names`
    fuzz target's runs take about 2 s each, against 1.2 s before, under its time-out of 10 s.

## Questions for the maintainer

1. **Case.** Proposed: a capital is its letter (Caps Lock harmless). The alternative leaves capitals unbound, so Caps
   Lock silences the letters.
2. **One meaning per key in the whole workspace.** Proposed, as today, with the moves above (`i` imports, `!` looks
   inside, `f` restores, `u` unlocks, the log is a tab). The alternative is one meaning per tab, which lets `i` look
   inside a snapshot and import a pool, but makes a letter's meaning depend on the tab.
3. **`Tab`.** Proposed: the next tab, as the roadmap says; the Pools mockup gave it to the next panel, and the current
   map to marking a snapshot, which keeps `Space`.
4. **Serial and SSH users on other layouts.** Proposed: characters only, with the arrows and the command line (rule 1).
   The alternative translates unbound characters through the image's layout, a guess that can run the wrong action.
5. **The start-up view.** Proposed: the Boot tab until start-up ends, so the Log tab and the command line work during
   it. The alternative keeps it a screen of its own, as today.

## Answers of the maintainer

Accepted as proposed, on 29 October 2026:

1. **Case:** a capital is its letter; Caps Lock changes no action.
2. **One meaning per key in the whole workspace,** with the moves above: `i` imports, `!` looks inside, `f` restores,
   `u` unlocks, and the log is a tab.
3. **`Tab`:** the next tab; `Space` marks.
4. **Serial and SSH users on other layouts:** characters only, with the arrows and the command line.
5. **The start-up view:** the Boot tab until start-up ends.

Decision 8, the key map itself, stays open until the usability sessions (UI11) compare it with the current map.
