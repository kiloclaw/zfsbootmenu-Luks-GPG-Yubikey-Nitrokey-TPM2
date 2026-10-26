# DejaVu Sans Mono

DejaVu Sans Mono 2.37, under the Bitstream Vera licence with the DejaVu changes in the public domain (`LICENSE`), for
the console on the display of images built with the Rust runtime (`zbm.console=drm`, ROADMAP.md #19, step 3). The
image build installs the faces in `/usr/share/zfsbootmenu/fonts/`, with `LICENSE` as
`/usr/share/zfsbootmenu/fonts/LICENSE.dejavu`. It covers Latin, Greek and Cyrillic, the box drawing and block
characters and the arrows and shapes the menu uses, so the console needs no 512-glyph subset for the languages of
#18.

| File | From | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `DejaVuSansMono.ttf` | `ttf/DejaVuSansMono.ttf` | 340712 | `b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582` |
| `DejaVuSansMono-Bold.ttf` | `ttf/DejaVuSansMono-Bold.ttf` | 331992 | `bce60f1b4421acd9ea51ba6623d7024ecbe6817a953e3654df62a5e6bdf8f769` |
| `DejaVuSansMono-Oblique.ttf` | `ttf/DejaVuSansMono-Oblique.ttf` | 251932 | `742097840c541870e8d6dc5c9b37bb1ceeea6c0dedd1d475faf903ef9df734b0` |
| `DejaVuSansMono-BoldOblique.ttf` | `ttf/DejaVuSansMono-BoldOblique.ttf` | 253580 | `91713a71d550bba22c2a6b2bb2a9ad8f9a159e12e4e9f0a5b2677998ba21213e` |
| `LICENSE` | `LICENSE` | | |

The files are unchanged copies from the release tarball,
<https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2> (5429777
bytes, SHA-256 `fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7`, the checksum the FreeBSD port
records). The bold, oblique and bold oblique faces draw bold and italic text (console roadmap C5); images install them
unless `zfsbootmenu_console_faces=regular`, and bold text is drawn bright as well, as the kernel console does.
