//! How fast the console draws (ROADMAP.md C112, benchmark B9 in process):
//! the time to its first frame, `cat` of a 10 MB file and a coloured flood,
//! each frame presented into a buffer as the display's would be, with a
//! console font and with a TrueType font, on a 1920×1080 display; the
//! fastest of three runs.
//!
//! ```sh
//! cargo run --release -p zbm-console --example render
//! ```

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    reason = "a benchmark: it reads the repository's fonts, prints rates and stops at an error"
)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use zbm_console::screen::Console;
use zbm_display::display::copy_area;
use zbm_display::grid::Grid;
use zbm_display::outline::Outline;
use zbm_display::psf::Font;

/// The display's size.
const PIXELS: (u32, u32) = (1920, 1080);

/// How much the programs write between frames, as the console reads a
/// burst before it draws.
const CHUNK: usize = 64 * 1024;

/// How often each run is made.
const RUNS: usize = 3;

/// The fonts of the repository.
const FONTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../legacy/zfsbootmenu/fonts"
);

/// Lines of text, as `cat` of a log shows them, `bytes` in all.
fn text(bytes: usize) -> Vec<u8> {
    let mut out = String::with_capacity(bytes + 128);
    let mut n = 0_u64;
    while out.len() < bytes {
        n += 1;
        let _ = writeln!(
            out,
            "{n:>8}  zbm: imported zroot/ROOT/void read-only; 6.12.9_1 kernel, {} snapshots",
            n % 997
        );
    }
    out.into_bytes()
}

/// Lines in eight colours, as `ls --color` or a build log shows them.
fn coloured(bytes: usize) -> Vec<u8> {
    let mut out = String::with_capacity(bytes + 128);
    let mut n = 0_u64;
    while out.len() < bytes {
        n += 1;
        let colour = 31 + n % 7;
        let _ = writeln!(
            out,
            "\x1b[{colour}m{n:>8}\x1b[0m  \x1b[1mbuild\x1b[0m \
             crates/zbm-console/src/screen.rs \x1b[2m({} ms)\x1b[0m",
            n % 313
        );
    }
    out.into_bytes()
}

/// A display's buffer in memory, which frames are copied into.
struct Buffer {
    bytes: Vec<u8>,
    pitch: usize,
}

impl Buffer {
    fn new() -> Buffer {
        let pitch = usize::try_from(PIXELS.0).unwrap() * 4;
        Buffer {
            bytes: vec![0; pitch * usize::try_from(PIXELS.1).unwrap()],
            pitch,
        }
    }

    /// Copies what was drawn since the last frame, as presenting does.
    fn present(&mut self, console: &mut Console) {
        let frame = console.frame_mut();
        let areas = frame.take_damage();
        for area in areas {
            copy_area(frame, area, &mut self.bytes, self.pitch);
        }
    }
}

/// Feeds `bytes` in bursts, drawing and presenting after each; the time
/// and the frames.
fn run(console: &mut Console, buffer: &mut Buffer, bytes: &[u8]) -> (Duration, usize) {
    let start = Instant::now();
    let mut frames = 0;
    for chunk in bytes.chunks(CHUNK) {
        console.feed(chunk);
        if console.draw() > 0 {
            buffer.present(console);
            frames += 1;
        }
    }
    (start.elapsed(), frames)
}

/// The benchmark with the grid `make` makes.
fn bench(name: &str, make: &dyn Fn() -> Grid) {
    let start = Instant::now();
    let mut console = Console::new(make(), PIXELS);
    let mut buffer = Buffer::new();
    console.feed(b"zfsbootmenu / > ");
    console.draw();
    buffer.present(&mut console);
    let first = start.elapsed();
    let (columns, rows) = console.size();
    println!("{name}: {columns}x{rows} cells, first frame in {first:.1?}");
    for (what, bytes) in [
        ("cat of 10 MB", text(10 << 20)),
        ("coloured flood of 5 MB", coloured(5 << 20)),
    ] {
        // The fastest of three: the machine's other work only slows a run.
        let (took, frames) = (0..RUNS)
            .map(|_| run(&mut console, &mut buffer, &bytes))
            .min()
            .unwrap();
        let megabytes = bytes.len() as f64 / f64::from(1 << 20);
        println!(
            "  {what}: {took:.2?}, {:.1} MB/s, {frames} frames, {:.2} ms a frame",
            megabytes / took.as_secs_f64(),
            took.as_secs_f64() * 1000.0 / frames.max(1) as f64
        );
    }
}

fn main() {
    let psf = std::fs::read(format!("{FONTS}/ter-v32b.psf")).unwrap();
    let ttf = std::fs::read(format!("{FONTS}/dejavu/DejaVuSansMono.ttf")).unwrap();
    bench("ter-v32b.psf", &|| Grid::new(Font::parse(&psf).unwrap(), 1));
    bench("DejaVuSansMono.ttf 24 px", &|| {
        Grid::outline(Outline::new(ttf.clone(), 24).unwrap())
    });
}
