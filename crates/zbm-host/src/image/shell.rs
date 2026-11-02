//! The files of the shell: `/etc/zfsbootmenu.conf`, the profiles, the
//! profiling configuration, and the line editor's input (`/etc/inputrc`,
//! the C.UTF-8 character set, xterm's terminal description, the hand-over
//! to brush).

use std::fmt::Write as _;
use std::path::Path;

use zbm_tools::host::fzf_supports;

use super::{Builder, Image};

/// `/etc/profile`, around the line that sets `TERM`.
const PROFILE: [&str; 2] = [
    "export PATH=/zbm/bin:/usr/sbin:/usr/bin:/sbin:/bin\n",
    "export HOME=/root\n",
];

/// The terminal of the legacy profile: always the Linux console.
const TERM_LINUX: &str = "export TERM=linux\n";

/// The terminal of the Rust runtime's profile: the Linux console, unless
/// the command line or an SSH client asked for a dumb terminal, which shows
/// the menu as lines (ADR 0011).
const TERM_UNLESS_DUMB: &str = "[ \"${TERM}\" = dumb ] || export TERM=linux\n";

/// `/root/.bashrc`.
const BASHRC: &str = r#"source /etc/zfsbootmenu.conf >/dev/null 2>&1
source /lib/kmsg-log-lib.sh >/dev/null 2>&1
source /lib/zfsbootmenu-core.sh >/dev/null 2>&1
source /lib/zfsbootmenu-kcl.sh >/dev/null 2>&1
[ -f /etc/profile ] && source /etc/profile
[ -f /lib/zfsbootmenu-completions.sh ] && source /lib/zfsbootmenu-completions.sh

export PROMPT_COMMAND="/libexec/recovery-error-printer"
export PS1='\[\033[0;33m\]zfsbootmenu\[\033[0m\] \w > '

alias clear="tput clear"
alias reset="tput reset"
alias trace="ztrace"
alias help="/libexec/zfsbootmenu-help -L recovery-shell"

zdebug "sourced /root/.bashrc" || true
"#;

/// The end of `/root/.bashrc` where the image has brush: SSH logins get the
/// shell the runtime chose.
const BASHRC_BRUSH: &str = r#"
# SSH logins get the shell that the runtime chose (zbm.shell)
if [ -n "${SSH_CONNECTION}" ] && [ -z "${BRUSH_VERSION}" ] \
  && shopt -q login_shell 2>/dev/null \
  && [ "$( cat "${BASE:-/zfsbootmenu}/shell" 2>/dev/null )" = /usr/bin/brush ]; then
  exec /usr/bin/brush
fi
"#;

/// `/etc/inputrc`: readline keeps bytes beyond ASCII.
const INPUTRC: &str = "set input-meta on\nset output-meta on\nset convert-meta off\n";

/// The options of `fzf` the legacy menu uses where `fzf` has them: the
/// variable that says so, and the options that tell.
const FZF_FEATURES: [(&str, &[&str]); 5] = [
    // fzf 0.22.0
    ("HAS_REFRESH", &["--bind", "alt-l:refresh-preview"]),
    // fzf 0.25.0
    ("HAS_DISABLED", &["--disabled"]),
    // fzf 0.35.0
    ("HAS_BORDER", &["--border-label=test"]),
    // fzf 0.52.0 (after HAS_COLUMN in the file)
    (
        "HAS_COLORS",
        &["--color=current-fg:red,selected-fg:magenta"],
    ),
    // fzf 0.66.0
    ("HAS_RAW", &["--raw"]),
];

/// `1` when `on`, else nothing, as the file has its flags.
fn flag(on: bool) -> &'static str {
    if on { "1" } else { "" }
}

/// `/etc/zfsbootmenu.conf`'s first part: what the build host's `fzf` can
/// do, whether the image has `column`, the builder, whether this is a
/// release and the runtime; with dracut, the embedded command line
/// (`embedded_kcl`) after it, in single quotes so that the shell and the
/// Rust runtime read it the same.
pub fn conf(image: &mut Image) {
    let fzf: Vec<(&str, &str)> = FZF_FEATURES
        .iter()
        .map(|(name, options)| (*name, flag(fzf_supports(image.host.runner, options))))
        .collect();
    let column = flag(image.host.which("column").is_some());
    let release = flag(image.config.is_release());
    let mut exports = fzf;
    exports.insert(3, ("HAS_COLUMN", column));
    exports.extend([
        ("ZBM_BUILDSTYLE", image.builder.name()),
        ("ZBM_RELEASE_BUILD", release),
        ("ZBM_RUNTIME", image.runtime.name()),
    ]);
    let mut text = zbm_config::conf::header(&exports);
    if image.builder == Builder::Dracut
        && let Some(kcl) = image.config.get("embedded_kcl")
    {
        let _ = writeln!(text, "export embedded_kcl='{}'", kcl.replace('\'', r"'\''"));
    }
    image.plan.write("/etc/zfsbootmenu.conf", 0o644, text);
}

