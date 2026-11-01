# ADR 0014: The EFI image written by `zbm-host`, without `objcopy`

- **Status:** proposed, 1 November 2026
- **Decides:** item CD12 of [ROADMAP.md](../../../ROADMAP.md), the `objcopy` row of [ADR 0005](0005-c-dependencies.md)'s
  programs, and the part of [ADR 0013](0013-host-tools.md)'s point 2 left open: the bundle's layout as a value before
  anything is written
- **Governs:** `zbm_host::generate_zbm` (the EFI bundle), `zbm_host::kcl` (the `.cmdline` section), `zbm_tools::efi`,
  `zbm_testkit::efi`; SE12's signing, which builds on it

## Context

An EFI bundle is the systemd EFI stub with sections added: `.osrel`, `.cmdline`, `.splash`, `.dtb`, `.initrd` and
`.linux`, in that order. The stub finds them by name in the loaded image and hands the kernel the initramfs and the
command line. Both host tools reach the sections through binutils, and read binutils' text:

- **`generate-zbm`** reads the stub's section alignment from `objdump -p`, the end of its last section from the last
  line of `objdump -w -h`, and builds the bundle with one `objcopy` call, an `--add-section` and a
  `--change-section-vma` per section, each placed at the end of the one before, rounded up to the alignment.
- **`zbm-kcl`** dumps `.cmdline` with `objcopy --dump-section`, finds the room before the next section in
  `objdump -h -w`, then removes the section and adds the new one at the same address with two more `objcopy` calls.
  A command line longer than that room is refused, and an image without `.cmdline` cannot get one. The arithmetic
  is still the Bash script's: 64-bit wrapping, addresses printed as two's complement.

`objcopy` does more than add sections. On the x86-64 stub of systemd 255 (`systemd-boot-efi`, 1 November 2026) it
rewrites the stub's own headers: the PE header moves behind binutils' DOS program (from offset `0x40` to `0x80`), the
linker version becomes binutils' (2.42), `SizeOfCode` and `SizeOfInitializedData` are recomputed from the raw sizes,
the checksum is computed, and the time stamp is the current time unless `SOURCE_DATE_EPOCH` is set. The sections it
adds are initialized, readable data (`0x40000040`), with their raw data at the file alignment. The stub's 1 KiB of
headers has room for 11 more section headers; six are needed.

So the image depends on binutils on the host, on the text format of `objdump`, and on a rewrite of the stub that
nothing asks for. The `object` crate, which reads PE32 and PE32+ images,
is already in `Cargo.lock` with its PE reader enabled (brush → `human-panic` → `backtrace`).

## Decision

**`zbm-host` reads and writes the EFI image itself, as a stub with sections added. The stub's own headers and
sections stay as the stub has them; the added sections are laid out as now. `objcopy` and `objdump` leave both tools.**

1. **One module, `zbm_host::pe`**, pure: bytes in, bytes out. It reads a PE32 or PE32+ image into the stub's headers
   (kept as bytes), its section table and the sections, and writes an image from them. The files are read and
   written by the build and install steps, as now. Reading is bounds-checked and refuses, with the reason and the
   file's name, an image that is not an EFI application (subsystem 10 to 13), whose sections overlap or lie outside
   the file, or whose section names are longer than 8 bytes. The parsing goes through `object` (question 1).
2. **The layout of the added sections** is the one `generate-zbm` uses now, so that the parity recordings, which
   compare the added sections and their addresses relative to the first, still hold: the first section at the end of
   the stub's last one, rounded up to `SectionAlignment`; each one after the one before; `.linux` last, so that the
   kernel can decompress without overwriting the others. Each added section is initialized, readable data
   (`0x40000040`), with its virtual size the length of its data and its raw data at `FileAlignment`, padded with
   zeros.
3. **The headers.** `NumberOfSections`, `SizeOfImage`, `SizeOfInitializedData` and the checksum are updated; the rest
   stays as the stub has it, without binutils' DOS program or linker version. The time stamp is `SOURCE_DATE_EPOCH`
   when it is set, and the stub's own otherwise, so that two builds from the same files give the same bytes without
   being asked to.
4. **When the section table has no room** for the new headers (question 2), the headers grow by `FileAlignment` and
   the raw data of every section moves down by as much, with the file offsets that name it: the section table and
   the debug directory's entries. No address in the loaded image changes.
