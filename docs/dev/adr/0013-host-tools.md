# ADR 0013: The host tools as one crate, and what they keep of the Perl tools

- **Status:** accepted, 1 November 2026 (proposed the same day)
- **Decides:** item DM10 of [ROADMAP.md](../../../ROADMAP.md), and the layer of `zbm-host`, which changes the crate
  map of [ADR 0003](0003-crate-map.md) (decision 14)
- **Governs:** `generate-zbm`, `zbm-kcl` and `zbm-image`; CD12, SE12, SB12; fixes 23, 24 and 25 of the review;
  `xtask/src/deps.rs`

## Context

`generate-zbm` and `zbm-kcl` are Rust ports of a Perl program and a Bash script, written to be checked against them
(Phase 3 of the port plan). The port followed the old programs step by step, so it keeps their machinery:

- **Perl's libraries, emulated:** Getopt::Long 2.54's parsing (`cli.rs`: unique prefixes, `+opt`, `-opt=value`,
  `POSIXLY_CORRECT`), YAML::PP's typing (`config.rs`: `3.10` is the number 3.1, `false` prints as `0`), YAML::PP's
  emitter for `--enable` and `--disable` (`dump/`), File::Glob's `csh_glob` with `:nocase` (`glob.rs`),
  Sort::Versions (`version.rs`), and `File::Basename` and number formatting (`perl.rs`). About 2,700 lines without
  their tests, each with recordings of the Perl results in `tests/golden/legacy/generate-zbm`.
- **A shell for every program:** `generate-zbm` runs dracut, mkinitcpio, `objcopy`, `objdump`, `mount` and the hooks
  through `/bin/sh -c '<command> 2>&1'`, because `Global.DracutFlags` and `Global.InitCPIOFlags` are shell words; every
  path is quoted into a string.
- **Bash's machinery in `zbm-kcl`:** `getopts`, and the legacy tokenizer of command lines, kept until DM10.
- **Writes in place:** `--enable` and `--disable` rewrite `config.yaml` (fix 25, comments lost), `zbm-kcl` rewrites a
  live EFI image (fix 24), and an interrupted build can leave a half-written image beside the good ones.

Since SB8 the plan of an image is Rust (`zbm-image`, [ADR 0006](0006-image-plan.md)). It sits in the orchestration
layer, beside `generate-zbm`, because ADR 0003 puts `zbm-host` among the **front ends**, and `check-deps` forbids front
ends to use adapters (`NOT_THROUGH`: front ends → adapters). The host tools cannot keep that rule: they run dracut,
mkinitcpio, `objcopy`, `zfs` and `mount`, which are adapters' work. The crate map and the rule cannot both hold for
`zbm-host`.

## Decision

**`zbm-host` is one crate in the orchestration layer, with `zbm-image` in it, and two binaries, `generate-zbm` and
`zbm-kcl`. It keeps the tools' contract with their users, their configuration and options, and drops the emulation
of how Perl and Bash happened to implement it.**

1. **The layer** (question 1). `zbm-host` joins orchestration, where `generate-zbm` and `zbm-image` are now, and they
   go into it as modules (`zbm_host::image`, `zbm_host::generate`, `zbm_host::kcl`). The front ends keep their rule.
   ADR 0003's table moves `zbm-host` from the front ends to orchestration; `zbm` keeps `zbm image` by depending on
   `zbm-host`, as it depends on `zbm-startup`. ADR 0003's rule 2, "orchestration does not use adapters", not yet
   enforced, becomes a rule for `zbm-startup`: start-up reaches the system through `zbm-app` and the domain's
   services, while the host tools, which have no `zbm-app`, use the adapters directly.
2. **The pipeline.** `generate-zbm` becomes four steps, each a value or an adapter:
   - **Configuration:** `config.yaml` and the command line, read once into typed settings (`zbm-config`'s option
     table, as now), with every problem reported together.
   - **Plan** (pure): the kernels chosen, the images to write and their names, the commands as argument vectors, the
     image plan of ADR 0006, the versions to prune. `generate-zbm --dry-run` prints it.
   - **Build** (adapters): the builder, `objcopy` (until CD12), the hooks, in a work directory.
   - **Install** (atomic): each image written beside its place under a temporary name, `fsync`ed and renamed; old
     versions pruned only after the new ones are in place. A failed build changes nothing in `ImageDir`.
