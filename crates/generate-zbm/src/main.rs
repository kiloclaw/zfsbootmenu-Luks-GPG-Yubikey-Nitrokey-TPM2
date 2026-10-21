//! `generate-zbm`: build ZFSBootMenu images. See `generate-zbm(8)` and the [`generate_zbm`] library.

use std::io::Write;

fn main() {
    // Like the Perl program, end quietly on a closed pipe, and handle SIGINT and SIGTERM by unmounting the boot
    // partition and exiting with status 1 (child programs still receive the signals).
    let _ = zbm_sys::restore_default_sigpipe();
    let _ = zbm_sys::record_signals(&[zbm_sys::Signal::Interrupt, zbm_sys::Signal::Terminate]);

    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "generate-zbm".to_owned());
    let args: Vec<String> = args.collect();
    let runner = zbm_core::exec::SystemRunner;
    let context = generate_zbm::generate::Context::from_environment(&runner, program);

    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = generate_zbm::generate::run(
        &context,
        &args,
        &mut generate_zbm::generate::Io {
            stdout: &mut stdout.lock(),
            stderr: &mut stderr.lock(),
        },
    );
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
