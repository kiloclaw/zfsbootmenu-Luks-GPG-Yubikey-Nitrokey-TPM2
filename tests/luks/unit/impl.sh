#!/bin/bash
# The implementation of the LUKS code that the unit suites test, selected with
# ZBMTEST_IMPL:
#
#   legacy  the Bash libraries in legacy/zfsbootmenu/lib (the default)
#   rust    the zbm program of the Rust runtime, which does not exist yet: it
#           is ported in Phases 5 and 6 of PORT-ROADMAP.md, and the suites skip
#           until then
#
# Suites source this file, call zbm_impl_check before setting anything up, and
# call zbm_impl_load in each case, after defining the stand-ins for logging and
# prompts and before overriding functions that only exist in the Bash code.

# Reports whether the selected implementation can be tested; exits with 77,
# which run-all.sh reports as skipped, when it cannot.
zbm_impl_check() {
  case "${ZBMTEST_IMPL:-legacy}" in
    legacy)
      return 0
      ;;
    rust)
      echo "the Rust runtime is not available yet (ZBMTEST_IMPL=rust)"
      exit 77
      ;;
    *)
      echo "unknown implementation ZBMTEST_IMPL=${ZBMTEST_IMPL}; use legacy or rust"
      exit 2
      ;;
  esac
}

# arg1: the ZFSBootMenu source tree
# arg2: the crypttab the code reads
# Loads the LUKS code into the calling shell.
zbm_impl_load() {
  local repo="${1?a source tree is required}"
  case "${ZBMTEST_IMPL:-legacy}" in
    legacy)
      _ZFSBOOTMENU_CORE=1
      # shellcheck disable=SC1091
      source "${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-kcl.sh" || return 1
      ZBM_CRYPTTAB="${2?a crypttab is required}"
      LUKS_KEYS_LIB="${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh"
      # shellcheck disable=SC1091
      source "${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-luks.sh" || return 1
      ;;
    *)
      echo "zbm_impl_load: no implementation '${ZBMTEST_IMPL}' to load" >&2
      return 1
      ;;
  esac
}
