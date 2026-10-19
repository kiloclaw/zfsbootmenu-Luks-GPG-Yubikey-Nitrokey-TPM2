#!/bin/sh
# vim: softtabstop=2 shiftwidth=2 expandtab

# Runs inside a Void Linux container (see cache-kernel.sh). Expects:
#   REPO       repository URL to download from
#   PKGS       space-separated package versions (pkgname-version_revision)
#   XBPS_ARCH  target architecture
# and the cache directory mounted at /cache. XBPS_OPTS may add options to the
# xbps-install and xbps-query commands (used for testing).

set -eu

die() {
  echo "ERROR: $*" >&2
  exit 1
}

: "${XBPS_OPTS:=}"
: "${CACHE:=/cache}"

# shellcheck disable=SC2086
xbps-install ${XBPS_OPTS} -S --repository="${REPO}" >/dev/null \
  || die "unable to synchronize repository ${REPO}"

sums="${CACHE}/SHA256SUMS.new"
trap 'rm -f "${sums}"' EXIT
: > "${sums}"

for pkg in ${PKGS}; do
  # shellcheck disable=SC2086
  if ! found="$(xbps-query ${XBPS_OPTS} --repository="${REPO}" -R -p pkgver "${pkg}" 2>/dev/null)" \
      || [ "${found}" != "${pkg}" ]; then
    die "${pkg} is no longer available from ${REPO}; the mirror has moved on to a newer build"
  fi

  # shellcheck disable=SC2086
  sha="$(xbps-query ${XBPS_OPTS} --repository="${REPO}" -R -p filename-sha256 "${pkg}")"
  [ -n "${sha}" ] || die "no checksum for ${pkg} in the repository index"

  file="${pkg}.${XBPS_ARCH}.xbps"
  echo "Fetching ${file}"
  xbps-fetch -o "${CACHE}/${file}" "${REPO}/${file}" >/dev/null \
    || die "unable to download ${REPO}/${file}"

  (cd "${CACHE}" && echo "${sha}  ${file}" | sha256sum -c -) \
    || die "checksum mismatch for ${file}"

  echo "${sha}  ${file}" >> "${sums}"
done

mv "${sums}" "${CACHE}/SHA256SUMS"

# Rebuild the local repository index from scratch
rm -f "${CACHE}/${XBPS_ARCH}-repodata" "${CACHE}/${XBPS_ARCH}-stagedata"
xbps-rindex -a "${CACHE}"/*.xbps >/dev/null || die "unable to index ${CACHE}"
echo "Indexed ${CACHE}"