/// `/etc/profile`, `/root/.bashrc`, and `/root/.profile`, a link to it for
/// `/bin/sh`, which dropbear starts.
pub fn profiles(image: &mut Image) {
    let term = if image.is_rust() {
        TERM_UNLESS_DUMB
    } else {
        TERM_LINUX
    };
    let profile = [PROFILE[0], term, PROFILE[1]].concat();
    image.plan.write("/etc/profile", 0o644, profile);
    image.plan.write("/root/.bashrc", 0o644, BASHRC);
    image.plan.link("/root/.bashrc", "/root/.profile");
}

/// Profiling, with `zfsbootmenu_trace_enable` and the profiling library of
/// the module root: the library and `/etc/profiling.conf`; without, a
/// library that does nothing.
pub fn traceconf(image: &mut Image) {
    let library = image.module_root().join("profiling/profiling-lib.sh");
    if !image.config.is_yes("zfsbootmenu_trace_enable") || !image.host.is_file(&library) {
        image
            .plan
            .write("/lib/profiling-lib.sh", 0o644, "return 0\n");
        return;
    }
    image.install(&library, Path::new("/lib/profiling-lib.sh"));
    let text = format!(
        "export zfsbootmenu_trace_term={}\nexport zfsbootmenu_trace_baud={}\n",
        image.config.or("zfsbootmenu_trace_term", ""),
        image.config.or("zfsbootmenu_trace_baud", ""),
    );
    image.plan.write("/etc/profiling.conf", 0o644, text);
}

/// The Rust runtime's line editor takes characters beyond ASCII, which the
/// console on the display types with its keyboard layouts: readline keeps
/// their bytes (`/etc/inputrc`), and the C library's C.UTF-8 character set,
/// when the host has one, makes them characters. Programs on the console on
/// the display get xterm's terminal description when the host has it
/// (`ZBM_CONSOLE=drm`). Where the image has brush, Bash, an SSH session's
/// login shell, hands over to it when the runtime chose it. After
/// [`profiles`].
pub fn input(image: &mut Image) {
    if !image.is_rust() {
        return;
    }
    image.plan.write("/etc/inputrc", 0o644, INPUTRC);
    let ctypes = [
        "/usr/lib/locale/C.utf8/LC_CTYPE",
        "/usr/lib/locale/C.UTF-8/LC_CTYPE",
    ];
    if let Some(ctype) = ctypes
        .iter()
        .map(Path::new)
        .find(|path| image.host.is_file(path))
    {
        image.install(ctype, Path::new("/usr/lib/locale/C.utf8/LC_CTYPE"));
        image
            .plan
            .append("/etc/profile", 0o644, "export LC_CTYPE=C.UTF-8\n");
    }
    let terminfo = ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"]
        .iter()
        .map(|dir| Path::new(dir).join("x/xterm-256color"))
        .find(|path| image.host.is_file(path));
    if let Some(terminfo) = terminfo {
        image.install(&terminfo, &terminfo);
        image.plan.append(
            "/etc/profile",
            0o644,
            "[ \"${ZBM_CONSOLE}\" = drm ] && export TERM=xterm-256color\n",
        );
    }
    if image.plan.has(Path::new("/usr/bin/brush")) {
        image.plan.append("/root/.bashrc", 0o644, BASHRC_BRUSH);
    }
}

/// The terminal descriptions of the console (`linux`) and of serial
/// terminals, from the first of the host's terminfo trees that has
/// `linux` (mkinitcpio, which has none of its own).
pub fn terminfo(image: &mut Image) {
    let root = ["/etc", "/usr/share", "/lib"]
        .iter()
        .map(|dir| Path::new(dir).join("terminfo"))
        .find(|root| image.host.is_file(&root.join("l/linux")));
    let Some(root) = root else {
        return;
    };
    for name in ["l/linux", "v/vt100", "v/vt102", "v/vt220"] {
        let src = root.join(name);
        if image.host.is_file(&src) {
            image.install(&src, &Path::new("/usr/share/terminfo").join(name));
        }
    }
}
