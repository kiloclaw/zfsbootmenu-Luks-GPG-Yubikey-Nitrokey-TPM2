//! How fast the menu shows many snapshots (ROADMAP.md B5, in process): the
//! demo machine with 100, 1,000 and 10,000 snapshots on its default
//! environment, the menu drawn on a terminal in memory at 120×40 and
//! 80×24, with each key map: in the workspace the snapshots are its
//! timeline. It times opening the snapshots (their load, ranking and the
//! first frame), scrolling a page, sorting them (`o`), folding the group
//! of the selection and unfolding it (the workspace's `z`), typing a
//! filter and the palette (opening it, and typing in it), the history of
//! the palette shown and hidden (`Ctrl-R`), and in the workspace a dry
//! run, from `:` to its plan drawn, each key with the frame it draws; the
//! workspace once more locked (ADR 0010), its entries marked. It reports
//! the median and the worst of each, and the peak memory of each count.
//! The budget is a frame, 16 ms (B4), at 10,000.
//!
//! ```sh
//! cargo run --release -p zbm-tui --example snapshots
//! cargo run --release -p zbm-tui --example snapshots -- --runs 20
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
use zbm_tui::menu::model::Dialog;
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::view::view;

/// The counts of snapshots measured.
const COUNTS: [usize; 3] = [100, 1_000, 10_000];

/// The terminal sizes measured.
const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

/// The menus measured, by name: the key maps, the workspace once more
/// locked.
const MENUS: [Menu; 3] = [
    Menu {
        name: "classic",
        workspace: false,
        locked: false,
    },
    Menu {
        name: "workspace",
        workspace: true,
        locked: false,
    },
    Menu {
        name: "locked",
        workspace: true,
        locked: true,
    },
];

/// How often each measure is made, unless `--runs` says.
const RUNS: usize = 5;

/// A frame (B4).
const FRAME: Duration = Duration::from_millis(16);

/// A menu measured: in the workspace or the classic key map, locked or
/// not.
#[derive(Clone, Copy)]
struct Menu {
    name: &'static str,
    workspace: bool,
    locked: bool,
}

/// What a menu and its terminal measure.
struct Bench {
    session: Session,
    terminal: Terminal<TestBackend>,
}

