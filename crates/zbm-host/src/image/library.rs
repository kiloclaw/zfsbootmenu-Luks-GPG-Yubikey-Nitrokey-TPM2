//! The shell library of the hook contract (ADR 0017, points 9 to 11): the
//! three files hooks source, `zfsbootmenu-core.sh`, `kmsg-log-lib.sh` and
//! `zfsbootmenu-kcl.sh`, as Rust images install them. Each function of the
//! legacy files becomes a wrapper that runs `zbm zfunc` with its name, but
//! those that only transform text and the logging functions, which stay
//! the legacy shell code; what the files run besides their functions (the
//! guards, the error trap, the `source` of the command-line functions)
//! stays as it is. The legacy files themselves go below
//! `/usr/share/zfsbootmenu/legacy`, for the functions `zbm zfunc` has no
//! Rust code for.

use std::collections::BTreeSet;
use std::fmt::Write as _;

/// The files of the library, below `lib` of the module root and `/lib` of
/// the image.
pub(crate) const FILES: [&str; 3] = [
    "zfsbootmenu-core.sh",
    "kmsg-log-lib.sh",
    "zfsbootmenu-kcl.sh",
];

/// Where Rust images keep the legacy files of the library, for the
/// functions `zbm zfunc` runs as the legacy runtime did.
pub(crate) const LEGACY: &str = zbm_tools::programs::LEGACY_LIBRARY;

/// The functions that stay shell code: those that only transform text,
/// and the logging functions, which spare a process per line logged.
pub const COPIED: [&str; 13] = [
    "decolorize",
    "colorize",
    "center_string",
    "is_snapshot",
    "kcl_tokenize",
    "kcl_suppress",
    "kcl_append",
    "kcl_assemble",
    "zdebug",
    "zinfo",
    "znotice",
    "zwarn",
    "zerror",
];

/// The functions of the three files, in their order: the hook contract,
/// which a test holds to the legacy files.
pub const FUNCTIONS: [&str; 69] = [
    "decolorize",
    "colorize",
    "center_string",
    "write_hostid",
    "get_spl_hostid",
    "match_hostid",
    "log_unimportable",
    "check_for_pools",
    "mount_block",
    "mount_zfs",
    "kexec_kernel",
    "duplicate_snapshot",
    "clone_snapshot",
    "create_snapshot",
    "rollback_snapshot",
    "set_default_kernel",
    "set_default_env",
    "find_be_initramfs",
    "find_be_kernels",
    "select_kernel",
    "find_root_prefix",
    "validate_cmdline_cache",
    "kernel_kcl_cache",
    "load_be_cmdline",
    "import_pool",
    "export_pool",
    "rewind_checkpoint",
    "has_resume_device",
    "timed_prompt",
    "resume_prompt",
    "is_snapshot",
    "is_degraded",
    "is_writable",
    "set_rw_pool",
    "set_ro_pool",
    "be_has_encroot",
    "be_is_locked",
    "be_keysource",
    "cache_key",
    "load_key",
    "be_location",
    "zfs_chroot",
    "emergency_shell",
    "zreport",
    "import_zbm_hooks",
    "is_mountpoint",
    "is_mounted",
    "is_efi_system",
    "mount_efivarfs",
    "is_zfs_filesystem",
    "recursive_umount",
    "zdebug",
    "zinfo",
    "znotice",
    "zwarn",
    "zerror",
    "print_kmsg_logs",
    "kcl_tokenize",
    "read_kcl_prop",
    "read_kcl_value",
    "kcl_suppress",
    "kcl_append",
    "kcl_assemble",
    "_build_zbm_kcl",
    "get_zbm_kcl",
    "make_cmdline_dir",
    "get_zbm_arg",
    "get_zbm_bool",
    "zbmcmdline",
];

/// The applet the wrappers run.
const ZBM: &str = "/libexec/zbm";

/// A function definition at the start of a line, `name() {`: its name.
fn defined(line: &str) -> Option<&str> {
    let (name, rest) = line.split_once("()")?;
    let is_name = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    (is_name && rest.trim() == "{").then_some(name)
}

/// The functions `text` defines at the start of a line, in order.
#[cfg(test)]
pub(crate) fn functions(text: &str) -> Vec<&str> {
    text.lines().filter_map(defined).collect()
}

