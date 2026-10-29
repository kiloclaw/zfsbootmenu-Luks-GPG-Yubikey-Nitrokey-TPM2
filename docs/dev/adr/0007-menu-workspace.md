# ADR 0007: The menu as a workspace, and keys that work on every keyboard

- **Status:** proposed, 29 October 2026
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
2. **Drill-down columns.** In a tab, columns go from the general to the particular (pool › environment › kernels or
   snapshots); moving in and out keeps the selection of each column, and the path is shown above them.
3. **An inspector.** The selection's details, the checks of whether it will boot, and the next steps with their keys,
   on the right or below; long values wrap instead of being cut.
4. **What needs attention, in view.** A sidebar lists degraded pools, missing devices, foreign pools and failing
   checks, each with the key that leads to it; the boot history (#9) shows which kernel last reached a login.
5. **A command line with a dry run (UI14).** `:` opens a line with fuzzy completion, grouped as actions, items and help,
   each with its key and what it costs, and a history. Before anything runs it shows the plan: the steps, the command
   each runs, the effect, the undo, and what the plan needs (a pool writable, a key). The plan is PM8's plan of steps,
   whose adapters describe their commands (SB2, SB3), so the dry run is what will run.
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
[keymap.md](../menu-design/keymap.md#the-classic-bindings).

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
  menu entry; no key has two meanings in two tabs.
- UI15's tests type every binding on `us`, `de`, `fr`, `uk`, `ru` and Dvorak: by position through the console's cell
  surface and its layout, and by character on the terminal backend, and compare the intents with the US layout's.
- The command line's matcher test finds every command by its name in each language of the catalogue and in English.
- Wireframe snapshots of the workspace at 120×40, 80×24 in ASCII and 52×20 in the pseudo-language; the
  10,000-snapshot test (decision 7) on the timeline.
- The usability sessions (UI11): at least five users, one on a serial console and one with a non-US layout, on the
  prototype, against the task budgets of [users-and-tasks.md](../menu-design/users-and-tasks.md).

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
