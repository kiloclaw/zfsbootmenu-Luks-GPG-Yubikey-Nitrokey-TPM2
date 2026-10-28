# ADR 0002: Trust zones

- **Status:** proposed, 28 October 2026
- **Decides:** decision 13 of [ROADMAP.md](../../../ROADMAP.md#decisions)
- **Governs:** PM6, PM9, PM10, SB10, SE9, SE11; supersedes, once phase 5 is done, decision 2 of
  [crates/DESIGN.md](../../../crates/DESIGN.md#decisions-for-review) ("the state directory stays the only shared
  state")

## Context

Every process of the runtime runs as root in one zone ([threat-model.md](../threat-model.md#zones-and-data-flows)):
the supervisor, the start-up program, the menu, the console on the display, the key methods and the recovery shell can
each reach every secret and every device. So a bug in any parser of hostile input is a bug with root and the keys:

- the console's terminal parses whatever the programs print, names from a foreign pool (A3) among it;
- the session parses pool and dataset names, properties, files in `/boot`, crypttab files and key devices (A2, A3);
- the start-up program holds the passphrases typed into the start-up view until it drops them, in the same process
  that parses those names.

Hiding nothing from root is not the aim: the boot loader must open volumes and `kexec` a kernel. The aim is that
input an attacker controls is parsed where the secrets are not, and drawn where nothing is privileged.

## Decision

**Four zones, each a process (or kind of process) with the least it needs.**

| Zone | Process | Holds | Parses | Privileges |
| --- | --- | --- | --- | --- |
| Z0 | `zbm-init`, process 1 | The table of services | Nothing untrusted | root; opens no socket |
| Z1 | `zbm-unlock`, the keyholder | Every passphrase, PIN, key file and unsealed key | LUKS headers and tokens, through the key methods | root, for the device mapper and the TPM |
| Z2 | The session | Snapshots, plans, no secret | Pool names, properties, `/boot`, crypttab, hook roots | root, for imports, mounts and `kexec` |
| Z3 | The front ends: the menu, `zbm console` | Cells and key presses | The programs' output (the console's terminal), messages from the session | none once started: `no_new_privs`, an unprivileged user, seccomp, landlock |

1. **Z0 supervises.** It starts the console, the session and the shell, restarts them by policy, and after a crash
   applies a `RecoveryPolicy` (the recovery shell, or the PIN screen of a locked menu). It parses no untrusted input
   and cannot panic (PM6, SE9).
2. **Z1 is the only place with secrets.** The session asks it `UnlockRequest { volume, method, credential }`, and it
   answers `UnlockOutcome`. A credential typed in the menu moves into the request and is wiped after use; it never
   rests in Z2 or Z3. Z1 extends the PCR of measured boot after its last unseal (SB10, SE10).
3. **Z2 parses and decides.** Hostile names and files become checked types (`Untrusted<T>`, then `DatasetName` and
   the rest) here, and every action a client sends is checked against the policy (the locked menu, the key map) before
   it acts.
4. **Z3 draws and reads keys, unprivileged.** The console opens the DRM card, the input devices and the
   pseudo-terminal, then drops its privileges; the menu is a client of the session. A compromise of the terminal
   parser leaks what is on the screen and the key presses, not root and not the keys.
5. **Zones talk over typed messages on Unix sockets** (`SOCK_SEQPACKET`, the peer's credentials checked with
   `SO_PEERCRED`), versioned, defined once in `zbm-app`. Not through PID files, signals or polling: the state
   directory stays for users, hooks and the recovery shell, not for coordination.
6. **Over SSH,** `zfsbootmenu` is a client of the same session, and takes the menu over with a handshake (`Takeover` →
   `Yielded`), not with a `SIGTERM` to whatever the menu runs (PM10).
7. **The zones come in order of need.** Phase 2 makes the session one process with the menu inside it (PM9); phase 5
   splits out the keyholder and drops the front ends' privileges (SB10, SE11). The messages are the same before and
   after, so a split changes the transport, not the code.

## Consequences

- A memory-safety bug in a parser of A2's or A3's input no longer reaches the keys: the parsers run in Z2 and Z3, the
  keys in Z1. Rust makes such bugs rare; the zones make the rare one less costly.
- The threat model gains rows for the messages between zones (new inputs), and the property that no secret is in Z2
  or Z3. The VM test `secrets-passphrase` searches the start-up program's memory before `kexec` today; it searches
  each zone's once they are processes.
- More processes and sockets: the boot gains a few milliseconds and the image a few kilobytes of protocol code. B7
  (the process count) moves the other way overall, as `stty`, `tput` and the shell wrappers go.
- The menu cannot run without the session: a session that crashes takes the screen with it until Z0 restarts it,
  where the menu's state (the selection, the dialogs) is lost.
- Debugging spans processes: `zbm debug` reads every zone, and each reports its health to Z0.

## Alternatives considered

- **One process, as today, hardened.** Least work, but every parser shares the address space with the keys.
- **Sandboxing the external programs instead** (`cryptsetup`, `tpm2_*` and `gpg` under seccomp). They need root and
  the devices to do their job, and the hostile parsing that matters is ours.
- **More zones** (a process per key method, per pool). Each split costs a protocol; four zones cover the attackers of
  the threat model, and finer ones can follow if a review asks for them.
- **A capability-based design** (file descriptors passed to an unprivileged session). Attractive, but `zpool import`,
  `mount` and `kexec_file_load` need root in any case; the session stays privileged and parses into checked types.

## How it is checked

- `docs/dev/threat-model.md` lists each zone, its inputs and its assets, and the property "no secret outside Z1" with
  the VM test that searches memory.
- `cargo xtask check-deps`: only `zbm-unlock`'s crates may depend on the key methods; front-end crates reach the
  system only through `zbm-app`.
- A VM test reads `/proc/<pid>/status` of each front end: no capabilities, `NoNewPrivs: 1`, `Seccomp: 2`.
- Fuzz targets for every message decoder between zones (SE5).

## Questions for the maintainer

1. **The recovery shell's zone.** It is root by design, and can do anything the zones prevent. Proposed: it stays in
   Z0's care, started only through `ShellGate::open(&Authorized)` (SE9), so a locked menu (#8) keeps it closed.
2. **Hooks.** User hooks run as root in Z2 today, with the session's environment. Proposed: they stay there, as users
   expect, and the threat model keeps them as trusted by the user who installed them; what A3 can do is name a hook
   root, which SE10 refuses on a pool imported under another host's ID.
3. **SSH logins.** Proposed: a login shell is a front end in Z3 with the menu as its program, and reaches the recovery
   shell only through the same gate.
