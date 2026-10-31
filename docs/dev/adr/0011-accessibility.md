# ADR 0011: Accessibility: Braille displays, speech, and a line view of the menu

- **Status:** accepted, 31 October 2026 (proposed the same day)
- **Decides:** item UI8 of [ROADMAP.md](../../../ROADMAP.md), the console's C85, the plain mode of feature #12
- **Governs:** the console on the display (`zbm console`), the menu's views and its `Announce` messages
  ([ADR 0004](0004-client-channel.md)), the start-up program's choice of front end, the image's build options

## Context

A blind person reaches a machine's console through a screen reader: `brltty` for a Braille display, `speakup` for
speech. Both read the Linux kernel's virtual terminals: `brltty`'s Linux screen driver reads `/dev/vcsaN`, the text, its
attributes and the cursor of a virtual terminal; `speakup`, in the kernel, speaks what is written to a terminal as it is
written, and lets its keys review the screen. Over a serial line, a person uses the screen reader of the machine they
connect from, which reads what the line prints.

ZFSBootMenu serves none of them well today:

- **The console on the display** (`zbm.console=drm`) draws its cells itself, with its virtual terminal in `KD_GRAPHICS`
  mode: the terminal's text, which `brltty` and `speakup` read, is empty or stale.
- **The menu** draws a full screen: columns, borders, a footer, redrawn in place. A screen reader on the kernel console
  or a serial terminal reads that as a jumble of cells that changed, and gets no sense of which item is selected, or
  that a dialog opened.
- **`TERM=dumb`**, which screen-reader users set for exactly this reason, makes the start-up program give up the menu
  (`Capabilities::interactive` is false): it counts down and opens the recovery shell, as the legacy runtime does
  without `fzf`. Booting another environment, a snapshot or a kernel needs the shell.
- **`Announce(text)`**, the channel's message for "a line for a screen reader", exists (ADR 0004) and nothing sends or
  shows it.

## Options

How a Braille display reads the console on the display:

1. **The console writes its screen into its virtual terminal** (`/dev/vcsaN` of the terminal it drew on): the rows of
   text with their colours as attributes, and the cursor, each time a frame is whole. `brltty` reads it as it reads any
   kernel console, with no change to `brltty` and nothing new in the image but the device in the console's sandbox.
   The terminal stays in `KD_GRAPHICS`, so the kernel draws none of it.
2. **The console talks to `brltty` over BrlAPI**, `brltty`'s client protocol: it would know what is focused, and could
   send only that. A protocol of our own to implement and keep, and `brltty` must run with its API.
3. **Only on the kernel console**: Braille users boot with `zbm.console=kernel` and the line view below; the console on
   the display does nothing for them.

When the menu shows as lines instead of a screen:

1. **When `TERM` is `dumb` or unset, when `speakup` is loaded, or with `zbm.menu=lines`.** Each automatically where a
   screen reader is likely, and anywhere by choice.
2. **Only with `zbm.menu=lines`.**
3. **Only when `TERM` is `dumb` or unset**, in place of the plain fallback.

What `Announce` carries:

1. **What changes without the person moving**: the countdown starting and stopping, a dialog or a question opening,
   an error, an action finishing, the screen reverting to the menu after the shell. The line view prints them; the
   console on the display puts the latest on the last row of the text it writes for `brltty` (option 1 above) for a few
   seconds, and on its status line when it has one.
2. **Every change of the selection too**, as a spoken menu would; noisy on a Braille display, which shows the selected
   line already.

`brltty` in the image:

1. **A build option, `zfsbootmenu_brltty=yes`**, installs the build host's `brltty` with its configuration and the
   drivers it names (`/etc/brltty.conf`), and starts it in `early-setup.d`, before anything is asked; `speakup`'s modules
   with `zfsbootmenu_speakup=<synth>`. Distributions whose `brltty` ships a dracut module of its own (Fedora's
   `brltty-dracut`) can use theirs instead.
2. **Documentation only**: how to add `brltty` with dracut's own configuration (`install_items`, a hook).
3. **Later**, after the line view.

## Proposal

The first option of each.

