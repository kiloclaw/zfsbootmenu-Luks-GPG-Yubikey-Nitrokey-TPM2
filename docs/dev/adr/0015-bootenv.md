# ADR 0015: `zbm-bootenv`, and `zbm-boot` without the console

- **Status:** proposed, 3 November 2026
- **Decides:** item DM7 of [ROADMAP.md](../../../ROADMAP.md), and what [ADR 0003](0003-crate-map.md) left of its
  point 3: the crate `zbm-bootenv`, and the end of the `zbm-boot` → `zbm-pool` exception in `SAME_LAYER`
- **Governs:** `zbm-boot`, the new `zbm-bootenv`, the read-write import in `zbm-pool`, `zbm_app`'s
  `Through::Console` and its report, `zbm_startup`'s import operator and graphics; the `Interaction` vocabulary of
  `zbm_types::interaction`; `xtask/src/deps.rs` and `xtask/src/deps/console.rs`

## Context

`zbm-boot` is about 5,300 lines outside its tests, in 25 files, and holds five kinds of code:

- **What a boot environment is:** what ZFS says of a dataset (`boot/zfs.rs`), mounting one (`boot/mounts.rs`), its
  kernels (`kernels.rs`, `boot/discovery.rs`), its command line (`commandline.rs`), where its key comes from and
  loading it (`keys/`), and the hooks, which key loading runs too (`load-key.d`).
