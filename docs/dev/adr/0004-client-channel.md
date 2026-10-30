# ADR 0004: The menu draws natively on the console

- **Status:** accepted, 28 October 2026 (proposed the same day)
- **Decides:** decision 15 of [ROADMAP.md](../../../ROADMAP.md#decisions)
- **Governs:** PM7, UI3, UI5, UI6, UI7, UI8, UI9; the console's C52, C89, C91, C96, C104, C105 and C110

## Context

On the display, `zbm console` (`zbm.console=drm`) runs Alacritty's terminal on a pseudo-terminal, and the menu is a
program on that terminal: Ratatui writes escape sequences, the console parses them back into cells and draws them.
Around that:

- the console learns what to do from signals and files: `SIGUSR1` with the `console-released` record to hand the
  display back before `kexec`, `SIGUSR2` with a layout request file, a font request file, and a record of the process
  that asks for a secret, so that its keys stay out of the scrollback and the mirror;
- keys reach the menu as terminal bytes, which the menu decodes again (UI2), so the key's layout and position are lost
  on the way, and Ctrl-K cannot switch the console's layout (C105);
- the menu cannot know what the console can draw (true colour, which characters the font has) and guesses from `TERM`
  and probes (C104);
- a passphrase typed into the menu crosses the pseudo-terminal, whose kernel buffer outlives it.

Serial consoles, SSH sessions and the kernel console have no `zbm console`, and must keep working with a terminal.

## Decision

**Two typed channels: a control socket for requests to the console, and a client channel on which the menu sends
cells and receives key presses. Programs that need a terminal keep one.**

1. **The control socket (PM7).** The console listens on a `SOCK_SEQPACKET` socket; `zbm console release`, `reattach`,
   `layout`, `font` and `status` become requests with replies (`Release` → `Released`, `Reattach` → `Reattached`,
   `Layout(name)` → `LayoutSet(name)` or `NoSuchLayout`, `Font(size)` → `FontSet`, `Status` → `Status { .. }`).
   A reply means it was done: no polling of records, no signals. The peer's credentials are checked (`SO_PEERCRED`,
   root only).
2. **The client channel (UI6).** The supervisor creates a socket pair and hands one end to the console and the other to
   the session ([ADR 0002](0002-trust-zones.md)). After a handshake, the client sends `Cells` (damaged rows of styled
   cells), `Cursor`, `Input { secret }`, `SwitchLayout(name)` and `Announce(text)`; the console sends `Caps` (the
   grid, true colour, the characters the font has), `Key(KeyPress)` with the layout's name, `Resize` and `Bell`.
3. **Surfaces.** The console shows surfaces: `Surface::Cells`, filled by a client, for the menu; `Surface::Terminal`,
   today's Alacritty terminal on a pseudo-terminal, for the shell, hooks and `/dev/console`. Layers (the status line,
   search, kernel messages) draw over either. One renderer draws both (`CellRenderer`, UI4).
4. **The secret flag in band.** `Input { secret: true }` travels in the same ordered stream as the keys, so the console
   knows exactly which key presses belong to a passphrase: they go to the client and nowhere else (no scrollback, no
   mirror, no log), and no pseudo-terminal holds them.
5. **The seat as typestate (UI5).** `Seat<Held>` → `release()` → `Seat<Released>` → `reattach()`; nothing draws on a
   display that was handed back before `kexec`.
6. **One front end runtime (UI3).** The menu is a `Frontend<B: Backend, I: Input>` with three backends: the console's
   cells, a terminal (serial, SSH, the kernel console when there is no `zbm console`), and Ratatui's test backend.
   The model, the view and the key map are the same on all three; if the console goes away, the front end goes on on
   the kernel console with the same model.
7. **Versioned messages.** Both channels start with `Hello { version }`; a console and a client of different versions
   refuse each other with a message, and the client falls back to the terminal backend.

## Consequences

- The menu's frames go to the console as cells, not as escape sequences parsed back: less work per frame, and no
  terminal emulation between the menu and the screen.
