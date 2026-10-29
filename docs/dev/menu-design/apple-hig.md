# Apple's design rules, adapted to the console

The menu follows Apple's [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines)
(HIG), translated to what a text console can draw: character cells, 16 colours or none, and only the glyphs of the
console font. This page lists each rule, how it is adapted, and where the prototype applies it. The other pages of
the design follow it; where they differ, this page wins.

## The principles

| HIG principle | On the console |
| --- | --- |
| **Clarity**: text is legible, icons precise, purpose obvious | Words before symbols; every state is written out; one action per key, named in the toolbar |
| **Deference**: the interface helps people understand content and never competes with it | Little chrome: thin borders, no inverse bars, colour only where it means something; the boot environments fill the screen |
| **Depth**: layers and motion convey hierarchy | A navigation bar says where you are and where `Esc` goes; dialogs float over a dimmed view |
| **Consistency**: familiar standards and paradigms | The same layout, keys and words on every view: `Enter` does the default, `Esc` goes back or cancels, `/` searches |
| **Feedback**: every action is acknowledged | A countdown banner, a start-up checklist, and a notification after each action |
| **Forgiveness**: actions are reversible where they can be | A safety snapshot before a rollback, and the confirmation says how to undo |

## The rules

### 1. Navigation bar

Line 1 of every view, as a navigation bar: the back label on the left (`‹ Boot Environments`, the view `Esc` returns
to; none on the main view), the title in the middle in bold, and the status of the machine on the right. No inverse
bar (deference). On narrow screens the back label shortens to `‹`, then the status takes its short forms.

In the workspace ([ADR 0007](../adr/0007-menu-workspace.md)) the line is a **tab bar** instead, as a macOS window's
toolbar of segments: the tabs with their digits, the current one in brackets, and the same status on the right. A tab
replaces the back label: `Esc` still goes back one step within the tab.

### 2. Banners for what happens without asking

The countdown is a one-line banner under the navigation bar, in the tint colour: `Booting zroot/ROOT/void in 10 s.
Press any key to choose another.` It disappears once a key stops it, giving its line back to the list.

### 3. Grouped lists

Lists are *inset grouped* lists: a box with thin square borders and one column of margin, its section header in the
top border in upper case and the secondary colour (`BOOT ENVIRONMENTS`), and the count on the right of the same border
(`4`, or `2 of 4` while searching). Rows are *value* cells: the name on the left, secondary facts right-aligned in the
secondary colour.

The selection is a full-width highlight in the tint colour, and a `▶` in front of the row (`>` in ASCII), so it shows
without colour too. Marked rows have a `•`.

Apple marks the chosen option with a check mark. The console fonts have none, and a `√` reads as a square root, so the
default environment and kernel carry a `default` label in green instead, which also reads as text.

### 4. Inspector

The detail pane is an inspector, like the Get Info window of macOS: labels right-aligned in the secondary colour, values
left-aligned after them, and facts on one line joined by ` · `. On wide screens it is a split view to the right of the
list (macOS sidebar and detail); on narrow ones it sits under the list.

### 5. Search field

`/` opens a search field at the top of the list, with `Esc Cancel` on its right, as the search bar of iOS. The matched
characters are highlighted, and the count in the header becomes `2 of 4`.

### 6. Alerts

Confirmations, errors and unlock prompts are *alerts*: a narrow box (at most 60 columns) over the dimmed view, with
the title bold and centred, the message left-aligned (centred paragraphs are hard to read in a monospaced font), and
buttons at the bottom right, as in macOS.

- Buttons are drawn `[ Roll Back ]`, labelled with a verb in title case, never *Yes*/*No*. *OK* is used only to
  acknowledge an error.
- The default button, which `Enter` presses, is filled in the tint colour. `Esc` always presses **Cancel** (or
  **Skip** when unlocking).
- A destructive button is red. HIG says a destructive action must not be the default. The menu asks for the target's
  name to be typed instead: the destructive button stays disabled (in the secondary colour) until the name is right,
  and only then becomes the default. Typing the name is the deliberate act that HIG's default rule protects.
- The message says what happens, what is lost, and how to undo it.

### 7. Action sheets

The actions on one item (a snapshot, a pool, a pool that did not import, power) are an *action sheet*: the item as the
title, a short message, one row per action, and **Cancel** at the bottom, set apart by a separator. Destructive actions
are red; actions that ask for more input before acting end with `…`, as in macOS menus.

### 8. Modality

Only the alerts and action sheets are modal. While one is open the view behind it is dimmed, the toolbar shows the
keys of the dialog, and `Esc` closes it without doing anything.

### 9. Toolbar

The last line is a toolbar: at most five actions of the focused element, the most important first, then `? More` on the
right, which opens help with every key. Each action is its key, in the tint colour, and a title-case label:
`Enter Boot   k Kernels   s Snapshots   e Edit   d Default`.

### 10. Feedback

- Start-up is a checklist with the step count in its header (`3 of 5`): `√` done, `…` running, `×` failed (with the
  reason), `–` skipped.
- After every action a notification line says how it went, with a glyph and a word: `√ Done`, `! Warning`,
  `× Failed`. It stays until the next key.
- Nothing moves but the countdown and progress (reduced motion by default).

### 11. Empty states

A list with nothing in it says why and what to do, centred in the box: a title (`No Boot Environments`), a sentence
(`No imported pool has a boot environment.`) and the keys that help (`p Pools   ! Recovery`). A search without
matches says so and how to clear it.

### 12. Typography and writing

- Three levels of text: titles in bold, body text, and secondary text in dark grey. Section headers are upper case
  and secondary.
- Titles, buttons, actions and toolbar labels are in title case (`Roll Back`, `Clone and Promote…`); messages are
  sentences.
- Plain words where they exist, ZFS terms where they are the user's words (*snapshot*, *pool*, *boot environment*).
- Units always: `12 h ago`, `1.1 GiB`, `10 s`.

### 13. Colour

The 16 basic colours only, each with one meaning:

| Colour | Meaning |
| --- | --- |
| cyan (tint) | interactive: the selection, keys, the back label, the default button, the banner |
| red | destructive actions and failures |
| yellow | warnings: `DEGRADED`, read-only |
| green | success and `default` |
| magenta | locked |
| dark grey | secondary text, section headers, borders |

Colour never carries meaning alone (rules 3, 10, and the monochrome test of the wireframes).

### 14. Layout by size class

| Size class | Screen | Layout |
| --- | --- | --- |
| compact | below 80×25, down to 52×20 | one column, the inspector under the list, short forms |
| regular | 80×25 to 99 columns | one column, the inspector under the list |
| wide | 100 columns and more | split view: list on the left, inspector on the right |

Margins: one column inside every box; rows never touch the borders.

### 15. Accessibility

- Every glyph is in the console fonts of the image (Terminus, 512 glyphs). A test reads the fonts' Unicode tables and
  fails if a wireframe uses a glyph they lack. There are no rounded corners, check marks or filled circles, which those
  fonts do not have.
- Monochrome and ASCII terminals get the same information (the selection marker, glyphs with words).
- Large text comes from the font, not the layout: `zbm.autosize` sizes the font to the display (HiDPI), and the layout
  works down to 52×20.