- **Booting it:** the sequence from a chosen kernel to `kexec` (`boot/sequence.rs`), the `kexec` load
  (`commandline.rs`'s `KexecLoad`), measured boot (`boot/measured.rs`), the GPUs handed back
  (`graphics/hand_back.rs`), and the logs saved on the ESP before the boot (`saved_logs.rs`).
- **Start-up's:** the import stage with the plain console as its `Operator` (`import.rs`), what the runtime cannot
  handle (`support.rs`), and the graphics policy: which GPUs get a driver and which card the console takes
  (`graphics.rs`, `graphics/known.rs`).
- **The menu's changes:** the read-write import before a snapshot, rollback or new default (`writable.rs`), with the
  hibernation check it needs (`resume.rs`).
- **The bug report** of `zbm debug report` (`report.rs`), and a re-export of the plain console's prompts
  (`prompt.rs`).

Every function takes the `System` bundle, console included, and four places use it:

1. **The resume question** (`resume.rs`) clears the screen, prints the warning and reads the line the user types:
   `NORESUME`, `DANGEROUS`, or anything else, which refuses the read-write import.
2. **The boot** (`boot/sequence.rs`) clears the screen before it mounts the environment, prints `Booting KERNEL for
   ENVIRONMENT ...`, and after a failed `kexec` shows a ten-second message.
3. **Key loading** (`keys/load.rs`) clears the screen and runs `zfs load-key -L prompt`, which asks for the passphrase
   on the terminal itself, three times at most.
4. **The import** (`import.rs`) shows the countdown prompts of the retry loop and of unsupported features.

So `zbm-boot` is the one domain crate `check-deps` lets name the console (`xtask/src/deps/console.rs`, `UNTIL`), and
the one with an edge in its own layer (`zbm-boot` → `zbm-pool`). It depends on `zbm-pool` for four things: the import
stage, the read-write import, the host ID of the command line, and `PoolError` inside `BootError`.

Three crates use it: `zbm-app` (booting, key loading, the read-write import, the kernels), `zbm-startup` (the import
stage, hooks, measured boot, graphics, the start-up boot) and `zbm` (the report, saved logs, and the kernels and
prediction of `zbm measure`). Every module is public; they use about forty of its items.

The `Interaction` port, which `zbm-luks` asks through, has no request for any of these: a `SecretRequest` names a
LUKS volume, a `PromptRequest` is a timed RETURN or ESCAPE, and `Tell` is unlocking's.

## Decision

**`zbm-bootenv` takes what a boot environment is; `zbm-boot` keeps booting it; start-up, the menu's changes and the
report go to their owners; and every function takes a `Context` and asks through the front end's `Interaction`.**

1. **`zbm-bootenv`**, in the domain layer: the dataset queries, mounting a boot environment and unmounting it, its
   kernels and their discovery, its command line (`be_cmdline`, `edited_cmdline`, the `root=` prefix, `os-release`,
   the device tree), key sources, the key cache and key loading, and the hooks. It depends on `zbm-pool` for the
   host ID alone (`SAME_LAYER`: `zbm-bootenv` → `zbm-pool`).
2. **`zbm-boot`** keeps booting: the sequence, the `kexec` load, measured boot, the teardown, the GPU inventory and
   hand-back, the saved logs, `efivarfs` and `securityfs`, and `Recovery`. It depends on `zbm-bootenv`
   (`SAME_LAYER`: `zbm-boot` → `zbm-bootenv`) and no longer on `zbm-pool`.
3. **What leaves both:**
   - the import stage's plain-console `Operator` and `support.rs` go to `zbm-startup`, beside the session's operator
     in `zbm`;
   - the graphics policy (`Choice`, `option`, `load`, `console_card`, the known GPUs) goes to `zbm_startup::graphics`,
     which `zbm` already calls; the GPU inventory stays in `zbm-boot`, which the hand-back needs;
   - the read-write import and the hibernation check go to `zbm-pool` (question 3);
   - the report goes to `zbm-app` (question 1), and `zbm debug report` stays its front end;
   - `prompt.rs` goes: `zbm-app` uses `zbm_tools::console`, and re-exports `ConsoleInteraction` and `TimedPrompt` for
     `zbm` and `zbm-startup`, which use no adapter.
4. **The console leaves the domain.** Every function of both crates takes a `Context`, and each of the four places
   asks through an `Interaction`, with requests that are data in `zbm_types::interaction`:
   - the passphrase of an encryption root is a `SecretRequest` whose subject is a LUKS volume or a ZFS encryption
     root (question 2);
   - the resume question is a new request with three answers, `Noresume`, `Dangerous` and `Refuse`; the plain
     console asks it as now, the warning and then the line typed;
   - `Booting KERNEL for ENVIRONMENT` is a `Tell` of a new kind, which the plain console prints in its colours;
   - the message after a failed `kexec` is a `PromptRequest` with a new subject;
   - the screen is the front end's: `Clear` leaves `KeyLoad`, and the plain console clears before it asks a secret
     or the resume question, as the boot of a chosen kernel does now.
5. **`Through::Console`** carries a `Context`, the front end's `Interaction` and its `Recovery`, where it carries the
   `System` now. In DM7 the menu still steps aside for a boot and answers with the plain console on its terminal, so
   what the user sees does not change; its own dialogs for these requests come with the workspace's (question 4).
6. **A narrow public API.** The modules of both crates are private, and each `lib.rs` names what the other crates use.
   The error enums and the types the crates return are `#[non_exhaustive]`; the types callers build (`BootSettings`,
   `KeyLoad`) get constructors. A metric column counts each crate's public items, with a baseline that may only
   fall (`cargo xtask metrics`).
7. **Error codes keep their numbers.** `KeyNotLoaded` (602), `Mount` (603) and `NoKernel` (604) move to
   `zbm-bootenv`'s table; `ResumeRefused` (606), `ReadWriteProhibited` (607) and `PoolProperty` (608) move with the
   read-write import; the others stay. A code names an error, wherever its crate is, and the journal, the
   documentation and `zbm explain` keep their meaning.
8. **`check-deps`:** `zbm-bootenv` in the domain layer; `SAME_LAYER` loses `zbm-boot` → `zbm-pool` and gains the two
   edges above; `UNTIL` in `console.rs` is empty, so no domain crate names the console.

## Consequences

- No domain crate draws on the screen or reads the keyboard: each request is data a front end shows its own way, and
  the unit tests record and answer them, as `zbm-luks`'s do.
- The menu can ask a ZFS passphrase, the resume question and the boot message in its own dialogs, without stepping
  aside, once it answers these requests; the session over SSH can pass them as it passes LUKS's (PM10).
- `zbm-boot` can no longer import a pool: what booting needs of pools goes through `zbm-bootenv` and the ports.
- SB5 (built-in hooks as native boot steps), and the menu's checks of an environment's ZFS and its boot counting
  (#6, #7), build on `zbm-bootenv`'s narrow API rather than on all of `zbm-boot`.
- Every caller of the boot and key code changes its arguments, and its tests: the work is large, as ROADMAP.md says.
- With question 2's proposal, a ZFS passphrase passes through ZFSBootMenu's memory, as a `Secret` (locked and wiped),
  where `zfs` read it from the terminal itself; LUKS passphrases already do.

## Alternatives considered

- **Keep `zbm-boot` whole and move only the prompts.** Smaller, but `zbm-boot` keeps its `zbm-pool` edge, and SB5
  and the menu's checks would depend on loading and booting to reach a kernel list.
- **One crate per kind** (kernels, keys, command lines). Finer, but the three share the mount of a boot environment
  and the dataset queries, so they would depend on each other in one layer.
- **The console as an argument, not a port** (a `&dyn Console` passed to the four functions). It ends the exception
  in name only: the domain still draws, and the menu could still not answer in its dialogs.
- **The resume question as a timed prompt** (RETURN goes on with `noresume`). Fewer request kinds, but typing
  `DANGEROUS` is the friction the legacy runtime chose before a write that can destroy a pool.

## How it is checked

- `cargo xtask check-deps` with the layers above and `UNTIL` empty; `--bless` regenerates
  [architecture.md](../architecture.md).
- The unit tests of both crates on fakes, with a recording `Interaction`: the passphrase asked for an encryption root
  and refused three times, the resume question's three answers, the boot message, the failed `kexec`.
- The plain console's output for each request against today's (the prompt `Enter passphrase for 'ROOT':`, the
  resume warning, `Booting ... for ...`), so that what users and the VM tests read stays.
- The metric column of public items, and `#[non_exhaustive]` on the error enums.
- The VM tests that boot encrypted environments and answer ZFS's prompts (`complex-badkey`, `runtime-rust-keysource`),
  and the rest of the suite.

## Questions for the maintainer

1. **The report:** in `zbm-app`, beside the snapshot, with `zbm debug report` its front end (proposed); in `zbm`, with
   an exception for it to `NOT_THROUGH`, as it uses three adapters; or assembled in `zbm` from parts each domain crate
   gives (the pools' status, the LUKS headers, the TPM)?
2. **A ZFS passphrase:** asked through `Interaction::secret` and given to `zfs load-key` on its standard input, three
   attempts, with `zfs`'s own prompt text on the plain console (proposed); or `zfs` asks on the terminal as now,
   inside `Interaction::lease_console`, so that the passphrase never passes through ZFSBootMenu?
3. **The read-write import and the hibernation check:** in `zbm-pool`, as an import (proposed); in `zbm-app`, as only
   the menu's changes need it; or left in `zbm-boot`, which then keeps its `zbm-pool` edge?
4. **The menu's dialogs for the new requests:** with the workspace's, after DM7, the menu stepping aside for a boot
   until then (proposed); or in DM7, so that the menu stays on screen through a boot's questions?
