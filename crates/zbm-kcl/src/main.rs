//! `zbm-kcl`: review or change the kernel command line of a ZFS boot
//! environment or a ZFSBootMenu EFI image.
//!
//! See `zbm-kcl(8)` and the [`zbm_kcl`] library, which holds the program.

use std::io::Write;

fn main() {
    // Like the Bash script, end quietly on a closed pipe, and keep running on
    // SIGINT and SIGTERM (the script's trap runs its clean-up and carries on; child
    // programs still receive the signals): they are blocked, and never read.
    let _ = zbm_sys::restore_default_sigpipe();
    let carried_on = [zbm_sys::Signal::Interrupt, zbm_sys::Signal::Terminate];
    let _signals = zbm_sys::Signals::block(zbm_sys::SignalSet::of(&carried_on));

    let (argv0, args) = match zbm_platform::args::split(std::env::args_os(), "zbm-kcl") {
        Ok(split) => split,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            std::process::exit(2);
        }
    };
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
