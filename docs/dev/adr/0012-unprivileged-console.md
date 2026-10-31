# ADR 0012: The console on the display as an unprivileged user

- **Status:** proposed, 31 October 2026
- **Decides:** the part of item SE11 of [ROADMAP.md](../../../ROADMAP.md) that is left for the console, and point 4 of
  [ADR 0002](0002-trust-zones.md) ("Z3 draws and reads keys, unprivileged") for `zbm console`
- **Governs:** how the supervisor starts the console, how the console gets its devices and its control socket, where it
  keeps its records, its locked memory, and the `Console` profile of `zbm_platform::sandbox`

## Context

The console on the display (`zbm console`, `zbm.console=drm`) parses whatever the programs print (I14 of the threat
model): names from pools, files from boot environments and the output of programs, through Alacritty's terminal parser.
It is the process most exposed to hostile text that runs all the time, and it sees every key typed, passphrases too.

Since SE11 began, it confines itself before it starts its threads: `no_new_privs`, a seccomp allow-list, landlock on its
devices, `/sys`, `/proc`, its fonts and layouts, the state directory and the sockets' directory, and two capabilities,
`CAP_SYS_TTY_CONFIG` and `CAP_IPC_LOCK`. It stays **user 0**, because:

- **It opens devices again.** It opens the DRM card, the virtual terminal and every keyboard each time it takes the
  display (`serve::on_display`): at the start, and again with `zbm console reattach` after a failed `kexec`. It opens a
  keyboard that is plugged in later when inotify on `/dev/input` reports it. These nodes are root's, mode `0600` or
  `0660`.
- **It sets the virtual terminal's modes** (`KDSETMODE` to graphics, `KDSKBMODE`, `VT_SETMODE`). The kernel allows these
  to the terminal's controlling process or to `CAP_SYS_TTY_CONFIG`; the console opens the terminal with `O_NOCTTY`.
- **It writes the state directory**, `/zfsbootmenu`, which is root's: `console` and `released-console` (its process ID,
  which the boot and the supervisor read), `console-layout`, `console-font`, `screenshot-N.txt` and `.png`,
  `console-scrollback.txt`, and its crash reports.
- **It binds its control socket** in `/run/zfsbootmenu`, a directory of root's with mode `0700`, and refuses a peer that
  is not of its own user, 0.
- **It locks all its memory** (`mlockall` with `MCL_FUTURE | MCL_ONFAULT`), so that the screen and its history, which
  can hold what was typed, never reach swap. Without `CAP_IPC_LOCK`, `RLIMIT_MEMLOCK` bounds that.
- **It writes the kernel log** (`/dev/kmsg`, root's, `0644`), opening it for each line.

Landlock and seccomp do most of the work today, but being user 0 still gives a compromised console these things:

- **It can impersonate the session.** Its landlock rules give it the whole sockets' directory, so it can remove
  `session.sock` and bind its own. Every peer check there is "is it user 0" (`zbm_app::wire::request_of`), and the
  console is user 0. A menu over SSH, or `zluks`, would then talk to the console as if it were the session.
- **It can write the state files the boot trusts** (I11): `luks-mappings`, `degraded` and the command line caches are
  in the directory it may write.
- **It owns every file of root's** wherever landlock is missing: a kernel without landlock, or with it left out of
  `lsm=`. The console then logs what it applied (`confined:` in the kernel log) and runs on. User 0 without
  `CAP_DAC_OVERRIDE` can still open any file whose owner is root, `/dev/sda` included.

## Options

How the console gets its devices:

1. **A broker in the supervisor; the console never runs as root.** The supervisor starts the console as its own user,
   and opens the devices it asks for on a socket pair (`--devices-fd`), as `systemd-logind`'s `TakeDevice` does: a DRM
   card, a virtual terminal, its text (`vcsa`), a keyboard. It checks that each is a character device of the expected
   major under `/dev` (226 for DRM, 13 with a minor of 64 or more for `evdev`, 4 for a virtual terminal from 1 to 63,
   7 for `vcsa`), opens it, and sends the descriptor. The console's seat and keyboards open through a port that is the
   broker in the image and the file system in tests.
2. **Root at the start, then a user change; the broker only for what comes later.** The console opens what it has at
   the start, then becomes its user and keeps the descriptors. A keyboard plugged in later and the display taken again
   after a failed `kexec` go through the broker of option 1. Two ways to get a device instead of one, and the console's
   start still runs as root, before its sandbox.
3. **Groups.** The console runs as its user with the groups `video`, `input` and `tty`, as a desktop session does. It
   depends on udev's rules giving the nodes those groups, and on `/etc/group` in the image naming them, which neither
   dracut nor mkinitcpio promises. Other users' devices of those groups open to it too.

How it sets the virtual terminal's modes:

1. **It keeps `CAP_SYS_TTY_CONFIG` as its one capability**, raised as an ambient capability by the supervisor before
   `exec`, under the new user. It lets the holder change any virtual terminal's modes and keyboard, and hang a terminal
   up (`vhangup`); landlock still limits which terminals it can open.
2. **The terminal becomes its controlling terminal** (`setsid`, then `TIOCSCTTY`), which needs no capability. The
   console then gets the terminal's `SIGHUP` and job control, and the terminal can be the controlling terminal of no
   other session; the relay on the kernel console, after the display is handed back, writes to it.

Its locked memory:

1. **The supervisor raises the console's `RLIMIT_MEMLOCK` to unlimited** before `exec`, as root may. `mlockall` works
   as now, without `CAP_IPC_LOCK`. A compromised console can lock memory it could exhaust anyway.
2. **A bound from `zbm.console_scrollback`.** With `MCL_FUTURE`, a mapping beyond the bound fails: an allocation the
   console did not plan for (a larger font, a resize) ends it.
3. **Only the history and the screen's cells in locked pages** (`zbm_sys::memory`'s locked regions), and no
   `mlockall`. The least locked, but the screen's cells and their caches would need their own allocator.

