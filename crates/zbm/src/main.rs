//! `zbm`: the ZFSBootMenu runtime, one program for all of its commands. See the
//! [`zbm`] library.

use std::io::Write;

fn main() {
    // Unlike the host tools, the runtime keeps SIGPIPE ignored (the Rust
    // default): a program that exits without reading all of its input, such as
    // `cryptsetup` refusing a device while a key is written to it, must not end
    // the process that feeds it. Writes to a closed pipe fail instead, and the
    // runtime handles the failure (see "Signals" in crates/DESIGN.md).
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_else(|| "zbm".to_owned());
    let args: Vec<String> = args.collect();
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
    std::process::exit(code);
}