**The line view.** A front end of `zbm-tui` over the same `Model` and `update` as the screen, with a view and an input
of its own (`Frontend<LineBackend, LineInput>`, UI3): the main view prints as a numbered list (`1) zroot/ROOT/void
(default, 6.12.8)`), a dialog as its question and its choices, a secret prompt as `Passphrase for root: ` without echo.
A line typed is a number, which selects that item and does its primary action, or one of the key map's letters (`k 2`
for the kernels of item 2, `s 2` for its snapshots, `!` for the recovery shell, `?` for help), which the input turns
into the same `KeyPress`es the screen gets. A view prints again only when it changed, never in place, and the countdown
prints its seconds once a second on one line. It runs wherever the menu runs: the start-up program, the menu program,
the menu over SSH; on the kernel console, a serial line, or the console on the display.

**The choice.** `TERM` `dumb` or unset, `/sys/accessibility/speakup` present, or `zbm.menu=lines` (a new option of the
command line, `screen` the default) gives the line view; `zbm.menu=screen` forces the screen. The plain fallback goes:
`TERM=dumb` gets the line view instead of the countdown and a shell.

**`Announce`.** The menu sends it for what changes without the person moving (option 1): the line view prints it; the
console on the display shows it on its status line, and on the last row of its terminal's text, for five seconds.

**`brltty`.** The console on the display writes each whole frame's text, attributes and cursor into `/dev/vcsaN` of its
virtual terminal, which its sandbox then reaches (`Access::Device`); a terminal it lends to the programs, as for the
shell, it leaves to the kernel, as now. `zfsbootmenu_brltty=yes` puts `brltty` into the image and starts it before
anything is asked; `zfsbootmenu_speakup=<synth>` the `speakup` modules, which speak the kernel console and the line
view.

## Consequences

- A person with a Braille display reads the console on the display as the kernel console, unchanged for `brltty`.
- A person with speech, or over a serial line, gets a menu made of lines that a screen reader reads in order, and does
  everything the screen does with numbers and the key map's letters.
- `TERM=dumb` no longer means "no menu".
- `brltty` in the image is a program of root that parses what a USB or serial device sends: the threat model gains it
  as an input (A2, a hostile device), for images that ask for it alone.
- The console's text in `/dev/vcsaN` is readable by root, who reads the screen anyway; a secret is never on it, as it
  is never drawn (C52).

## Alternatives considered

- **A text dump of the screen in a file**, for a `brltty` screen driver of our own: `brltty` has none for it, and its
  Linux driver already reads `/dev/vcsa`.
- **The line view as a separate program**: two programs to keep in step with the model; one model with two views is
  what UI3 built the front end runtime for.

## How it is checked

- Unit tests of the line view on the demo machine: each core task (boot the default, another environment, a kernel, a
  snapshot; unlock a volume; the recovery shell; power off) done by lines alone, as the screen's tests do it by keys;
  the views printed match golden files.
- The VM test `runtime-rust-console-lines`: with `TERM=dumb` on the serial console, the menu prints its list, `2`
  boots the second environment; with `zbm.menu=lines`, the same. With `zbm.console=drm`, `/dev/vcsa1` read from the
  recovery shell, before it lends the terminal, holds the menu's text.
- `brltty` itself on real hardware: by the maintainer, or someone who uses one, before the release notes claim it.

## Where the work stands

Accepted; the work has not started. Nothing is built: `Announce` is in the channel's messages and unused, and `TERM=dumb` gives the plain
fallback.

## Answers of the maintainer

Accepted as proposed, on 31 October 2026:

1. **Braille on the console on the display:** its text, attributes and cursor written into its virtual terminal's
   `/dev/vcsaN`, which `brltty` reads (option 1).
2. **When the line view shows:** `TERM` dumb or unset, `speakup` loaded, or `zbm.menu=lines`; `zbm.menu=screen` forces
   the screen (option 1).
3. **What `Announce` carries:** what changes without the person moving (option 1).
4. **`brltty` in the image:** the build options `zfsbootmenu_brltty` and `zfsbootmenu_speakup` (option 1).
