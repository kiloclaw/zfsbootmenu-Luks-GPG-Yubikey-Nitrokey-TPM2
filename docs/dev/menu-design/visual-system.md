# The visual system

The rules behind it are Apple's, adapted to the console: [apple-hig.md](apple-hig.md).

The menu runs on the Linux console, serial consoles, and terminal emulators from the 1980s to now. It must read well
on all of them, so the visual system is small: a handful of roles, each drawn in three ways, and borders in two.

## Roles

Every styled piece of text has a role; views never pick colours. The roles and their styles are in `theme.rs` of
`zbm-tui`.

| Role | Used for | 16 colours | Linux console | Monochrome |
| --- | --- | --- | --- | --- |
| `Text` | ordinary text | default | white | default |
| `Muted` | secondary facts: sizes, ages, origins; labels; toolbar titles | dark grey | grey | default |
| `Title` | view and pane titles | bold | white, bold | bold |
| `Selected` | the selected line of a list | black on cyan, never bold | black on cyan, never bold | reverse |
| `Match` | characters the filter matched | yellow, bold | same | bold, underlined |
| `Default` | the default environment or kernel | green, bold | same | bold |
| `Good` | `ONLINE`, `open`, `done` | green | same | default |
| `Warning` | `DEGRADED`, read-only, warnings | yellow | same | bold |
| `Danger` | `FAULTED`, failures, destructive actions | red, bold | same | bold, reverse |
| `Locked` | encrypted and not unlocked | magenta | same | underlined |
| `Key` | key names in the toolbar and help | cyan, bold | same | bold |
| `Border` | borders and separators | dark grey | same | default |
| `StatusBar` | unused since the navigation bar replaced the status bar | black on white | same | reverse |
| `Accent` | the tint colour: the back label, the countdown's seconds | cyan, bold | same | bold |
| `Header` | section headers, in upper case | dark grey | grey | default |

**On the Linux console** (`TERM=linux` in colour, or `--theme console` to preview it) the background is black and the
palette is the VGA one, so the theme can use three levels of text, as labels have in Apple's dark appearance: white
for what is read first (names, values, actions), grey for what explains it (labels, sizes, ages, toolbar titles,
section headers), and dark grey only for lines, whose low contrast is wanted. Elsewhere the palette and the
background are unknown: text keeps the terminal's own colour, and secondary facts are dark grey, which every palette
draws readably on its background. Text drawn without a role takes the `Text` style of the screen and of each dialog,
so it is primary everywhere.

**The selection is never bold.** The console draws bold as bright, and bright black is dark grey, which cannot be read
on cyan: the `default` label of the selected row was unreadable. The selected row's labels take its black on cyan.

**Emphasis is spent once per line.** The countdown banner is plain text with the environment in bold, the seconds in
the accent colour and the hint in grey, so the eye goes to what boots and when; a fallback's banner stays a warning
throughout. The toolbar's keys are in the accent colour and their titles in grey, so the keys stand out and the
toolbar stays quiet.

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

Only glyphs that every console font an image can have, as images with the Rust runtime install them: the six
Terminus fonts, or the four Spleen fonts with `Global.ConsoleFont: spleen`, as 256-glyph subsets (`zbm_types::psf`),
which keep the console's 16 colours. A test of the wireframes makes the subsets of both families and fails on any
character they cannot draw. Each glyph has an ASCII form.

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
(`zbm.theme=colour|console|mono|ascii`, proposed for R5).

| Signal | Means |
| --- | --- |
| `NO_COLOR` set to anything | monochrome |
| `TERM` is `dumb` or unset | no TUI: the plain console fallback |
| `TERM` starts with `vt1`, `vt2` or is `ansi` | ASCII |
| `TERM` is `linux` | colour with the console's palette (three levels of text), line characters (the console font has them) |
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