5. **A signed stub.** A certificate table is dropped, with a warning: a signature over the stub does not cover the
   bundle. The bundle is signed after it is built, by `sbsign` today and by `zbm-host` with SE12.
6. **`zbm-kcl`** reads `.cmdline` from the section table and replaces it in place when the new line fits in the room
   before the next section. Otherwise, and for an image without `.cmdline`, the sections after it are laid out again
   (question 4); `-d` removes the section and its header. The shell arithmetic and its messages about offsets go.
7. **`objcopy`** leaves both tools, with `zbm_tools::efi` (question 3). binutils stays a test dependency, to check
   the writer against.
8. **The bundle as a value.** The layout of the bundle (the stub, each section with its file, address and size) is
   computed before anything is written (`BundlePlan`), from the files the build produced; the writer only carries it
   out. `--dry-run` shows the sections it knows: those whose files exist before the build.

## Consequences

- Building an EFI image needs no binutils on the host; one program fewer in ADR 0005's list. The build container
  keeps binutils for `strip`, which dracut and its own build of `zbm` use.
- Nothing parses the text of `objdump`; the image is read as a structure, with its errors named.
- Bundles differ in bytes from those `objcopy` built: the stub's headers stay the stub's, and the time stamp is the
  stub's unless `SOURCE_DATE_EPOCH` sets it. What the firmware and the stub read (the section table, the sections,
  `SizeOfImage`, the entry point) is the same.
- `zbm-kcl` can lengthen a command line, and give one to an image without it.
- The model is what SE12 needs to sign an image (Authenticode hashes the headers and sections), and what new
  sections of the UKI format (`.uname`, `.sbat`, `.pcrsig`) need when they come.
- A stub with a layout the writer refuses stops the build, where `objcopy` might have produced something; the error
  names the stub and the reason.

## Alternatives considered

- **Keep `objcopy`.** It works, and it has written every bundle so far. It keeps binutils on the host, the parsing of
  `objdump`'s text, the rewrite of the stub's headers, and `zbm-kcl`'s limit on the command line.
- **systemd's `ukify`.** It builds unified kernel images with `pefile`, and knows the newer sections. It needs Python
  and systemd 253 or later on the host, and makes the bundle's layout systemd's to change.
- **`object`'s PE writer** (`write::pe`), which builds a whole image. It would re-emit the stub's code, relocations and
  directories rather than keep them, which is more to get wrong than appending, and needs `object`'s write features
  (`crc32fast`, `indexmap`).
- **A reader of our own, without a crate.** `zbm_testkit::efi` reads section tables in 60 lines; a reader that
  validates PE32 and PE32+ headers, data directories and bounds is a few hundred, and `object` is already built.

## How it is checked

- Unit tests on small PE images built in the tests, PE32 and PE32+: sections added, headers updated, a table without
  room grown, a certificate table dropped, every refusal of point 1.
- Property tests: for any sizes of the sections, the layout has no overlap, every section lies inside `SizeOfImage`,
  and the image read back gives the same sections.
- Against `objcopy`: for the stubs on the build host and in the build container, the bundle the writer builds and the
  one `objcopy` builds from the same files have the same section table (names, addresses, virtual sizes, data) and
  `SizeOfImage`, and differ in the listed header fields only.
- A fuzz target for the reader: `zbm-kcl` reads images from any file and from standard input.
- The parity recordings of `generate-zbm` and `zbm-kcl` hold, but for the intended differences of point 6, listed
  beside them.
- The VM tests of the `firmware` group boot the bundle as the firmware's fallback loader under UEFI.

## Where the work stands

On 1 November 2026, nothing of it is built. `generate-zbm` and `zbm-kcl` use `objcopy` and `objdump` through
`zbm_tools::efi` and `Invocation`s, and `zbm_testkit::efi` reads section tables for the parity tests.

## Questions for the maintainer

1. **Reading PE images:** the `object` crate with its PE reader alone, already in `Cargo.lock` (proposed); a reader
   of our own, grown from `zbm_testkit::efi`; or `goblin`?
2. **A stub whose section table has no room:** move the sections' data down and grow the headers, as `objcopy` does
   (proposed); or refuse the stub with the reason?
3. **`objcopy`:** removed from both tools, kept to check against in the tests (proposed); kept for one release behind
   a setting (`EFI.Assembler: objcopy`); or used when the writer refuses a stub?
4. **`zbm-kcl` and a longer command line:** lay out the sections after `.cmdline` again, so that a longer line and an
   image without `.cmdline` both work (proposed); or refuse as now?
