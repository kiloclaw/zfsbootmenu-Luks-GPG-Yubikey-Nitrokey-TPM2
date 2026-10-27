# Dependencies

Every third-party crate the workspace uses is vendored in `vendor/`, so that builds, including container builds,
need no network (`.cargo/config.toml` replaces crates.io with that directory). Refresh it after changing a dependency
in `Cargo.toml`:

```sh
cargo vendor --versioned-dirs --sync fuzz/Cargo.toml vendor
cargo deny check
```

`cargo vendor` owns `vendor/` and deletes any other file in it, which is why this page lives here. The fuzz targets
have a lock file of their own: after changing the dependencies of a crate they use, refresh it with
`cargo check --manifest-path fuzz/Cargo.toml --offline` (`cargo xtask lint` fails until then).

Each crate a workspace crate depends on directly is listed below with the reason it is used, and allowed for that
crate in `xtask/src/deps.rs` (`THIRD_PARTY`), which `cargo xtask check-deps` enforces; `cargo deny check` checks their
licences and advisories (`deny.toml`). Adding one is a decision for the pull request; ROADMAP.md
("Decisions") lists the crates agreed for the refactoring, with their versions and features pinned in
`[workspace.dependencies]`.

## The boot runtime (`zbm`)

| Crate | Used by | Why |
| --- | --- | --- |
| `thiserror` | `zbm-types`, `zbm-trace`, `zbm-crypt`, `zbm-zfs`, `zbm-luks`, `zbm-pool`, `zbm-app`, `zbm-boot`, `zbm-startup` | Error enums with their causes; runs at build time only |
| `tracing`, `tracing-subscriber` | `zbm-trace`, `zbm-platform`, `zbm-crypt`, `zbm-luks`, `zbm-app`; `tracing` in `zbm` and `zbm-boot` | Spans and events; the stderr output and the JSON journal |
| `serde_json` | `zbm-trace`, `zbm-zfs`, `zbm-crypt` | The JSON journal; the JSON output of `zfs` and `zpool`; LUKS2 metadata from `cryptsetup` |
| `serde` | `zbm-zfs` | The shape of that output, derived (`derive`) |
| `ratatui` (with `crossterm`) | `zbm-tui` | The menu: layout, widgets, terminal output and key input |
| `fluent-bundle`, `unic-langid` | `zbm-tui` | The menu's message catalogue ([text.md](menu-design/text.md)): Project Fluent's bundle, with plurals and attributes, and its language identifiers. Already built for `uucore`, which takes them through `fluent`; Apache-2.0 or MIT |
| `nucleo-matcher` | `zbm-tui` | Fuzzy matching of the filter text, the matcher of Helix's Nucleo; MPL-2.0 (see `deny.toml`) |
| `uucore` (`version-cmp`) | `zbm-types` | Version order, the `sort -V` of uutils coreutils, the images' coreutils. Its required dependencies (`clap`, `fluent`, `nix` and others) are built, but link-time optimization leaves only `version_cmp` in `zbm`: 6.8 KB of a 2.2 MB stripped binary |
| `drm` | `zbm-display` | The display without a terminal (ROADMAP.md #19, step 1): DRM dumb buffers, modes and outputs, in safe Rust over `rustix`, which the workspace already has. Smithay's crate, MIT. Pinned to 0.15.0 |
| `drm-ffi`, `drm-sys`, `drm-fourcc`, `bytemuck`, `bytemuck_derive` | (`drm`) | Its dependencies: the ioctl structures and calls, pixel format codes, and plain-data casts (`bytemuck_derive` runs at build time only). `drm-sys` brings an older `linux-raw-sys` (0.9) beside the 0.12 of `rustix` |
| `alacritty_terminal` | `zbm-console` | The console on the display (ROADMAP.md #19, step 2, `zbm.console=drm`): Alacritty's terminal state machine, with `vte`'s escape sequence parser, on a pseudo-terminal. Apache-2.0. Pinned to 0.26.0 without its default `serde` feature. It and `vte` 0.15.0 carry patches of ours for the Linux console's own sequences, blinking text, the reverse screen and the terminal's name ([patches/README.md](../../patches/README.md)) |
| `base64`, `home`, `parking_lot`, `polling`, `regex-automata`, `rustix-openpty`, `signal-hook` 0.4, `cursor-icon` | (`alacritty_terminal`) | Its dependencies; `polling`, `rustix-openpty` and `home` serve its own event loop and process spawning, which `zbm-console` does not use, and link-time optimization drops them |
| `swash` | `zbm-display` | TrueType and OpenType fonts for the console on the display (ROADMAP.md #19, step 3): glyphs hinted to the pixel grid and rasterized as coverage masks. Linebender's crate, MIT or Apache-2.0. Pinned to 0.2.10 without its default features but `std`, `scale` and `render`. Chosen over `fontdue`, whose parser `ttf-parser` is unmaintained (RUSTSEC-2026-0192) |
| `skrifa`, `read-fonts`, `font-types`, `yazi`, `zeno`, `once_cell` | (`swash`) | Its dependencies: Google Fonts' font parser (the replacement the advisory names), decompression of compressed glyph data, and the rasterizer |
| `miniz_oxide` | `zbm-display` | The compression of PNG files: the console's screenshots, and the reference images of the snapshot tests. Already vendored for `backtrace`, which `brush-shell` uses; pure Rust, MIT, Zlib or Apache-2.0 |
| `kbvm` | `zbm-display` | Keyboard layouts for the console on the display (ROADMAP.md #19, step 4): XKB keymaps, compiled into self-contained keymap files when the image is built and loaded at boot, the key state machine (modifiers, Caps Lock, level and group switching) and dead keys through a compose table. The XKB engine of the Jay compositor, MIT or Apache-2.0. Pinned to 0.1.7 with only its `compose` feature. Chosen over `xkbcommon-rs`, which was last released in February 2025 |
| `bstr`, `indexmap`, `linearize`, `linearize-derive`, `kbvm-proc`, `isnt`, `debug-fn`, `secure-execution`, `pkg-config`, `version_check` | (`kbvm`) | Its dependencies: byte strings and ordered maps for the XKB parser, enum-indexed arrays, its procedural macros, and the check that keeps environment variables out of set-user-ID programs; `pkg-config` and `version_check` run at build time only, to find the build host's XKB files |
| `rustix` | `zbm-console` | The pseudo-terminal (`pty`), its size and the kernel console's raw mode (`termios`), and the signal of the hand-over (`process`), in safe Rust; already in the workspace through `drm` |
| `signal-hook` | `zbm-console` | `SIGUSR1`, the request to hand the display back before `kexec`, as a flag; the version `alacritty_terminal` brings |
| `brush-shell` | `zbm-brush` | The shell of the Rust images, `brush`: the recovery shell, the shell that runs hooks and the shell of SSH sessions, in place of GNU Bash, which stays only as dracut's `/bin/sh`. Bash-compatible, written in Rust, MIT. Pinned to 0.4.0 with its default features (the reedline, basic and minimal line editors). Its parser, `brush-parser`, carries a patch of ours for `case` patterns inside `$( )` ([patches/README.md](../../patches/README.md)) |
| `brush-core`, `brush-parser`, `brush-builtins`, `brush-interactive`, `reedline`, `nu-ansi-term`, `tokio`, `nix`, `fancy-regex`, `chrono`, `terminfo`, `procfs`, `sysinfo` and others | (`brush-shell`) | Its dependencies: the interpreter, its parser (a PEG grammar, `peg`) and built-in commands, the line editor with completion and history, an asynchronous runtime for jobs, regular expressions for `[[ =~ ]]`, and process and system information for its built-ins. About 190 crates, which `cargo tree -e normal -p zbm-brush` lists. `terminfo` is WTFPL and `git-version`, run at build time, BSD-2-Clause: both permissive, allowed for those crates in `deny.toml` |

Their own dependencies are vendored with them; `cargo tree -e normal -p zbm` lists everything that is compiled into
`zbm`. Some vendored crates are never compiled for Linux: `windows-sys`, `winapi`, `wasi`, `redox_syscall` and the
like, which `Cargo.lock` records for other platforms, and proc-macro helpers such as `darling` and `derive_more`,
which run at build time only.

## Host tools

| Crate | Used by | Why |
| --- | --- | --- |
| `yaml-rust2` | `generate-zbm` | Reads `/etc/zfsbootmenu/config.yaml` as the Perl `YAML::PP` did |
| `hashlink`, `hashbrown`, `foldhash`, `arraydeque` | (`yaml-rust2`) | Its dependencies |

## Tests

These are development dependencies only, allowed for every crate (`TEST_THIRD_PARTY` in `xtask/src/deps.rs`).

| Crate | Why |
| --- | --- |
| `insta` | Snapshot tests: the menu's screens (`crates/zbm-tui/src/snapshots/`) and the console's reference images (`crates/zbm-console/src/screen/tests/reference/`) |
| `proptest` | Property tests: parsing never panics, valid values round-trip (`zbm-types`) |
| `pretty_assertions` | Readable diffs when an assertion of equality fails |
| `serde`, `serde_json` | The golden files of the legacy results (`zbm_testkit::parity`, in `tests/golden/legacy/`); `serde`'s `derive` describes a test's recorded result |

## Fuzzing

`fuzz/` is a workspace of its own, so these are never built with the rest.

| Crate | Why |
| --- | --- |
| `libfuzzer-sys` | Links libFuzzer and provides `fuzz_target!`, as `cargo fuzz` expects |
| `arbitrary`, `cc`, `jobserver`, `shlex`, `find-msvc-tools` | Its dependencies: input types, and building libFuzzer's C++ |

## Developer tooling

Used by the developer tools `xtask` and `zbm-vmtest` (the VM test harness), never by an image or a host tool.

| Crate | Used by | Why |
| --- | --- | --- |
| `syn`, `proc-macro2`, `unicode-ident` | `xtask` | Parse Rust source for `cargo xtask metrics` |
| `serde_json` | `xtask`, `zbm-vmtest` | Read `cargo metadata` and clippy's JSON messages, write `target/metrics.json`; the VM tests' `result.json` and `sfdisk --json` |
| `vt100` | `zbm-vmtest` | A terminal emulator for the serial console: the screen as a user sees it, where Ratatui moves the cursor over blank cells instead of writing spaces |
| `vte`, `arrayvec`, `itoa`, `unicode-width` | (`vt100`) | Its dependencies: the escape sequence parser |
| `regex` | `zbm-vmtest` | The patterns the scenarios wait for on the console, as `pexpect` matched them |
| `regex-automata`, `regex-syntax`, `aho-corasick`, `memchr` | (`regex`) | Its dependencies |
| `thiserror` | `zbm-vmtest` | Its error enum |
| `rustix` | `zbm-vmtest` | `flock` on each test image, so that harness processes run in parallel build it once; already in the workspace for `zbm-console` |

## Agreed, not used yet

Pinned in `[workspace.dependencies]` and vendored when a crate first uses them: `serde` with `derive` for
`zbm-crypt` (which reads JSON through `serde_json::Value` so far). `crossterm` is pinned for direct use but
reached through `ratatui::crossterm` so far, which keeps one version.

## Vendored but never built

`serde_json` names `serde`, and `serde_core` names `serde_derive`, as dependencies for a target that never matches
(`cfg(any())`), to keep the versions of the serde crates in step. Cargo records them in `Cargo.lock` and therefore
vendors them, with what `serde_derive` needs: `quote` and `syn` 3. None of them is compiled.
