//! `zbm`: the ZFSBootMenu runtime, one program for all of its commands. See the
//! [`zbm`] library.

use std::io::Write;

fn main() -> std::process::ExitCode {
    // Unlike the host tools, the runtime keeps SIGPIPE ignored (the Rust
    // default): a program that exits without reading all of its input, such as
    // `cryptsetup` refusing a device while a key is written to it, must not end
    // the process that feeds it. Writes to a closed pipe fail instead, and the
    // runtime handles the failure (see "Signals" in crates/DESIGN.md).
    // Secrets (ROADMAP.md #1): no core dump of the runtime, and no process but
    // root's may read its memory. A failure leaves it as it was.
    let _ = zbm_sys::memory::set_undumpable();
    let (argv0, args) = match zbm_platform::args::split(std::env::args_os(), "zbm") {
        Ok(split) => split,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            return std::process::ExitCode::from(2);
        }
    };
    // A panic leaves a crash report in the state directory (see
    // `zbm_platform::crash`); where that does not exist, as on a booted system,
    // only the standard message on standard error remains.
    zbm_platform::CrashReporter::new(
        &zbm_platform::RealFiles,
        std::path::Path::new(zbm::debug::BASE),
        &zbm::applet_name(&argv0, &args),
    )
    .install();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = zbm::run(
        &argv0,
        &args,
        &mut zbm::Io {
            stdout: &mut stdout.lock(),
            stderr: &mut stderr.lock(),
        },
    );
    let _ = std::io::stdout().flush();
    // As `exit` would: the status modulo 256.
    std::process::ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1))
}
