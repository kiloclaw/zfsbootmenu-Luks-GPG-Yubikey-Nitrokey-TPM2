#!/bin/bash
# Test hook: copy the Rust runtime's journal to the serial log, so that a
# failed unlock explains itself in the test's record: the warnings (ZW|), the
# debug lines of tpm2-tools, clevis and cryptsetup (ZD|), and the key programs
# run, with their statuses (ZC|)
zbm=/libexec/zbm
[ -x "${zbm}" ] && [ -s /zfsbootmenu/journal.jsonl ] || exit 0
"${zbm}" debug journal --level warn 2>&1 | sed 's/^/ZW| /'
"${zbm}" debug journal --level debug 2>&1 | grep -E '^ *[0-9]+ DEBUG ' \
  | grep -E 'tpm2_|clevis|cryptsetup' | sed 's/^/ZD| /'
"${zbm}" debug commands 2>&1 | grep -E ' ms  (clevis|jose|tpm2_|cryptsetup)' | sed 's/^/ZC| /'
exit 0
