//! `zbm`: the ZFSBootMenu runtime, one program for all of its commands. See the
//! [`zbm`] library.

use std::io::Write;

fn main() {
    let _ = zbm_sys::restore_default_sigpipe();
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_else(|| "zbm".to_owned());
    let args: Vec<String> = args.collect();
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