3. **Compatibility** (question 2). Every key of `config.yaml`, every documented option and every hook directory keep
   their names and meanings. Where the Perl tool's behaviour was an accident of Perl, the new one says what the
   manual says, and `CHANGES-FROM-LEGACY.md` lists each difference: a version such as `3.10` stays `3.10`; `false`
   is false, not `0`; globs in paths match case-sensitively, as the file system does; versions sort as `zbm-boot`
   sorts kernels (uucore's `version_cmp`), and names such as `git-abc` are pruned (fix 25).
4. **Commands without a shell** (question 3). Programs run with argument vectors through the adapters
   (`zbm_tools::builders`, `zbm_tools::efi`, `zbm_tools::mount`). `Global.DracutFlags` and `Global.InitCPIOFlags`
   are split as shell words, quotes and backslashes honoured, without expansion; a flag with `$`, `` ` `` or a glob
   character is refused with the reason, rather than passed on with another meaning.
5. **The command line** (question 4). `generate-zbm` and `zbm-kcl` take the options their manual pages document, in
   the spellings they document, through one parser shared by both. The spellings Getopt::Long and `getopts` also
   took (unique prefixes, `+opt`, `-long`) still work for one release, each with a warning that names the documented
   spelling, and are errors after it.
6. **`--enable` and `--disable`** change the one value of `Global.ManageImages` in the text of `config.yaml`,
   keeping its comments and layout, and write it atomically; a file where that value is not a plain scalar on its
   own line is refused, with the line to change by hand. The YAML::PP emitter goes.
7. **`zbm-kcl`** works on `KernelCommandLine` (`zbm-types`), writes a changed EFI image to a new file and renames it
   over the old (fix 24), and keeps `objcopy` until CD12 writes the sections itself.
8. **Tests.** Golden plans of `generate-zbm` for a set of configurations, as SB8's for images (`--dry-run`'s output,
   under insta); the recordings of the emulated libraries (`cli`, `config`, `dump`, `glob`, `perl`) go with their
   modules. The recordings of whole runs (`parity`) are compared where the result is meant to be the same, and
   their differences listed with point 3. Fix 23 (an initramfs of another prefix renamed without a warning) gets
   its test.

## Consequences

- One crate for what runs on the host, and the plan of an image beside the program that builds it; `zbm-image` and
  `generate-zbm` disappear as crates (25 → 24 with `zbm-kcl`, 23).
- About 2,700 lines of emulation and their recordings go; what replaces them is typed settings, an argument-vector
  adapter and a plan with golden files.
- Builds stop leaving half-written images and rewritten configuration files behind.
- Some configurations behave differently, each listed: the version and boolean forms of point 3 and the shell flags
  of point 4. A configuration that relied on them gets an error or a warning that names the key, not a silent change.
- `zbm` grows by the host tools' code it links for `zbm image` (as with SB8); the boot image does not run it.

## Alternatives considered

- **Keep `zbm-host` a front end, with a named exception for adapters.** Keeps ADR 0003's table, but the exception
  would be the whole crate's work, and the rule would no longer say anything about the host tools.
- **Two crates: a front-end binary crate over an orchestration library.** Keeps both the table and the rule, at the
  cost of a crate that only parses options and calls the library; the binaries are small enough to live in the
  library's `src/bin/`.
- **Keep byte-for-byte parity with the Perl tool.** No user sees a difference, but the emulation stays, and with it
  the quirks the manual never promised (`3.10` as `3.1`).
- **Keep `/bin/sh -c` for the builders.** Users' flags keep any shell meaning, but every path stays quoted into a
  string and every command is one quoting mistake from running something else.

## How it is checked

- `cargo xtask check-deps` with `zbm-host` in orchestration and the front-end rule unchanged.
- Golden plans of `generate-zbm --dry-run`; the parity recordings of whole runs where the results stay the same.
- Tests of the atomic install: a build that fails at each step leaves `ImageDir` and `config.yaml` as they were.
- The VM suites build their images through `generate-zbm` as now.

## Where the work stands

Built on 1 November 2026, the day the decision was accepted (DM10a to DM10g):

- **Points 1 and 7:** `zbm-host` in orchestration, with the modules `generate_zbm`, `kcl` and `image` and the
  binaries `generate-zbm` and `zbm-kcl`; `check-deps` holds it there. `zbm-kcl` works on `KernelCommandLine`, and
  writes a changed EFI image through the same atomic replace as the install (`zbm_host::files`). It keeps `objcopy`
  (CD12).
- **Points 3 and 6:** the configuration is read without Perl's typing, globs are case-sensitive, versions sort with
  `version_cmp`, and `--enable` and `--disable` edit the one line. The emulations of YAML::PP, its emitter,
  File::Glob, Sort::Versions, Getopt::Long and `getopts`, and the legacy tokenizer of `zbm-kcl`, are gone with their
  recordings.
- **Point 4:** every program runs from an argument vector (`zbm_tools::programs::merged`); the flags are split with
  `zbm_platform::shell::split_words`.
- **Point 5:** one parser, `zbm_host::options`, with the warnings of answer 4.
- **Point 2, in part.** The pipeline is not four separate stages. The run keeps the order of the Perl program, and
  every change it makes (a mount, a hook, a build, a copy, a rename, a removal, the edit of `config.yaml`) goes
  through one guard, which `--dry-run` turns into a line that starts with `Would`; the install writes each file
  beside its place and renames it. That gives the dry run and the atomic install the plan was for, with less code
  than a plan as a value built before anything runs; such a plan is not built for the whole run. CD12 made the
  EFI bundle one ([ADR 0014](0014-efi-image.md), point 8): its stub and sections are planned before it is built.
- **Point 8:** the golden runs of `--dry-run` under insta (`tests/parity/snapshots/`), fix 23's test, and the parity
  recordings of whole runs, with the intended differences stated beside them (`Case::differs` for `generate-zbm`,
  `intended()` for `zbm-kcl`).
- `zbm-host` is on the strict lints (`xtask/strict/crates.txt`). It reads and writes the host's files and
  environment itself, which its modules allow with that reason; its tests run it in sandboxes.

## Answers of the maintainer

Accepted as proposed, on 1 November 2026:

1. **The layer:** `zbm-host` in orchestration, with `zbm-image` in it; ADR 0003's rule 2 names `zbm-startup`.
2. **Compatibility:** the same keys and options, with the Perl accidents fixed and listed in
   `CHANGES-FROM-LEGACY.md`.
3. **`Global.DracutFlags` and `Global.InitCPIOFlags`:** shell words without expansion; `$`, `` ` `` and globs refused.
4. **Other spellings of the options:** accepted with a warning that names the documented spelling for one release,
   then an error.
