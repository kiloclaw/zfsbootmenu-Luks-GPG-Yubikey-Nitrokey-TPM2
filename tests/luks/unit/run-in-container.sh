#!/bin/bash
# Runs the LUKS unit suites inside the ZFSBootMenu build container, so that they
# use the cryptsetup, systemd, GnuPG, tpm2-tools and clevis versions that end
# up in ZFSBootMenu images.
#
# Usage: run-in-container.sh
#
# Environment:
#   ZBMTEST_IMAGE    build container image (default localhost/zbm-builder)
#   ZBMTEST_RUNTIME  docker or podman (default: docker if installed)
#   ZBMTEST_IMPL     legacy or rust, as for run-all.sh; for rust, build the test
#                    build of zbm on the host first (see impl.sh), as the
#                    source tree is mounted read-only
#
# The container runs privileged with the host's /dev, because the suites use
# loop devices; see docs/luks/testing.rst.

repo="$( cd "$( dirname "$0" )/../../.." && pwd )"
image="${ZBMTEST_IMAGE:-localhost/zbm-builder}"
runtime="${ZBMTEST_RUNTIME:-}"
if [ -z "${runtime}" ]; then
  if command -v docker >/dev/null 2>&1; then runtime=docker; else runtime=podman; fi
fi

exec "${runtime}" run --rm --privileged -v /dev:/dev -v "${repo}:/zbm:ro" \
  -e ZBMTEST_IMPL="${ZBMTEST_IMPL:-legacy}" \
  -v "${ZBMTEST_UNIT_WORK:-/var/tmp/zbm-luks-unit}-container:/var/tmp/zbm-luks-unit" \
  --entrypoint /bin/bash "${image}" -c '
    apt-get update -qq >/dev/null &&
    apt-get install -y -qq --no-install-recommends swtpm swtpm-tools fdisk >/dev/null &&
    /zbm/tests/luks/unit/run-all.sh /zbm'
