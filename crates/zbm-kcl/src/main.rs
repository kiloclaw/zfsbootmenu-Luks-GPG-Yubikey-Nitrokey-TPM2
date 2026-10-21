//! `zbm-kcl`: review or change the kernel command line of a ZFS boot environment or a ZFSBootMenu EFI image.
//!
//! See `zbm-kcl(8)` and the [`zbm_kcl`] library, which holds the program.

use std::io::Write;

fn main() {
    // Like the Bash script, end quietly on a closed pipe, and keep running on SIGINT and SIGTERM (the script's trap
    // runs its clean-up and carries on; child programs still receive the signals).
    let _ = zbm_sys::restore_default_sigpipe();
    let _ = zbm_sys::record_signals(&[zbm_sys::Signal::Interrupt, zbm_sys::Signal::Terminate]);

    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_else(|| "zbm-kcl".to_owned());
    let args: Vec<String> = args.collect();
    let context = zbm_kcl::Context::from_environment(&argv0);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = zbm_kcl::run(
        &context,
        &args,
        &mut zbm_kcl::Io {
            stdin: &mut stdin.lock(),
            stdout: &mut stdout.lock(),
            stderr: &mut stderr.lock(),
        },
    );
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