impl Bench {
    /// The menu on the demo machine with `count` snapshots, its main view
    /// drawn at `size`, the countdown stopped, as `menu` says.
    fn new(count: usize, (width, height): (u16, u16), menu: Menu) -> Bench {
        let options = DemoOptions {
            snapshots: count,
            timeout: None,
            classic: !menu.workspace,
            menu_locked: menu.locked,
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

    /// The time from `:` to the plan drawn: `query` typed, and `Enter` on
    /// the entry it finds first, which opens its dry run; with no query,
    /// the entry chosen last. The page is closed.
    fn dry_run(&mut self, query: &str) -> Duration {
        let planned = self.session.planned();
        let open = self.press(KeyPress::new(Key::Char(':')));
        let typed: Duration = self.type_text(query).into_iter().sum();
        let enter = self.press(KeyPress::new(Key::Enter));
        let model = self.session.model();
        let drawn = matches!(&model.dialog, Some(Dialog::Review(page)) if page.plan.is_some());
        assert!(
            drawn && self.session.planned() == planned + 1,
            "the dry run of {query:?} opened"
        );
        self.press(KeyPress::new(Key::Escape));
        let model = self.session.model();
        assert!(model.dialog.is_none() && model.palette.is_none());
        open + typed + enter
    }
}

/// The times of a measure, all runs together.
#[derive(Default)]
struct Times {
    open: Vec<Duration>,
    filter: Vec<Duration>,
    scroll: Vec<Duration>,
    sort: Vec<Duration>,
    fold: Vec<Duration>,
    palette_open: Vec<Duration>,
    palette_type: Vec<Duration>,
    history: Vec<Duration>,
    dry_run: Vec<Duration>,
}

/// One run at `count` snapshots and `size`, in `menu`: the workspace's
/// tab `2` opens the snapshots, and `z` folds; the workspace's dry run
/// exports a pool, chosen by name, then again as the entry chosen last,
/// which is timed; the classic map has none. Then, the query empty, the
/// history is shown alone and hidden: the export in the workspace, no
/// entry in the classic map.
fn run(count: usize, size: (u16, u16), menu: Menu, times: &mut Times) {
    let workspace = menu.workspace;
    let mut bench = Bench::new(count, size, menu);
    let open = if workspace { '2' } else { 's' };
    times.open.push(bench.press(KeyPress::new(Key::Char(open))));
    for _ in 0..10 {
        times.scroll.push(bench.press(KeyPress::new(Key::PageDown)));
    }
    times.sort.push(bench.press(KeyPress::new(Key::Char('o'))));
    if workspace {
        for _ in 0..2 {
            times.fold.push(bench.press(KeyPress::new(Key::Char('z'))));
        }
    }
    bench.press(KeyPress::new(Key::Char('/')));
    times.filter.extend(bench.type_text("2026-0"));
    bench.press(KeyPress::new(Key::Escape));
    bench.press(KeyPress::new(Key::Escape));
    times
        .palette_open
        .push(bench.press(KeyPress::new(Key::Char(':'))));
    times.palette_type.extend(bench.type_text("2026-09"));
    bench.press(KeyPress::new(Key::Escape));
    if workspace {
        bench.dry_run("export");
        times.dry_run.push(bench.dry_run(""));
    }
    bench.press(KeyPress::new(Key::Char(':')));
    for shown in [true, false] {
        let ctrl_r = KeyPress::new(Key::Char('r')).with_ctrl();
        times.history.push(bench.press(ctrl_r));
        let palette = bench.session.model().palette.as_ref().unwrap();
        let entries = palette.matches.len();
        assert!(
            palette.recent == shown && (!shown || entries == usize::from(workspace)),
            "the history shown: {shown}, {entries} entries"
        );
    }
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

/// The number of runs `--runs N` asks for, or [`RUNS`]; `None` for any
/// other arguments, a count that is not a number or a count of 0.
fn runs_of(args: &[String]) -> Option<usize> {
    match args {
        [] => Some(RUNS),
        [flag, count] if flag == "--runs" => count.parse().ok().filter(|&n| n > 0),
        _ => None,
    }
}

/// The number of runs the arguments ask for; the usage and exit status 2
/// for bad arguments.
fn runs() -> usize {
    let args: Vec<String> = std::env::args().skip(1).collect();
    runs_of(&args).unwrap_or_else(|| {
        eprintln!("usage: snapshots [--runs N], N at least 1");
        std::process::exit(2);
    })
}

fn main() {
    let runs = runs();
    println!("B5: median and worst of {runs} runs, in ms ('!' over a frame of 16 ms)");
    println!(
        "{:>9} {:>6} {:>7}  {:>15} {:>15} {:>15} {:>15} {:>15} {:>15} {:>15} {:>15} {:>15} {:>9}",
        "menu",
        "count",
        "size",
        "open",
        "filter key",
        "page down",
        "sort",
        "fold",
        "palette",
        "palette key",
        "Ctrl-R",
        "dry run opened",
        "peak KiB"
    );
    for menu in MENUS {
        for count in COUNTS {
            for size in SIZES {
                reset_peak();
                let mut times = Times::default();
                for _ in 0..runs {
                    run(count, size, menu, &mut times);
                }
                let workspace_only = |times: &mut Vec<Duration>| {
                    if menu.workspace {
                        summary(times)
                    } else {
                        format!("{:>15}", "-")
                    }
                };
                let fold = workspace_only(&mut times.fold);
                let dry_run = workspace_only(&mut times.dry_run);
                println!(
                    "{:>9} {count:>6} {:>7}  {} {} {} {} {fold} {} {} {} {dry_run} {:>9}",
                    menu.name,
                    format!("{}x{}", size.0, size.1),
                    summary(&mut times.open),
                    summary(&mut times.filter),
                    summary(&mut times.scroll),
                    summary(&mut times.sort),
                    summary(&mut times.palette_open),
                    summary(&mut times.palette_type),
                    summary(&mut times.history),
                    peak_kib()
                );
            }
        }
    }
}
