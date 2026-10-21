//! The interactive menu of the ZFSBootMenu runtime: package E of
//! `PORT-ROADMAP.md`.
//!
//! This crate will replace `bin/zfsbootmenu`, `lib/zfsbootmenu-ui.sh`,
//! `lib/fzf-defaults.sh` and the programs `fzf` starts: the main loop and its
//! screens, the `fzf` options and key bindings, previews, the help viewer, the
//! command line editor and the power menu. The menu keeps driving `fzf`; which
//! screen follows which key is a pure function, and the text of each screen is
//! checked with snapshot tests. The code arrives in Phase 6.
