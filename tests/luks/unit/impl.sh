#!/bin/bash
# The implementation of the LUKS code that the unit suites test, selected with
# ZBMTEST_IMPL:
#
#   legacy  the Bash libraries in legacy/zfsbootmenu/lib (the default)
#   rust    the test build of the Rust runtime's zbm program (the Cargo feature
#           test-seams), at ZBMTEST_ZBM or target/test-seams/debug/zbm, which
#           zbm_impl_check builds with cargo when it is missing
#
# Suites source this file, call zbm_impl_check before setting anything up, and
# call zbm_impl_load in each case. Everything a suite observes or controls in
# the code under test goes through the seams below, so that another
# implementation can provide the same seams (see "Driving the existing suites"
# in crates/DESIGN.md).
#
# What the suites observe:
#
#   $FAKE/log      one line per message, "LEVEL: text" (INFO, WARN, ERROR,
#                  DEBUG), instead of the kernel log
#   $FAKE/prompts  one line per timed prompt, "timed_prompt <arguments>"
#   $FAKE/out      everything written to standard output and standard error
#   $FAKE/calls    the commands the stub programs in stubs/ and kstubs/ ran
#   /dev/mapper    the mappings, which the stub cryptsetup creates
#
# What the suites control:
#
#   standard input                  passphrases, PINs and other typed answers
#   $FAKE/answers (or $ANSWERS)     timed prompts: one status per line, 0 for
#                                   RETURN and 1 for ESCAPE; 1 when empty
#   ZBMTEST_PROMPT_DELAY            seconds each timed prompt waits (default 0)
#   $FAKE/holders/<device>          a volume counts as open while this exists;
#                                   the stub cryptsetup creates and removes it
#   $BASE/cmdline.d/<option>        ZFSBootMenu command line options
#   ZBM_CRYPTTAB, LUKS_STATE, LUKS_SKIPPED, LUKS_GNUPG_DIR, LUKS_TPM2_TCTI,
#   LUKS_NO_TOKEN_PLUGINS, LUKS_DEFAULT_TRIES, LUKS_RETRY
#                                   configuration of the LUKS code
#   PATH                            stub programs and extra stand-ins
#
# What the suites call:
#
#   luks_unlock_all, luks_close_all [name], luks_resolve_device <spec>,
#   luks_list_tokens <device>, luks_record_mapping <name> <device> <auto>,
#   luks_token_wait
#
# Two cases in edge.sh reach inside the legacy code and run only against it
# (zbm_impl_is_legacy): the retry without cryptsetup replaces luks_available,
# and the import-loop case runs init.d/50-import-pools with its functions
# replaced. The Rust runtime covers both with unit tests.

_zbm_impl_repo="$( cd "$( dirname "${BASH_SOURCE[0]}" )/../../.." && pwd )"

# The test build of zbm.
ZBMTEST_ZBM="${ZBMTEST_ZBM:-${_zbm_impl_repo}/target/test-seams/debug/zbm}"

# Reports whether the selected implementation can be tested; exits with 77,
# which run-all.sh reports as skipped, when it cannot.
zbm_impl_check() {
  case "${ZBMTEST_IMPL:-legacy}" in
    legacy)
      return 0
      ;;
    rust)
      if [ ! -x "${ZBMTEST_ZBM}" ] && command -v cargo >/dev/null 2>&1; then
        ( cd "${_zbm_impl_repo}" && CARGO_TARGET_DIR=target/test-seams \
            cargo build -q -p zbm --features test-seams ) || exit 2
      fi
      if ! "${ZBMTEST_ZBM}" --list 2>/dev/null | grep -q '^zbm-luks-test '; then
        echo "no test build of zbm at ${ZBMTEST_ZBM}; build it with:"
        echo "  CARGO_TARGET_DIR=target/test-seams cargo build -p zbm --features test-seams"
        exit 77
      fi
      return 0
      ;;
    *)
      echo "unknown implementation ZBMTEST_IMPL=${ZBMTEST_IMPL}; use legacy or rust"
      exit 2
      ;;
  esac
}

# Whether the selected implementation is the legacy one, for the cases that
# replace a function inside it.
zbm_impl_is_legacy() {
  [ "${ZBMTEST_IMPL:-legacy}" = "legacy" ]
}

