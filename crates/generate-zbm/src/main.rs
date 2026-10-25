//! `generate-zbm`: build ZFSBootMenu images. See `generate-zbm(8)` and the
//! [`generate_zbm`] library.

use std::io::Write;

fn main() {
    // A closed pipe (`generate-zbm | less`, quit early) only makes writes fail,
    // and the program goes on to its end, as the Perl one does with its output
    // buffered on the pipe: dying there could leave the old image renamed to
    // its backup and the new one not in place. SIGPIPE stays ignored, as the
    // standard library sets it; programs it starts get the default action
    // back. SIGINT and SIGTERM unmount the boot partition and exit with status
    // 1 (child programs still receive them).
    let _ = zbm_sys::record_signals(&[zbm_sys::Signal::Interrupt, zbm_sys::Signal::Terminate]);

    let (program, args) = match zbm_core::args::split(std::env::args_os(), "generate-zbm") {
        Ok(split) => split,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            std::process::exit(2);
        }
    };
    let runner = zbm_platform::exec::SystemRunner;
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
