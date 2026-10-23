# Spleen console fonts

Spleen 2.2.0 by Frederic Cambus, under the BSD 2-clause licence in `LICENSE`, for images built with
`zfsbootmenu_console_font=spleen` (`Global.ConsoleFont: spleen` in `config.yaml`) and the Rust runtime (ROADMAP.md
#20). The image build installs them as 256-glyph subsets, with `LICENSE` as
`/usr/share/zfsbootmenu/fonts/LICENSE.spleen`.

| File | From |
| --- | --- |
| `spleen-8x16.psf` | `spleen-8x16.psfu` |
| `spleen-12x24.psf` | `spleen-12x24.psfu` |
| `spleen-16x32.psf` | `spleen-16x32.psfu` |
| `spleen-32x64.psf` | `spleen-32x64.psfu` |
| `LICENSE` | `LICENSE` |

The files are unchanged copies from the release tarball,
<https://github.com/fcambus/spleen/releases/download/2.2.0/spleen-2.2.0.tar.gz> (660902 bytes, SHA-256
`ec42925c6b56d2138c862b2f97147c872e472f674bf03423417d827a08d69a89`, the checksum the FreeBSD and OpenBSD ports
record), renamed from `.psfu` to `.psf`, the extension the image's font tools look for. The 5×8 and 6×12 sizes are
left out: they lack `Œ œ Ÿ`, which French needs (#18), and are too small for any display the autosize picks them for.