- Keys arrive as `KeyPress` with their layout, so Ctrl-K switches the console's layout (C105), and the unlock dialog
  translates by key position with the right keymap ([UI1](../../../ROADMAP.md#ui-the-menu-the-console-and-the-display)).
- The capabilities come in the handshake, so C104's probing goes; C52's detection of secret prompts becomes the flag;
  C89 (the console restarting) is a reconnect; C96 and C110 shrink.
- Passphrases typed in the menu on the display no longer pass through a pseudo-terminal.
- Two backends to keep working: the cells for the console, the terminal for everything else. The test backend and the
  wireframe snapshots cover the view once for both.
- Hooks, the shell and programs the menu runs (an editor, `zfs-chroot`) still get a terminal surface; the menu lends
  the screen with a `SurfaceLease` and takes it back when the lease drops.
- The console gains a socket server, and with it inputs to fuzz (SE5).

## Alternatives considered

- **Keep the menu on the console's terminal.** It works today, but every limit above stays, and secret handling relies
  on a record of the asking process instead of the stream itself.
- **The console as a Ratatui backend in the same process as the menu.** Fastest, but it puts the DRM card, the input
  devices and the menu's parsing of pool data in one process, against [ADR 0002](0002-trust-zones.md).
- **A shared-memory frame buffer** between the menu and the console. Less copying, but the console would have to trust
  the menu's pixels, and accessibility (text for `brltty`, UI8) needs cells, not pixels.
- **D-Bus or another general bus.** A daemon and a dependency in the initramfs for two peers that know each other.

## How it is checked

- Fuzz targets for both channels' decoders, on both sides (SE5).
- A test drives the menu through the client channel against a console on a fake display, and compares its cells with
  the same view on Ratatui's test backend.
- The VM test of secrets searches the console's memory, scrollback and mirror for a typed passphrase.
- `runtime-rust-console` and its sibling VM tests run on the cell surface once UI6 lands.

## Notes from the implementation

- **UI3** (29 October 2026): the front end runtime is `zbm_tui::menu::frontend::Frontend<B: Surface, I: Input>`. A
  `Surface` is a Ratatui backend that is told its size and presents each frame (the terminal's `QuietBackend`, the
  display's pixels, the test backend); an `Input` gives keys, and may wake for the machine's mailbox. The menu's loop
  is `Frontend::run`, for any machine on any surface, and the start-up view is the start-up screen of the model on a
  `Frontend`. The cell surface of the client channel (UI6) is a `Surface` too.
- **PM7** (29 October 2026): the control socket is `/run/zfsbootmenu/console.sock`, served by a thread of the console
  on `zbm_sys::socket`, root peers only. The messages are in `zbm_app::console`, beside the session's, with a version
  in each, and the console's side is `zbm_console::control`. There is no `Hello`: each request carries the version,
  and one of another version is refused with `version`, in the console's version. Besides the requests above, the
  menu's secret prompt is a `secret` request held open for as long as the prompt shows, instead of a record of the
  process; it becomes `Input { secret }` on the client channel with UI6. `SIGUSR1`, `SIGUSR2` and the request files
  are gone; the records of the console's process, its hand-over, its layout and its font stay, for hooks, users and
  the boot.
- **UI4** (30 October 2026): the one renderer is `zbm_display::render::CellRenderer`, over the `Grid` of a font. The
  console's terminal draws through it, and so does the menu's pixel backend until the cell surface of UI6 replaces
  it: a `Surface::Cells` will be drawn by the same renderer.

## Answers of the maintainer

Accepted as proposed, on 28 October 2026:

1. **The encoding:** a small binary encoding of our own for `Cells` (rows of runs of one style, UTF-8 text), which
   must be cheap at every frame, and JSON with `serde_json`, already in the runtime, for the control socket, where
   readability helps debugging.
2. **Where the control socket lives:** `/run/zfsbootmenu/console.sock`, beside, not inside, the state directory, so
   hooks that list `/zfsbootmenu` do not trip over it.
3. **Several clients:** one cell client at a time (the session); the SSH menu draws on its own terminal and reaches
   the session, not the console ([ADR 0002](0002-trust-zones.md), PM10).