# The stand-ins for logging and prompts, which write to $FAKE.
_zbm_impl_seams() {
  zinfo()  { echo "INFO: $*"  >> "${FAKE:?}/log"; }
  zwarn()  { echo "WARN: $*"  >> "${FAKE:?}/log"; }
  zerror() { echo "ERROR: $*" >> "${FAKE:?}/log"; }
  zdebug() { [ $# -gt 0 ] && echo "DEBUG: $*" >> "${FAKE:?}/log"; return 0; }
  colorize() { shift; echo -ne "$@"; }
  timed_prompt() {
    local answers="${ANSWERS:-${FAKE:?}/answers}" a
    echo "timed_prompt $*" >> "${FAKE:?}/prompts"
    [ "${ZBMTEST_PROMPT_DELAY:-0}" -gt 0 ] && sleep "${ZBMTEST_PROMPT_DELAY}"
    a=$(head -n1 "${answers}" 2>/dev/null); sed -i 1d "${answers}" 2>/dev/null
    return "${a:-1}"
  }
}

# arg1: the ZFSBootMenu source tree
# arg2: the crypttab the code reads
# Loads the LUKS code into the calling shell, with the seams described above.
zbm_impl_load() {
  local repo="${1?a source tree is required}"
  case "${ZBMTEST_IMPL:-legacy}" in
    legacy)
      _zbm_impl_seams
      _ZFSBOOTMENU_CORE=1
      # shellcheck disable=SC1091
      source "${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-kcl.sh" || return 1
      # Both are read by the library sourced below
      # shellcheck disable=SC2034
      ZBM_CRYPTTAB="${2?a crypttab is required}"
      # shellcheck disable=SC2034
      LUKS_KEYS_LIB="${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh"
      # shellcheck disable=SC1091
      source "${repo}/legacy/zfsbootmenu/lib/zfsbootmenu-luks.sh" || return 1
      # sysfs holders cannot be faked; consult the stub's record instead
      luks_is_open() { [ -e "${FAKE:?}/holders/$(basename "$(readlink -f "$1")")" ]; }
      ;;
    rust)
      # The variables the legacy library sets when it is sourced; they reach
      # zbm through _zbm_impl_rust
      # shellcheck disable=SC2034
      ZBM_CRYPTTAB="${2?a crypttab is required}"
      : "${LUKS_STATE:=${BASE}/luks-mappings}"
      : "${LUKS_SKIPPED:=${BASE}/luks-skipped}"
      luks_unlock_all() { _zbm_impl_rust unlock_all; }
      luks_close_all() { _zbm_impl_rust close_all "$@"; }
      luks_resolve_device() { _zbm_impl_rust resolve_device "$@"; }
      luks_list_tokens() { _zbm_impl_rust list_tokens "$@"; }
      luks_record_mapping() { _zbm_impl_rust record_mapping "$@"; }
      luks_token_wait() { _zbm_impl_rust token_wait; }
      ;;
    *)
      echo "zbm_impl_load: no implementation '${ZBMTEST_IMPL}' to load" >&2
      return 1
      ;;
  esac
}

# args: an operation of the test build and its arguments
# Runs the operation with the seams and the configuration variables, which the
# suites set without exporting them, as the legacy library reads them.
_zbm_impl_rust() {
  local -a vars=()
  local var
  for var in BASE ZBM_CRYPTTAB LUKS_STATE LUKS_SKIPPED LUKS_GNUPG_DIR LUKS_TPM2_TCTI \
      LUKS_NO_TOKEN_PLUGINS LUKS_DEFAULT_TRIES LUKS_RETRY zbm_retry_delay; do
    [ -n "${!var+x}" ] && vars+=( "${var}=${!var}" )
  done
  env "${vars[@]}" ZBMTEST_LOG="${FAKE:?}/log" ZBMTEST_PROMPTS="${FAKE}/prompts" \
    ZBMTEST_ANSWERS="${ANSWERS:-${FAKE}/answers}" ZBMTEST_HOLDERS="${FAKE}/holders" \
    "${ZBMTEST_ZBM}" zbm-luks-test "$@"
}
