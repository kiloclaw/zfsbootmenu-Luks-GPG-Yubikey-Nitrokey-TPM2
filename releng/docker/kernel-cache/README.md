# Pinned kernel package cache

`../cache-kernel.sh` downloads the kernel pinned in `../kernel-pin` (package
and headers) into this directory, verifies them against the SHA-256 sums in
the Void repository index, records those sums in `SHA256SUMS` and creates a
local XBPS repository index.

When an index is present here, `../Dockerfile` and `../image-build.sh` use
this directory as the highest-priority package repository, so the pinned
kernel remains installable after Void's mirrors have moved on to a newer
build. The packages are not tracked by git; back up this directory (or a
tarball of it) if you need reproducible builds in the future.
