//! How fast the menu shows many snapshots (ROADMAP.md B5, in process): the
//! demo machine with 100, 1,000 and 10,000 snapshots on its default
//! environment, the menu drawn on a terminal in memory at 120×40 and
//! 80×24. It times opening the snapshots (their load, ranking and the
//! first frame), typing a filter, scrolling a page and the palette (opening
//! it, and typing in it), each key with the frame it draws, and reports the
//! median and the worst of each, and the peak memory of each count. The
//! budget is a frame, 16 ms (B4), at 10,000.
//!
//! ```sh
//! cargo run --release -p zbm-tui --example snapshots
//! ```

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    reason = "a benchmark: it reads its own peak memory, prints times and stops at an error"
)]

use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::{DemoOptions, Session};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::view::view;

/// The counts of snapshots measured.
const COUNTS: [usize; 3] = [100, 1_000, 10_000];

/// The terminal sizes measured.
const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

/// How often each measure is made.
const RUNS: usize = 5;

/// A frame (B4).
const FRAME: Duration = Duration::from_millis(16);

/// What a menu and its terminal measure.
struct Bench {
    session: Session,
    terminal: Terminal<TestBackend>,
}

impl Bench {
    /// The menu on the demo machine with `count` snapshots, its main view
    /// drawn at `size`, the countdown stopped.
    fn new(count: usize, (width, height): (u16, u16)) -> Bench {
        let options = DemoOptions {
            snapshots: count,
            timeout: None,
            ..DemoOptions::default()
        };
        let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
        session.start();
        let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut bench = Bench { session, terminal };
        bench.draw();
        bench
    }

    fn draw(&mut self) {
        let model = self.session.model();
        self.terminal.draw(|frame| view(model, frame)).unwrap();
    }

    /// The time `key` takes, with the frame it draws.
    fn press(&mut self, key: KeyPress) -> Duration {
        let start = Instant::now();
        self.session.press(key);
        self.draw();
        start.elapsed()
    }

    /// The time of each key of `text`, typed.
    fn type_text(&mut self, text: &str) -> Vec<Duration> {
        text.chars()
            .map(|c| self.press(KeyPress::new(Key::Char(c))))
            .collect()
    }
}

/// The times of a measure, all runs together.
#[derive(Default)]
struct Times {
    open: Vec<Duration>,
    filter: Vec<Duration>,
    scroll: Vec<Duration>,
    palette_open: Vec<Duration>,
    palette_type: Vec<Duration>,
}

/// One run at `count` snapshots and `size`.
fn run(count: usize, size: (u16, u16), times: &mut Times) {
    let mut bench = Bench::new(count, size);
    times.open.push(bench.press(KeyPress::new(Key::Char('s'))));
    for _ in 0..10 {
        times.scroll.push(bench.press(KeyPress::new(Key::PageDown)));
    }
    bench.press(KeyPress::new(Key::Char('/')));
    times.filter.extend(bench.type_text("2026-0"));
    bench.press(KeyPress::new(Key::Escape));
    bench.press(KeyPress::new(Key::Escape));
    times
        .palette_open
        .push(bench.press(KeyPress::new(Key::Char(':'))));
    times.palette_type.extend(bench.type_text("2026-09"));
}

/// The median and the worst of `times`, in milliseconds.
fn summary(times: &mut [Duration]) -> String {
    times.sort_unstable();
    let ms = |time: Duration| time.as_secs_f64() * 1000.0;
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let worst = times.last().copied().unwrap_or_default();
    let over = if worst > FRAME { " !" } else { "" };
    format!("{:7.2} {:7.2}{over}", ms(median), ms(worst))
}

/// The peak resident memory of the process since it was last reset, in
/// KiB, from `/proc/self/status`.
fn peak_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or_default()
}

/// Resets the peak resident memory to what the process holds now.
fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

fn main() {
    println!("B5: median and worst of {RUNS} runs, in ms ('!' over a frame of 16 ms)");
    println!(
        "{:>6} {:>7}  {:>15} {:>15} {:>15} {:>15} {:>15} {:>9}",
        "count", "size", "open", "filter key", "page down", "palette", "palette key", "peak KiB"
    );
    for count in COUNTS {
        for size in SIZES {
            reset_peak();
            let mut times = Times::default();
            for _ in 0..RUNS {
                run(count, size, &mut times);
            }
            println!(
                "{count:>6} {:>7}  {} {} {} {} {} {:>9}",
                format!("{}x{}", size.0, size.1),
                summary(&mut times.open),
                summary(&mut times.filter),
                summary(&mut times.scroll),
                summary(&mut times.palette_open),
                summary(&mut times.palette_type),
                peak_kib()
            );
        }
    }
}
