# The visual system

The rules behind it are Apple's, adapted to the console: [apple-hig.md](apple-hig.md).

The menu runs on the Linux console, serial consoles, and terminal emulators from the 1980s to now. It must read well
on all of them, so the visual system is small: a handful of roles, each drawn in three ways, and borders in two.

## Roles

Every styled piece of text has a role; views never pick colours. The roles and their styles are in `theme.rs` of
`zbm-tui`.

| Role | Used for | 16 colours | Monochrome |
| --- | --- | --- | --- |
| `Text` | ordinary text | default | default |
| `Muted` | secondary facts: sizes, ages, origins | dark grey | default |
| `Title` | view and pane titles | bold | bold |
| `Selected` | the selected line of a list | black on cyan | reverse |
| `Match` | characters the filter matched | yellow, bold | bold, underlined |
| `Default` | the default environment or kernel | green, bold | bold |
| `Good` | `ONLINE`, `open`, `done` | green | default |
| `Warning` | `DEGRADED`, read-only, warnings | yellow | bold |
| `Danger` | `FAULTED`, failures, destructive actions | red, bold | bold, reverse |
| `Locked` | encrypted and not unlocked | magenta | underlined |
| `Key` | key names in the toolbar and help | cyan, bold | bold |
| `Border` | borders and separators | dark grey | default |
| `StatusBar` | unused since the navigation bar replaced the status bar | black on white | reverse |
| `Accent` | the tint colour: the back label, the countdown banner | cyan, bold | bold |
| `Header` | section headers, in upper case | dark grey | default |

Only the eight basic colours and their bright forms are used, as the Linux console has no more, and they map to every
terminal's palette. Backgrounds are left to the terminal except for the selection and the default button.

**Meaning is never carried by colour alone.** Every role that means something has a word or mark too: `default`,
`ONLINE`/`DEGRADED`, `locked`, `failed`, `!` with the count, the selection's `▶`. A monochrome screenshot must tell the same
story; the snapshot tests compare the monochrome text with the coloured one.

## Three ways to draw

- **Colour**: the 16-colour styles above.
- **Monochrome**: bold, underlined and reverse only; for terminals without colour and for `NO_COLOR`.
- **ASCII**: monochrome styles, and borders drawn with `+`, `-` and `|`; for terminals that cannot draw line
  characters (`TERM=vt100`, `vt102`, `vt220`, `ansi`, and any terminal whose locale is not UTF-8). Icons (`🔒`) become
  words.

## Glyphs

Only glyphs that every console font of the image has (the six Terminus fonts, 512 glyphs each); a test of the
wireframes reads the fonts' Unicode tables and fails on any other. Each has an ASCII form.

| Glyph | Means | ASCII |
| --- | --- | --- |
| `‹` | back (navigation bar) | `<` |
| `▶` | the selected row | `>` |
| `•` | a marked row, a list item in a message | `+`, `-` |
| `√` | done | `*` |
| `…` | running; an action that asks first | `>`, `...` |
| `×` | failed | `x` |
| `–` | skipped | `-` |
| `!` | warning | `!` |
| `·` | between facts on one line | `|` |
| `─ │ ┌ ┐ └ ┘` | borders, square: the fonts have no rounded corners | `- | +` |

## Terminal capabilities

Detected once at start, from the environment the menu was started with; the kernel command line can override them
(`zbm.theme=colour|mono|ascii`, proposed for R5).

| Signal | Means |
| --- | --- |
| `NO_COLOR` set to anything | monochrome |
| `TERM` is `dumb` or unset | no TUI: the plain console fallback |
| `TERM` starts with `vt1`, `vt2` or is `ansi` | ASCII |
| `TERM` is `linux` | colour, line characters (the console font has them) |
| a locale that is not UTF-8 (`LANG`, `LC_ALL`, `LC_CTYPE`) on a terminal other than `linux` | ASCII |
| anything else | colour, line characters |
| the terminal's size | the layout; below 52×20 the menu shows a one-line notice and the list only |

## Layout by size

| Size class | Inspector | Navigation bar | Toolbar |
| --- | --- | --- | --- |
| wide, 100 columns and more | right of the list, 42% (split view), with kernels and snapshots when tall | full | five actions and `? More` |
| regular, 80×25 | below the list, 6 lines | short status | five actions and `? More` |
| compact, down to 52×20 | below the list, 3 lines | short status, `‹` alone | as many actions as fit, and `? More` |

## Motion and redraw

Nothing moves on its own except the countdown and progress, which redraw one line once a second. Ratatui redraws only
the cells that change, so a key press in the filter costs tens of bytes; over 115200 baud a full redraw of 80×25 takes
about 0.2 s, which the menu avoids except when the size changes.
