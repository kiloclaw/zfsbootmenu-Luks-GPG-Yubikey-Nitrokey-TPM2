# The console on the display: roadmap to a full console

`zbm console` (`zbm.console=drm`, ROADMAP.md #19) draws the runtime's terminal through DRM instead of leaving it to the
kernel console. Steps 1 to 4 of #19 made it work; this page plans what it lacks to be a full console, measured against
three references: the Linux virtual terminal and `fbcon`, `kmscon` (a console in user space on DRM), and a terminal
emulator such as Alacritty or xterm. Step 5 of #19, making it the default, comes at the end of this plan (M9).

Each item has an ID (`C1` to `C112`) that commits, tests and the CHANGELOG name. Priorities: **P1** users hit it today,
or a full console needs it; **P2** the references have it; **P3** nice to have. Effort: **S** a day or less, **M** a few
days, **L** a week or more. Items marked *vendor* need a patch to the vendored `alacritty_terminal` (the vendoring rules
of [dependencies.md](dependencies.md) apply, and the patch is offered upstream).

## Where it stands

| Area | What works |
| --- | --- |
| Display | A DRM dumb buffer on the first connected output, in its preferred mode; only damaged areas copied; handed back before `kexec`, taken again when `kexec` fails (`zbm console release`, `reattach`) |
| Text | The image's PSF fonts, doubled where `zbm console-autosize` says; DejaVu Sans Mono through `swash`, hinted and anti-aliased; box drawing and block characters drawn to fill their cells; combining marks |
| Colour | 16, 256 and true colour; bold drawn bright; dim, inverse, hidden, one kind of underline; palette changes by OSC 4 |
| Terminal | Alacritty's state machine: alternate screen, scroll regions, character sets, synchronized updates; answers for cursor position, device attributes, colours and size |
| Keys | XKB layouts through `kbvm` with dead keys; the Linux console's key sequences; cursor key mode (DECCKM); Caps Lock light; keyboards found when plugged in; keys read back after lost events |
| Processes | A pseudo-terminal with sessions and job control (`setsid -c`); `zbm console layout`; the relay on the kernel console after the hand-over; a mirror of the screen's text for the VM tests |
| Secrets | Typed bytes, the programs' output and the screen wiped; no scrollback; no clipboard (OSC 52); the process cannot be dumped |
| Power | Woken by requests' signals and an inotify watch on `/dev/input`; asleep otherwise |

Done in `9645a64`: D1 the console sleeps while idle; D2 the screen's text is built only for the mirror; D3 TrueType
characters are drawn without copying their glyphs; D4 output and screen are wiped; D5 combining marks are drawn.

## How the work runs

- Each milestone is a series of commits, each with its unit tests, passing `cargo xtask lint`, `cargo xtask test` and
  the coverage and metrics ratchets. A milestone ends with the VM tests it names, run by the maintainer with KVM
  (`CLAUDE.md`), on images rebuilt from its last commit.
- What the console keeps or shows obeys [threat-model.md](threat-model.md): nothing typed at a secret prompt reaches a
  buffer that is not wiped, a scrollback, a selection, a screenshot or a reply to a program.
- The milestones are in order of need, not of size; within one, items are independent unless a dependency is named.
- The VM tests extend `runtime-rust-console` (`crates/zbm-vmtest/src/scenarios/runtime/console.rs`) or add
  `runtime-rust-console-*` tests next to it, reading the screen through the mirror (`zbm.console_mirror`) and QEMU
  screenshots.

## M1. Starts reliably, draws correctly

Goal: what users hit today on real machines.

- **C34 Start without a keyboard** (P1, S). `Keyboards::open` fails with "no keyboard found" when a USB keyboard has
  not enumerated yet, and the whole boot then stays on the kernel console. Open with none, and let the device watch of
  D1 add them. `zbm-display/src/input.rs`, `zbm-console/src/serve.rs`.
- **C58 The output the kernel console shows** (P1, S). `output::choose` takes the first connected connector; prefer one
  with a current CRTC, so a closed laptop's panel is not lit instead of the external monitor.
- **C59 Keep the current mode** (P1, S). Keep the CRTC's mode when the connector offers it, rather than switching to the
  preferred one: no flicker, no monitor losing sync.
- **C69 One frame per burst** (P1, S). `run` draws after every read of 16 KB; keep reading while output is ready, up to
  a few milliseconds, then draw once.
- **C70 Scroll by moving pixels** (P1, M). A scrolled screen redraws every cell; detect a scroll of the grid, move the
  frame's rows and draw only the new lines. The largest gain on `simpledrm`.
- **C78 `/dev/console` on the console** (P1, S). Writes to `/dev/console` (hooks, tools) go to the hidden kernel
  console; `TIOCCONS` on the programs' side of the pseudo-terminal shows them.
- **C90 The screen across a failed `kexec`** (P1, S). After `reattach` the console starts blank; keep the terminal
  across the hand-over, wiping it only when the console ends, or ask the programs to redraw (`SIGWINCH`).
- **C16 Cursor shapes** (P1, S). Draw the bar and underline cursors that DECSCUSR asks for; `screen.rs` draws a block.
- **C17 Cursor blink** (P1, S). Blink on a timer that only runs while the cursor is shown.
- **C2 Strikeout** (P1, S), **C3 underline styles** (P1, S): double, curly, dotted and dashed underlines, which the
  terminal's cells hold; **C4 underline colour** (P2, S), SGR 58.
- **C35 Keypad and Num Lock on the US tables** (P1, S), with the Num Lock light.
- **C94 Finish the mirror's screen** (P2, S). A screen the mirror could not take whole waits for the next frame; flush
  it when the mirror can be written, now that the console sleeps.
- **C108 Fuzz the terminal** (P1, S). A fuzz target that feeds random bytes to `Console::feed` and `draw`, in
  `fuzz/`, listed in the threat model.

VM tests: a USB keyboard added after start-up (`device_add usb-kbd`), two monitors with the second one current, the
cursor shapes of `printf '\e[5 q'`, and a hook writing to `/dev/console`.

## M2. A real terminal

Goal: programs see a terminal that does what they ask.

- **C25 Its own terminal description** (P1, M). Programs run with `TERM=linux` and use 8 colours. Install a terminfo
  entry for the console (or `linux-16color` and `COLORTERM=truecolor`), set by the supervisor for programs on it.
- **C26 Modifiers on special keys** (P1, S). Ctrl, Alt and Shift with the cursor keys, Home, End and the function keys
  send xterm's sequences (`ESC [ 1 ; 5 D`), described by C25. `zbm-console/src/keys.rs`.
- **C27 Application keypad** (P2, S). DECKPAM, which the terminal tracks (`TermMode::APP_KEYPAD`).
- **C28 The kitty keyboard protocol** (P2, M), whose modes the terminal tracks; the key encoder honours them.
- **C29 New line mode** (P3, S): Enter sends CR LF in LNM. **C30 Identification** (P3, S): XTVERSION and the secondary
  device attributes name the console.
- **C1 Wide characters** (P1, M). Characters two cells wide drawn across both; `screen.rs` draws a space on the
  spacer and cuts the glyph at the first cell.
- **C5 Bold and italic faces** (P1, M). DejaVu Sans Mono Bold and Oblique in the image (about 330 KB each, optional);
  bold stays bright for the 16 colours.
- **C8 Overhanging glyphs** (P2, S), drawn into the next cell rather than cut. **C9 Underline metrics** (P2, S) from the
  font's `post` table.
- **C10 Braille patterns** (P2, S), **C11 box drawing diagonals** (P2, S), **C12 Powerline and legacy computing
  symbols** (P3, S): drawn by `zbm-display/src/boxes.rs` to fill their cells.
- **C6 A fallback font** (P2, M). Fonts tried in order for characters the first lacks, from the image's font
  directory; the replacement character only after all of them.
- **C18 Cursor colour** (P2, S), OSC 12 and 112.
- **C19 Linux cursor size**, **C21 Linux palette** (`ESC ] P`, `ESC ] R`), **C22 Linux private sequences** of `setterm`
  (P2, S, *vendor*): what programs send to a terminal named `linux`, until C25 names it otherwise.
- **C20 Blinking text** (P3, S, *vendor*), **C24 reverse screen** DECSCNM (P3, S, *vendor*), **C13 double-width and
  double-height lines** (P3, M, *vendor*).
- **C109 Reference images** (P1, M). The terminal drawn in memory at three sizes and compared with images in the
  repository, as the menu's wireframes are compared with text: colours, attributes, cursor shapes, wide characters.

VM tests: `vttest` screens read through the mirror; `tput colors` and a 256-colour bar in a screenshot; Ctrl-Left in
the recovery shell's readline.

## M3. A comfortable recovery shell

Goal: the recovery shell is as usable as on a desktop terminal.

- **C52 Scrollback** (P1, M). Shift+PgUp and Shift+PgDn over a scrollback of a few thousand lines in locked memory,
  off and wiped while a secret prompt is open (the menu and `zluks` say so through the state directory) and when the
  console ends.
- **C53 Search and vi mode** (P2, M) in the scrollback, with Alacritty's vi mode. **C54 Save the scrollback** (P3, S)
  to the state directory for `zbm report`. Both depend on C52.
- **C87 Font and size while running** (P2, M). `zbm console font <name|size>` and Ctrl+Alt+`=` / `-` rebuild the grid
  and tell the programs the new size.
- **C83 Status line** (P2, M). An optional last row: Caps Lock, Num Lock, the layout, the title (**C31**, P2, S), the
  time, and the battery on laptops.
- **C84 Visual bell** (P2, S): the frame's border flashes on `Event::Bell`. **C47 Audible bell** (P3, S): a tone on the
  PC speaker's input device (`EV_SND`).
- **C88 Screenshot** (P3, S). Print Screen saves the screen's text and image to the state directory; none while a
  secret prompt is open.
- **C42 Layout hotkey** (P2, S), cycling the image's layouts. **C43 Dead key preview** (P2, S): the waiting accent at
  the cursor. **C44 Unicode input** (P3, M): Ctrl+Shift+U and the code.
- **C36 Num Lock at start** (P2, S), `zbm.numlock=on`. **C37 Scroll Lock holds output** (P2, S), with its light, as on
  the kernel console. **C38 More keys** (P2, S): F13 to F24, Print, Pause, Menu, Super, media keys.
- **C39 Key repeat** (P2, S), `zbm.keyboard_repeat=<delay>,<rate>` through `EVIOCSREP`.
- **C40 Compose key** (P2, M) and **C41 layouts per keyboard** (P2, M), left for later by #19 step 4.
- **C23 Colour schemes** (P2, S), `zbm.console_palette=` (the Linux palette, a high-contrast one, a few others), and
  **C86 large text and high contrast** (P2, S) as one switch.

VM tests: scrollback shown and then gone after a passphrase prompt (searched in memory as `runtime-rust-secrets`
searches), a font size changed with keys, and the status line's Caps Lock.

## M4. Displays

Goal: the displays a machine has, as the kernel console uses them, and better.

- **C61 Hotplug and resize** (P1, M). DRM hotplug events (`uevent` on the card); choose the output again, resize the
  terminal and send the programs the size.
- **C62 Every monitor** (P2, M), the same picture on each connected output, at the size of the smallest.
- **C63 Rotation** (P2, M), from the connector's `panel orientation` property, or `zbm.console_rotate=`.
- **C60 A chosen mode** (P2, S), `zbm.console_mode=<width>x<height>`, as `video=` chooses it for the kernel.
- **C64 Blanking and power saving** (P2, S), `zbm.console_blank=<minutes>`: the screen goes dark, then the output off
  (DPMS), and wakes on a key, as `setterm -blank -powerdown` do.
- **C65 Pixel formats** (P2, S). RGB565 and the BGR orders when a driver refuses XRGB8888.
- **C66 Another program holding the display** (P2, S): retry for a while and say which, as Plymouth can.
- **C67 Backlight** (P3, S): the brightness keys of laptop panels through sysfs.
- **C68 Scaling** (P3, M): an integer or fractional scale beyond `zbm.autosize`.
- **C74 Double buffering** (P2, M): two buffers, flipped on vertical blank, no tearing.

VM tests: a second display added and removed in QEMU, a rotated panel (`virtio-gpu` with a portrait mode), blanking
after a short timeout and waking on a key.

## M5. Speed

Goal: a shell flooding the console keeps up on the slowest framebuffer.

- **C71 Cell cache** (P2, M). Cells drawn once per character, colours and attributes, then copied row by row; the grid
  computes each pixel today.
- **C72 The buffer stays mapped** (P2, S). `Display::present` maps and unmaps the dumb buffer for each frame and copies
  pixel by pixel; keep the mapping and copy rows.
- **C73 Several clip rectangles** (P2, S) per frame instead of their union (`Frame::take_damage`).
- **C75 Frames at the refresh rate** (P3, S) at most.
- **C76 Warm and lean** (P3, S): rasterize ASCII and the menu's characters at start; no allocation per frame.
- **C112 Benchmarks** (P2, S): time to the first frame, `cat` of a large file, the image's size, each against the
  kernel console, recorded in `docs/luks/testing.rst`.

## M6. The kernel console's place

Goal: nothing the kernel console showed is lost under the console.

- **C77 Kernel messages** (P1, M). Messages at `err` and above from `/dev/kmsg` in the status line or an overlay, and a
  key that shows the kernel log; the kernel console in graphics mode shows none of them.
- **C79 Reading `/dev/console`** (P2, S). A hook reading there waits on a keyboard the console holds; detect it, warn
  in the log, and document it for hook writers.
- **C80 The text before the console** (P2, S). Start with what the virtual terminal showed (`/dev/vcs`), so messages
  of the early start-up stay on screen.
- **C81 The serial console too** (P2, M). The same session on a serial port, its output written there and its keys
  read, for machines used both at a desk and through a serial line.
- **C82 A logo and progress** (P3, S) before the first program writes.
- **C104 The menu knows its console** (P2, S). An environment variable tells the menu it runs on this console, which
  draws true colour and every character, beyond the 256 glyphs of the console fonts.
- **C105 Ctrl-K switches the console's layout** (P2, M), and the unlock dialog stops translating keys by position on
  this console (#19 step 4, left for later).
- **C106 Image options** (P2, S): the TrueType font, the colour scheme, the first layout and Num Lock chosen in
  `generate-zbm`'s configuration.

## M7. Robust and private

Goal: the console never takes the boot down with it, and keeps secrets as the rest of the runtime does.

- **C89 Restart after a crash** (P1, M). The supervisor starts the console again, at most a few times with growing
  delays, before staying on the kernel console (#19 step 2, left for later).
- **C91 Answers instead of polling** (P2, S). `release` and `reattach` wait for a reply on a socket or a `pidfd`
  instead of reading the state directory every 20 ms.
- **C92 Watchdog** (P2, M): a present that blocks too long gives the display back and relays.
- **C93 A display that goes** (P2, S): a test of the fall-back when the card disappears, which names the card.
- **C95 Out of the OOM killer's reach** (P2, S), `oom_score_adj` -1000.
- **C96 Locked buffers** (P1, S). The queue of typed bytes, the decoder's keys and the layout's events in locked pages,
  as secrets are (`zbm_platform::secret`).
- **C45 Exclusive keyboards** (P1, S). `EVIOCGRAB`, so that no other process reads the keys typed at a prompt.
- **C46 SysRq and Ctrl-Alt-Del** (P2, S): check that the kernel still handles them while the console reads the keys,
  test and document it.
- **C97 The parser's buffers** (P2, S), wiped with the screen.
- **C98 Fewer privileges** (P2, M) once the display, the keyboards and the virtual terminal are open.
- **C99 A system call filter** (P2, M) for the console's process.
- **C100 No reading back** (P2, S): a test that no answer to a program carries the screen's text.
- **C110 A passphrase in no buffer** (P1, M): typed at a prompt on the console, then searched in the console's memory
  and the frame, as `runtime-rust-secrets` searches the runtime.

VM tests: the console killed twice and back each time; a passphrase searched in the console's memory.

## M8. Mouse, selection and terminals

Goal: what desktop users expect, within the threat model.

- **C48 Mouse and touchpad** (P2, M): pointer, buttons and wheel from the input devices.
- **C49 Mouse reports** (P2, M) to programs that ask (the terminal tracks the modes); the menu can use the wheel and
  clicks.
- **C50 Selection and paste** (P2, M) with the mouse or a keyboard copy mode, pasted with bracketed paste; nothing
  selected from a secret prompt, and no clipboard outside the console. Depends on C48 and C52.
- **C51 Paste a file** (P3, S), such as a key file's path or contents, after a confirmation.
- **C55 A second terminal** (P2, L). Alt+F2 opens a shell on a terminal of its own while the menu runs on the first;
  Alt+F1 goes back. **C56** (P3, M): which terminal is shown, and a layout for each.
- **C57 Virtual terminal switches** (P2, S): `VT_LOCKSWITCH`, or `VT_PROCESS` mode that gives the display back on a
  switch.
- **C32 Hyperlinks** (P3, S) underlined; **C33 images** (P3, L, sixel or the kitty protocol); **C7 colour emoji**
  (P3, L); **C15 right-to-left text** (P3, L): last, if at all.

VM tests: QEMU's tablet device clicking in the menu; a selection pasted into the shell; Alt+F2 and back.

## M9. The default

Goal: #19 step 5.

- **C85 Screen readers** (P2, M). The screen's text where `brltty` reads it (a file like `/dev/vcsa`, or its socket),
  for Braille displays; documented with `speakup`, which needs the kernel console.
- **C101 `zbm console status`** (P2, S): card, mode, font, grid, layouts, keyboards, frame times; in `zbm report`.
- **C102 `zbm.console=auto`** (P2, M): the console on the display when a DRM driver and a display are there, the kernel
  console otherwise.
- **C107 Documentation** (P2, S): the options, the keys, hook writers' notes (C79), troubleshooting, in `docs/` and the
  manual pages.
- **C111 VM tests in full** (P2, M): every milestone's tests in one `runtime-rust-console-*` group.
- **C103 The default** (P1, L). After the VM tests and a list of real machines (the GPUs of #17's known list, laptops
  with internal and external displays, a serial-only server), `zbm.console=drm` becomes the default where
  `zbm.console=auto` finds a display; `zbm.console=kernel` keeps the kernel console.

## Left out

- **C14 Ligatures**: a console shows each character as typed.
- A clipboard shared with the booted system or with programs (OSC 52), scrollback or selection while a secret prompt
  is open, and answers that read the screen back: they would break the threat model.

## Index

| Milestone | Items |
| --- | --- |
| M1 | C2 C3 C4 C16 C17 C34 C35 C58 C59 C69 C70 C78 C90 C94 C108 |
| M2 | C1 C5 C6 C8 C9 C10 C11 C12 C13 C18 C19 C20 C21 C22 C24 C25 C26 C27 C28 C29 C30 C109 |
| M3 | C23 C31 C36 C37 C38 C39 C40 C41 C42 C43 C44 C47 C52 C53 C54 C83 C84 C86 C87 C88 |
| M4 | C60 C61 C62 C63 C64 C65 C66 C67 C68 C74 |
| M5 | C71 C72 C73 C75 C76 C112 |
| M6 | C77 C79 C80 C81 C82 C104 C105 C106 |
| M7 | C45 C46 C89 C91 C92 C93 C95 C96 C97 C98 C99 C100 C110 |
| M8 | C7 C15 C32 C33 C48 C49 C50 C51 C55 C56 C57 |
| M9 | C85 C101 C102 C103 C107 C111 |
| Left out | C14 |
