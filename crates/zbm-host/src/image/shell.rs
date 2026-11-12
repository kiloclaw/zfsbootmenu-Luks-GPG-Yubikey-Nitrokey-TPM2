//! The files of the shell: `/etc/zfsbootmenu.conf`, the profiles, the
//! profiling configuration, and the line editor's input (`/etc/inputrc`,
//! the C.UTF-8 character set, xterm's terminal description, the hand-over
//! to brush).

use std::fmt::Write as _;
use std::path::Path;

use zbm_tools::host::fzf_supports;
use zbm_types::{Completion, LINKED};

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

/// `/root/.bashrc`, up to the completions: the configuration, the
/// library and the profile.
const BASHRC: &str = r"source /etc/zfsbootmenu.conf >/dev/null 2>&1
source /lib/kmsg-log-lib.sh >/dev/null 2>&1
source /lib/zfsbootmenu-core.sh >/dev/null 2>&1
source /lib/zfsbootmenu-kcl.sh >/dev/null 2>&1
[ -f /etc/profile ] && source /etc/profile
";

/// The legacy runtime's completions, a file of its module root, which Rust
/// images do not take.
const COMPLETIONS_LEGACY: &str =
    "[ -f /lib/zfsbootmenu-completions.sh ] && source /lib/zfsbootmenu-completions.sh\n";

/// The Rust runtime's completions, before the lines that name their
/// commands ([`completions_rust`]).
const COMPLETE_FUNCTION: [&str; 2] = [
    r#"
# Tab completes the argument of the recovery shell's commands with the names
# zbm lists for it, those the command takes
_zbm_complete() {
  local kind IFS=$'\n'
  COMPREPLY=()
  case "${1##*/} ${COMP_CWORD} $3" in
"#,
    r#"    *) return 0 ;;
  esac
  COMPREPLY=( $( /libexec/zbm complete "${kind}" "$2" ) )
}
"#,
];

/// The Rust runtime's completions, from the names an image links to `zbm`
/// and the shell library's functions, each with what it completes (ADR
/// 0018, point 23): one function that asks `zbm complete` for the names
/// that fit the word, and the commands it completes. A command's first
/// argument is completed, or the argument of its option where it names
/// one; the patterns match the command's name, the word's place and the
/// word before it.
pub(super) fn completions_rust() -> String {
    let linked = LINKED.iter().filter_map(|linked| {
        let (kind, option) = linked.complete?;
        let pattern = match option {
            Some(option) => format!("\"{} 2 {option}\"", linked.name),
            None => format!("\"{} 1 \"*", linked.name),
        };
        Some((linked.name, pattern, kind))
    });
    let functions = Completion::FUNCTIONS
        .iter()
        .map(|(name, kind)| (*name, format!("\"{name} 1 \"*"), *kind));
    let completed: Vec<(&str, String, Completion)> = linked.chain(functions).collect();
    let mut text = String::from(COMPLETE_FUNCTION[0]);
    for kind in Completion::ALL {
        let patterns: Vec<&str> = completed
            .iter()
            .filter(|(_, _, of)| *of == kind)
            .map(|(_, pattern, _)| pattern.as_str())
            .collect();
        if !patterns.is_empty() {
            let _ = writeln!(text, "    {}) kind={} ;;", patterns.join("|"), kind.name());
        }
    }
    text.push_str(COMPLETE_FUNCTION[1]);
    for (name, _, _) in &completed {
        let _ = writeln!(text, "complete -F _zbm_complete {name}");
    }
    text
}

/// The command the legacy runtime runs before each prompt, after a blank
/// line: the errors logged since the last prompt, read with `dmesg
/// --since` a time.
const PROMPT_LEGACY: &str = "\nexport PROMPT_COMMAND=\"/libexec/recovery-error-printer\"\n";

/// The command the Rust runtime runs before each prompt, after a blank
/// line: the errors logged after the last record shown, escaped (ADR 0018,
/// point 13).
const PROMPT_RUST: &str = "\nexport PROMPT_COMMAND=\"/libexec/zbm logs --prompt\"\n";

/// `/root/.bashrc` after the prompt's command, up to the alias `help`.
const BASHRC_ALIASES: &str = r#"export PS1='\[\033[0;33m\]zfsbootmenu\[\033[0m\] \w > '

alias clear="tput clear"
alias reset="tput reset"
alias trace="ztrace"
"#;

/// The legacy runtime's `help`: its help viewer, on the page of the
/// recovery shell.
const HELP_LEGACY: &str = "alias help=\"/libexec/zfsbootmenu-help -L recovery-shell\"\n";

/// The Rust runtime's `help`: `zbm help` and the topic typed after it, as
/// `/bin/zbm` passes a command on to `/libexec/zbm` (ADR 0018, point 17).
/// Bash runs an alias before its builtin `help` and before `/bin/help`.
const HELP_RUST: &str = "alias help=\"zbm help\"\n";

/// The rest of `/root/.bashrc`, after the alias `help`.
const BASHRC_END: &str = r#"
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

/// What the legacy menu reads in `/etc/zfsbootmenu.conf`: what the build
/// host's `fzf` can do, and whether the image has `column`. The Rust
/// runtime runs no `fzf` (ADR 0018, point 21), so its images have none of
/// these flags, and their build asks neither.
fn fzf_flags(image: &Image) -> Vec<(&'static str, &'static str)> {
    let mut flags: Vec<(&str, &str)> = FZF_FEATURES
        .iter()
        .map(|(name, options)| (*name, flag(fzf_supports(image.host.runner, options))))
        .collect();
    flags.insert(
        3,
        ("HAS_COLUMN", flag(image.host.which("column").is_some())),
    );
    flags
}

/// `/etc/zfsbootmenu.conf`'s first part: in legacy images the flags of the
/// legacy menu (`fzf_flags`); the builder, whether this is a release and
/// the runtime; with dracut, the embedded command line (`embedded_kcl`)
/// after it, in single quotes so that the shell and the Rust runtime read
/// it the same.
pub fn conf(image: &mut Image) {
    let release = flag(image.config.is_release());
    let mut exports = if image.is_rust() {
        Vec::new()
    } else {
        fzf_flags(image)
    };
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
    image
        .plan
        .write("/root/.bashrc", 0o644, bashrc(image.is_rust()));
    image.plan.link("/root/.bashrc", "/root/.profile");
}

/// `/root/.bashrc` of the Rust runtime (`is_rust`) or of the legacy one,
/// which differ in the completions, in the command run before each prompt
/// and in `help`.
fn bashrc(is_rust: bool) -> String {
    let (completions, prompt, help) = if is_rust {
        (completions_rust(), PROMPT_RUST, HELP_RUST)
    } else {
        (COMPLETIONS_LEGACY.to_owned(), PROMPT_LEGACY, HELP_LEGACY)
    };
    [
        BASHRC,
        &completions,
        prompt,
        BASHRC_ALIASES,
        help,
        BASHRC_END,
    ]
    .concat()
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
