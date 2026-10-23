# DejaVu Sans Mono

DejaVu Sans Mono 2.37, under the Bitstream Vera licence with the DejaVu changes in the public domain (`LICENSE`), for
the console on the display of images built with the Rust runtime (`zbm.console=drm`, ROADMAP.md #19, step 3). The
image build installs it as `/usr/share/zfsbootmenu/fonts/DejaVuSansMono.ttf`, with `LICENSE` as
`/usr/share/zfsbootmenu/fonts/LICENSE.dejavu`. It covers Latin, Greek and Cyrillic, the box drawing and block
characters and the arrows and shapes the menu uses, so the console needs no 512-glyph subset for the languages of
#18.

| File | From |
| --- | --- |
| `DejaVuSansMono.ttf` | `ttf/DejaVuSansMono.ttf` |
| `LICENSE` | `LICENSE` |

The files are unchanged copies from the release tarball,
<https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2> (5429777
bytes, SHA-256 `fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7`, the checksum the FreeBSD port
records). `DejaVuSansMono.ttf` is 340712 bytes, SHA-256
`b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582`. The bold face is left out: the console draws bold
text bright, as the kernel console does.
