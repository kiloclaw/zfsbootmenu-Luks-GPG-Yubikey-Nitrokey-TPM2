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
    // 1 (child programs still receive them): they wait to be read, blocked
    // first, before any thread starts.
    let stopping = [zbm_sys::Signal::Interrupt, zbm_sys::Signal::Terminate];
    let signals = zbm_sys::Signals::block(zbm_sys::SignalSet::of(&stopping)).ok();
    let signal = || {
        signals
            .as_ref()
            .and_then(|signals| signals.received().first())
    };

    let (program, args) = match zbm_platform::args::split(std::env::args_os(), "generate-zbm") {
        Ok(split) => split,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            std::process::exit(2);
        }
    };
    let runner = zbm_platform::exec::SystemRunner;
    let context = generate_zbm::generate::Context::from_environment(&runner, program, &signal);

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