Its records:

1. **A directory of its own, `/zfsbootmenu/console/`,** made by the supervisor and owned by the console's user:
   `pid`, `released`, `layout`, `font`, the screenshots, the scrollback and its crash reports. The rest of
   `/zfsbootmenu` stays root's, and landlock gives the console that directory alone. `zbm_state` reads them there,
   and `zbm report` adds them as now.
2. **The console tells the supervisor**, which writes the files where they are now. Nothing moves, and the supervisor
   gains writes on behalf of the console.
3. **Files made by the supervisor at the old paths**, owned by the console's user. The screenshots, whose names grow
   with each one, cannot be made in advance.

## Proposal

The first option of each.

**The user.** A fixed user and group, 60600 (`zbm-console`), recorded once in `zbm_platform::sandbox`. Nothing looks
it up by name, so the image needs no `/etc/passwd` entry; the initramfs has root alone, so the number belongs to no one
else.

**The start.** The supervisor starts the console with, in `pre_exec`: its records' directory made and given to its
user, `RLIMIT_MEMLOCK` unlimited, the control socket bound and listening (passed as `--control-fd`, so that the
console never needs `/run/zfsbootmenu`), the broker's end (`--devices-fd`), `/dev/kmsg` opened for writing (passed as
`--kmsg-fd`), and the user change with `CAP_SYS_TTY_CONFIG` kept as ambient. The serial console and the test mirror
are opened by the broker too. The console confines itself as now, with landlock on its records' directory instead of
the state directory and the sockets' directory, and without `CAP_IPC_LOCK`.

**The broker.** A thread of the supervisor answers the console's requests on the socket pair: `Open { kind, path }`, a
path below `/dev` that must be a character device of `kind`'s major and minor range, opened with `O_CLOEXEC |
O_NOCTTY`, and nothing else. It refuses any other path, kind or file type, and logs the refusal. It serves the console
it started and no one else: the socket pair has no name.

**The sockets.** The control socket accepts peers of user 0 (the supervisor, the boot, `zbm console`, the menu) and
refuses others, including the console's own user. A client of the console checks that the socket's owner is the
console's user, so that no other process can stand in for it. The session's socket and the keyholder's are out of the
console's reach: it has neither the directory nor user 0.

**What fails.** If the supervisor cannot start the console as its user (a kernel without ambient capabilities, before
4.3), it starts it as now, user 0 with two capabilities, and logs why; the console's `confined:` line in the kernel
log names its user, and the VM test checks it. The boot asks the console's control socket whether it holds or has
released the display (`status`), instead of reading `console` and `released-console`, which stay for users and hooks
(ADR 0002, point 5).

## Consequences

- A compromise of the terminal parser gets the screen, the key presses and the console's own records: not user 0, not
  the session's socket, not the state the boot trusts, and no file of root's even without landlock.
- The supervisor, in zone Z1, gains the broker: a parser of the console's requests, small and fuzzed, which opens
  character devices of four kinds below `/dev`. It is the one new piece of root code.
- The paths of the screenshots and the scrollback move to `/zfsbootmenu/console/`; `zbm report` finds them as before,
  and `zfsbootmenu(7)` says where they are.
- `CAP_SYS_TTY_CONFIG` stays, held by a user that is not root.
- Nothing changes for the menu or the keyholder, nor on the kernel console (`zbm.console=kernel`), where no console
  runs.

## Alternatives considered

- **Leave the console as user 0 and narrow landlock**: its rules on `/run/zfsbootmenu` and `/zfsbootmenu` become the
  console's socket and files alone. It closes the session's socket and the state files, but not the files of root's
  where landlock is missing, and every peer check still trusts the console.
- **A separate broker process** instead of a thread of the supervisor: one more process of root's to supervise, for no
  gain in what root code runs.
- **`systemd-logind`'s broker itself**: not in the initramfs of mkinitcpio, and not running before dracut's switch
  root.

## How it is checked

- Unit tests of the broker: each kind's major and minor range, a regular file, a symbolic link out of `/dev`, a block
  device and a path with `..` refused; the descriptor it sends opens the node it named.
- Unit tests of the console's seat and keyboards on a fake broker, as on the fake file system now.
- `runtime-rust-console` reads the console's `/proc/<pid>/status`: `Uid` and `Gid` 60600, `CapEff` holding
  `CAP_SYS_TTY_CONFIG` alone, and `Seccomp: 2`. From the recovery shell, the console's records are in
  `/zfsbootmenu/console/`, and the console's `confined:` line names its user.
- `runtime-rust-console` takes the display again after a failed `kexec` (`zbm console reattach`) and plugs in a
  keyboard after the start, through the broker.
- A fuzz target for the broker's requests.

## Where the work stands

Proposed; nothing is built.

## Questions for the maintainer

1. **How the console gets its devices:** the broker in the supervisor, and the console never root (option 1); root at
   the start, then the broker for what comes later (option 2); or groups (option 3)?
2. **The virtual terminal's modes:** `CAP_SYS_TTY_CONFIG` as the console's one capability (option 1), or the terminal
   as its controlling terminal (option 2)?
3. **Locked memory:** `RLIMIT_MEMLOCK` unlimited (option 1), a bound from the scrollback (option 2), or only the
   history's pages locked (option 3)?
4. **Its records:** a directory of its own, `/zfsbootmenu/console/` (option 1); written by the supervisor at the old
   paths (option 2); or files made in advance at the old paths (option 3)?
