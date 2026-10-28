# ADR 0001: The process model and concurrency

- **Status:** accepted, 28 October 2026 (proposed the same day)
- **Decides:** decision 10 of [ROADMAP.md](../../../ROADMAP.md#decisions); item PM1
- **Governs:** PM2, PM4, PM6, PM8, PM9, PM10

## Context

The Rust runtime keeps the legacy process structure ([crates/DESIGN.md](../../../crates/DESIGN.md#process-model)):
a supervisor as process 1, the start-up program, the menu, the console on the display, the shell, and `zluks unlock`
on the plain console. Inside each, work is done in a straight line:

- The start-up program runs its stages one after another, and a stage blocks while its commands run.
- The menu's loop waits for a key or a tick (200 ms), and calls the machine in between. While the machine imports a
  pool, opens a LUKS volume or derives an Argon2 key, the loop does not run: the screen does not redraw, the countdown
  does not count and nothing can be cancelled.
- Threads exist only around a command: a watchdog per time-out and a reader per output stream (SB1).
- Processes coordinate through the state directory, with `flock` locks since PM5, and through signals, which PM3 made
  data: blocked, and read from a `signalfd`. `zbm console release` still learns that the console is done by looking
  at its records every 20 ms.

The work ahead needs more than one thing at a time in a process: the menu drawing while the session works (PM8), the
supervisor waiting on children, signals and restarts at once (PM6), cancelling a command (PM4), and running checks in
parallel without running out of memory (Argon2 takes about 1 GiB per derivation).

## Decision

**Threads and channels, one reactor per process, no async runtime.**

1. **One reactor per process.** One thread owns the process's state. It blocks in a single `poll` over every source
   of input the process has: the terminal or the keyboards, a `Mailbox`, the `signalfd`, a `pidfd` per child it
   waits for, its sockets, the uevent netlink socket where it needs one. The poll's time-out is the nearest
   deadline (a tick, a countdown, a command's time-out). It handles what came, as a message, and polls again.
2. **Workers own nothing global.** Long work runs on worker threads. A worker takes its input by value or behind an
   `Arc`, and sends typed messages back through the reactor's `Mailbox`: an `eventfd` that wakes the poll, paired with
   a bounded `std::sync::mpsc` channel. It never touches the reactor's state, and holds no lock the reactor waits on.
   Workers start through `spawn_traced`, which carries the current span and the kernel log's settings (PM2).
3. **Typed messages.** Each channel has its own enum (`Command`, `Event`, `Reply<T>`). A question from the domain to
   the user is a request with a one-shot reply channel, not a nested event loop (`Interaction`, PM8). Between
   processes the same types travel serialized ([ADR 0002](0002-trust-zones.md), [ADR 0004](0004-client-channel.md)).
4. **No async runtime** in the runtime's crates: no `tokio`, `async-std`, `smol` or `futures` executors, and no
   `async fn` in their APIs. `brush` uses `tokio` for its jobs; that stays inside `zbm-brush`, its own binary.
5. **Cancellation and time-outs.** Every command gets a `CancelToken`. On cancel or time-out, the `Runner` kills the
   command's process group through its `pidfd`, and waits on that `pidfd` instead of a watchdog thread (PM4).
   Work in the process, such as a key derivation, checks its token between steps; its result is dropped when it comes
   back cancelled.
6. **Bounded parallelism.** Parallel work runs in `std::thread::scope` on a `WorkPool` with a fixed number of threads,
   under a `MemoryBudget` whose RAII permits are sized by the work (an Argon2 permit is its memory cost). The TPM is
   one `Mutex<Tpm>`, so PIN attempts stay one at a time by construction.
7. **Signals only as data.** A program blocks the signals it waits for before it starts a thread, and reads them in its
   reactor from a `signalfd`; children start with an empty mask (`zbm_sys::Signals`, `prepare_child`; PM3, done).
8. **Ports are `Send + Sync`.** The fakes use `Mutex` instead of `RefCell` (PM2). Tracing and the kernel log are set up
   once per process.

The reactor's parts are in `zbm_platform::event` (PM4): `Mailbox`, `Reactor` (the poll set and its deadlines),
`CancelToken`, `WorkPool`, `MemoryBudget`.

## Consequences

- The menu redraws, counts down and takes keys while the session imports, unlocks or loads; a slow device or a long
  derivation no longer freezes the screen, and Escape can cancel it.
- A reactor is a function from messages to a new state and effects, so it is tested by feeding it messages, without
  threads or time; workers are tested alone.
- Every port and fake must be `Send + Sync`: PM2 changes the fakes from `RefCell` to `Mutex`, and code that kept a
  `&dyn Port` in a `!Send` struct changes with them.
- Blocking calls stay blocking, on workers: no executor to feed, no colouring of functions, and backtraces that read
  as the code does.
- A worker cannot be stopped in the middle of a system call. Commands are processes, which can be killed; work in the
  process is written in steps that check their token.
- Each process has a loop of its own, but built from the same parts, so the supervisor, the session, the console and
  the menu share one implementation of polling, deadlines and wake-ups.

## Alternatives considered

- **An async runtime (`tokio`).** The runtime has a handful of concurrent activities, not thousands, and most of its
  waits are on child processes and devices, which `tokio` runs on a blocking thread pool anyway. It would add a large
  dependency to every process, two ways of writing every port, and a runtime whose failure modes (a blocked executor,
  a dropped future) are harder to see in a boot loader. `brush` brings `tokio` today; measuring it (SB9) is a reason
  to keep it in its own binary, not to spread it.
- **Threads sharing state behind locks.** Simple to start, but every lock is a place for a deadlock or a torn update,
  and the state of the menu and the session would have no single owner to reason about.
- **The current straight-line loops.** They are what freezes the screen today; adding threads to them piecemeal leads
  to the previous alternative.
- **An actor framework crate.** The parts needed are small (a channel, an `eventfd`, a poll set), and a framework
  would bring its own executor or its own model of supervision.

## How it is checked

- `cargo xtask check-deps` refuses `tokio`, `async-std`, `smol` and `futures-executor` in every crate but
  `zbm-brush`, and `deny.toml` bans them where it can.
- The workspace's clippy configuration disallows `std::thread::spawn` outside `zbm_platform::event`, so workers start
  through `spawn_traced`.
- PM8's test: the menu on a fake session whose import takes a minute of fake time keeps drawing and counting, and
  Escape cancels it.
- PM4's test: a command past its time-out is killed through its `pidfd`, with no thread left behind.

## Notes from the implementation

- **PM2** (28 October 2026): the ports of the system (`Runner`, `Files`, `Clock`, `Console`, `Processes`,
  `Environment`) are `Send + Sync`, and so are their fakes. `Interaction` is not yet: the menu answers it on the
  thread that draws, and PM8 makes its questions requests with replies. The kernel log stays installed for a thread
  and a scope, as the start-up stages install it with their own level and state directory; `spawn_traced` carries
  it, with the current span, into every worker that thread starts, so a process logs to one place whichever thread
  writes.

## Answers of the maintainer

Accepted as proposed, on 28 October 2026:

1. **The mailbox's bound:** 256 messages, the sender blocking when it is full, so a flood of events slows the worker
   instead of growing memory.
2. **Deadlines:** in the poll's time-out, computed from the nearest deadline; a `timerfd` only if the supervisor
   needs timers that survive a long wait elsewhere.
3. **`async` in `zbm-brush`:** allowed there only, as brush's own; our code in that crate stays synchronous.
