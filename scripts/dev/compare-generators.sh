#!/bin/bash
# vim: softtabstop=2 shiftwidth=2 expandtab
#
# Builds ZFSBootMenu images in the build container with the Perl and with the
# Rust generate-zbm, and checks with compare-images.sh that they match.
#
# Usage: compare-generators.sh [style ...]
#
# Styles: release and recovery (the release configurations in
# config/zfsbootmenu) and builder (the container's default configuration,
# config/zbm-builder); all three by default. EFI bundles are built too.
#
# Environment:
#   ZBM_BUILDER_IMAGE  build container image (default localhost/zbm-builder)
#   ZBM_RUNTIME        docker or podman (default: docker if installed)
#   ZBM_COMPARE_WORK   work directory (default: a new temporary directory)
#
# Exits with 0 when every style matches.

repo="$( cd "$( dirname "$0" )/../.." && pwd )"
image="${ZBM_BUILDER_IMAGE:-localhost/zbm-builder}"
runtime="${ZBM_RUNTIME:-}"
if [ -z "${runtime}" ]; then
  if command -v docker >/dev/null 2>&1; then runtime=docker; else runtime=podman; fi
fi

work="${ZBM_COMPARE_WORK:-$( mktemp -d )}" || exit 2
mkdir -p "${work}/target" || exit 2

styles=( "$@" )
[ "${#styles[@]}" -gt 0 ] || styles=( release recovery builder )

status=0
for style in "${styles[@]}"; do
  config="${work}/${style}/config"
  rm -rf "${work:?}/${style}"
  mkdir -p "${config}/dracut.conf.d" || exit 2
  case "${style}" in
    release|recovery)
      cp "${repo}/config/zfsbootmenu/${style}.yaml" "${config}/config.yaml"
      cp "${repo}/config/zfsbootmenu/${style}.conf.d/"*.conf "${config}/dracut.conf.d/"
      ;;
    builder)
      ;;
    *)
      echo "unknown style '${style}'" >&2
      exit 2
      ;;
  esac

  for generator in perl rust; do
    echo "== ${style}: building with the ${generator} generate-zbm"
    mkdir -p "${work}/${style}/${generator}"
    # The Rust programs are built once and reused through CARGO_TARGET_DIR
    if ! "${runtime}" run --rm -e CARGO_TARGET_DIR=/cargo-target \
        -v "${repo}:/zbm:ro" -v "${config}:/build:ro" \
        -v "${work}/${style}/${generator}:/out" -v "${work}/target:/cargo-target" \
        "${image}" -o /out -V -g "${generator}" -e ".EFI.Enabled = true" \
        > "${work}/${style}/${generator}.log" 2>&1; then
      tail -n 20 "${work}/${style}/${generator}.log"
      echo "FAILED: ${style} with the ${generator} generate-zbm"
      status=1
      continue 2
    fi
  done

  echo "== ${style}: comparing"
  if ! "${runtime}" run --rm -v "${repo}:/zbm:ro" -v "${work}/${style}:/images:ro" \
      --entrypoint /zbm/scripts/dev/compare-images.sh "${image}" /images/perl /images/rust; then
    status=1
  fi
done

echo "Work directory: ${work}"
exit "${status}"