/// The shell variables `texts` read, `${name}` and `$name`, which the
/// wrappers export to `zbm zfunc`: a variable a hook set without exporting
/// it reaches the function, as it reached the legacy one in the hook's
/// shell. The shell's own variables are left out, and the libraries'
/// include guards, which would keep the legacy files from loading.
fn read_variables(texts: &[&str]) -> BTreeSet<String> {
    const SHELL: [&str; 12] = [
        "BASH_REMATCH",
        "BASH_SOURCE",
        "BASH_COMMAND",
        "FUNCNAME",
        "IFS",
        "LINENO",
        "OPTARG",
        "OPTIND",
        "PATH",
        "PIPESTATUS",
        "RANDOM",
        "REPLY",
    ];
    let mut names = BTreeSet::new();
    for text in texts {
        let mut rest = *text;
        while let Some(at) = rest.find('$') {
            rest = &rest[at + 1..];
            let name: String = rest
                .trim_start_matches('{')
                .chars()
                .take_while(|c| *c == '_' || c.is_ascii_alphanumeric())
                .collect();
            let starts_well = name
                .chars()
                .next()
                .is_some_and(|c| c == '_' || c.is_ascii_alphabetic());
            let own = SHELL.contains(&name.as_str())
                || name.starts_with("BASH")
                || name.starts_with("_ZFSBOOTMENU");
            if starts_well && !own {
                names.insert(name);
            }
        }
    }
    names
}

/// The helper every wrapper calls: `zbm zfunc` in a subshell that exports
/// the variables the functions read.
fn helper(variables: &BTreeSet<String>) -> String {
    let mut text = String::from(
        "# Generated: the functions of this library run in zbm (ADR 0017).\n\
         _zbm_zfunc() {\n  (\n",
    );
    let mut line = String::from("    export");
    for name in variables {
        if line.len() + name.len() > 76 {
            let _ = writeln!(text, "{line} 2>/dev/null");
            line = String::from("    export");
        }
        line.push(' ');
        line.push_str(name);
    }
    let _ = writeln!(text, "{line} 2>/dev/null");
    let _ = write!(text, "    exec {ZBM} zfunc \"$@\"\n  )\n}}\n\n");
    text
}

/// The body of the wrapper of `name`, with the shell code around the call
/// for what the legacy function left in its caller's shell.
fn wrapper(name: &str) -> String {
    let call = format!("_zbm_zfunc {name} \"$@\"");
    let body = match name {
        // The default boot environment, once it is set.
        "set_default_env" => format!("  {call} || return\n  BOOTFS=\"${{1}}\"\n"),
        // The exit status, and the apparent host ID after a failure.
        "import_pool" => format!(
            "  {call}\n  ret=$?\n  [ \"${{ret}}\" -eq 0 ] || \
             spl_hostid=\"$( _zbm_zfunc get_spl_hostid )\"\n  return \"${{ret}}\"\n"
        ),
        "export_pool" => format!("  {call}\n  ret=$?\n  return \"${{ret}}\"\n"),
        // The size of the terminal the prompt measured.
        "timed_prompt" => format!(
            "  local status\n  {call}\n  status=$?\n  \
             HEIGHT=\"$( tput lines 2>/dev/null )\"\n  \
             WIDTH=\"$( tput cols 2>/dev/null )\"\n  return \"${{status}}\"\n"
        ),
        // The device asked about.
        "is_mounted" => format!("  mount_dev=\"${{1}}\"\n  {call}\n"),
        _ => format!("  {call}\n"),
    };
    format!("{name}() {{\n{body}}}\n")
}

/// The file of the library whose legacy text is `text`, given the legacy
/// texts of all three `all`: each function a wrapper, but those of
/// [`COPIED`], and the helper before the first function.
pub(crate) fn generate(text: &str, all: &[&str]) -> String {
    let mut out = String::new();
    let mut lines = text.lines();
    let mut helped = false;
    while let Some(line) = lines.next() {
        let Some(name) = defined(line) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if !helped {
            out.push_str(&helper(&read_variables(all)));
            helped = true;
        }
        if COPIED.contains(&name) {
            out.push_str(line);
            out.push('\n');
            for body in lines.by_ref() {
                out.push_str(body);
                out.push('\n');
                if body.starts_with('}') {
                    break;
                }
            }
        } else {
            out.push_str(&wrapper(name));
            for body in lines.by_ref() {
                if body.starts_with('}') {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
