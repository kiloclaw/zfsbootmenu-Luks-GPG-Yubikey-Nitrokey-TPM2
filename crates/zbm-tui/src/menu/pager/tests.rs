use std::collections::VecDeque;
use std::os::fd::BorrowedFd;
use std::time::Duration;

use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zbm_display::keys::Key;

use super::*;
use crate::menu::keymap::{Binding, Modifiers, Scope, commands, describe, press};

/// Keys from a script; the input ends after them, as a terminal hung up.
struct Script(VecDeque<KeyPress>);

impl Input for Script {
    fn key_or_wake(
        &mut self,
        _: Duration,
        _: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        let key = self.0.pop_front();
        key.map(Some)
            .ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }
}

fn plain(key: Key) -> KeyPress {
    press(key, Modifiers::NONE)
}

fn ctrl(c: char) -> KeyPress {
    press(Key::Char(c), Modifiers::CTRL)
}

/// A pager of the kernel log with `count` numbered lines.
fn numbered(count: usize) -> Pager {
    Pager {
        topic: Topic::Logs,
        lines: (1..=count).map(|n| format!("line {n}")).collect(),
    }
}

/// `pager` shown on a test terminal with the key map `map` until `keys`
/// leave it or run out: whether they left it, and what the terminal shows
/// after.
fn shown_until(pager: &Pager, map: Map, keys: &[KeyPress]) -> (bool, String) {
    let script = Script(keys.iter().copied().collect());
    let mut frontend = Frontend::new(TestBackend::new(80, 25), script).unwrap();
    let theme = Theme::new(Capabilities::FULL);
    let left = run(&mut frontend, pager, (theme, map)).is_ok();
    (left, text_of(frontend.terminal().backend()))
}

/// The text of `backend`, a line per row, without trailing blanks.
fn text_of(backend: &TestBackend) -> String {
    let buffer = backend.buffer();
    let area = buffer.area;
    let mut text = String::new();
    for y in 0..area.height {
        let row: String = (0..area.width)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect();
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

/// What `reading` draws in `capabilities` with the key map `map` at
/// `width` by `height`, and the area it gives the lines.
fn drawn(
    reading: &Reading<'_>,
    map: Map,
    capabilities: Capabilities,
    (width, height): (u16, u16),
) -> (String, Rect) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut area = Rect::default();
    let look = (Theme::new(capabilities), map);
    terminal
        .draw(|frame| area = view::draw(frame, reading, look))
        .unwrap();
    (text_of(terminal.backend()), area)
}

/// `reading` after `keys`, read with the key map `map`, its area as an
/// 80×25 frame gives it; whether one of them left it.
fn after(reading: &mut Reading<'_>, map: Map, keys: &[KeyPress]) -> bool {
    after_in(reading, map, (80, 25), keys)
}

/// `reading` after `keys`, read with the key map `map`, its area as a frame
/// of `size` gives it; whether one of them left it.
fn after_in(reading: &mut Reading<'_>, map: Map, size: (u16, u16), keys: &[KeyPress]) -> bool {
    let mut left = false;
    for key in keys {
        reading.area = drawn(reading, map, Capabilities::FULL, size).1;
        left |= reading.press(map, *key) == After::Leave;
    }
    left
}

/// The pager of the wireframes: the kernel log's errors and warnings, one
/// line with an escape a device wrote, and more lines than a screen holds.
pub(crate) fn wireframe(capabilities: Capabilities, (width, height): (u16, u16)) -> String {
    let pager = wireframe_pager();
    let reading = Reading::new(&pager);
    drawn(&reading, Map::Workspace, capabilities, (width, height)).0
}

/// The lines of the wireframes, most of them two rows at 80 columns.
fn wireframe_pager() -> Pager {
    let mut lines = vec![
        "[    2.104317] ZFSBootMenu: zroot was last imported by another host".to_owned(),
        "[    2.231055] ZFSBootMenu: unable to load the key of zroot/ROOT/void".to_owned(),
        "[    3.900112] usb 1-2: device descriptor read/64, error -71".to_owned(),
        "[    4.017741] sd 2:0:0:0: [sdb] label \u{1b}]0;owned\u{7} has no partitions".to_owned(),
    ];
    let long = "of a long log, wrapped where it is too long for the screen";
    lines.extend((5..=48).map(|n| format!("[   {n:>2}.000000] ZFSBootMenu: warning {n} {long}")));
    Pager {
        topic: Topic::Logs,
        lines,
    }
}

#[test]
fn q_escape_and_ctrl_c_leave_the_pager_and_h_in_the_workspace() {
    let pager = numbered(3);
    for map in [Map::Classic, Map::Workspace] {
        for key in [plain(Key::Char('q')), plain(Key::Escape), ctrl('c')] {
            let (left, screen) = shown_until(&pager, map, &[key]);
            assert!(left, "{map:?} {key:?}");
            // The terminal is given back cleared.
            assert_eq!(screen.trim(), "", "{map:?} {key:?}");
        }
    }
    // `h` moves back in the workspace, as in every view; in the classic
    // map it means Rewind in the Pools view alone, and nothing here.
    let h = plain(Key::Char('h'));
    assert!(shown_until(&pager, Map::Workspace, &[h]).0);
    assert!(!shown_until(&pager, Map::Classic, &[h]).0);
    // Keys that do not leave it leave it waiting; the input that ends is an
    // error, and the terminal is given back all the same.
    let (left, screen) = shown_until(&pager, Map::Classic, &[plain(Key::Down)]);
    assert!(!left);
    assert_eq!(screen.trim(), "");
}

#[test]
fn slash_filters_the_lines_and_escape_clears_it() {
    let pager = Pager {
        topic: Topic::Trace,
        lines: ["mount zroot", "IMPORT tank", "boot", "import zroot"]
            .map(str::to_owned)
            .to_vec(),
    };
    for map in [Map::Classic, Map::Workspace] {
        let mut reading = Reading::new(&pager);
        let typed: Vec<KeyPress> = "/imp".chars().map(|c| plain(Key::Char(c))).collect();
        assert!(!after(&mut reading, map, &typed));
        assert!(reading.typing);
        assert_eq!(reading.context(), Context::Text);
        assert_eq!(reading.kept(), ["IMPORT tank", "import zroot"]);
        let (screen, _) = drawn(&reading, map, Capabilities::FULL, (80, 25));
        assert!(screen.contains("/ imp"), "{screen}");
        assert!(screen.contains("1-2 of 2"), "{screen}");
        assert!(!screen.contains("mount zroot"), "{screen}");
        // Letters type in the field, `q` too; Backspace and Ctrl-U edit it.
        after(&mut reading, map, &[plain(Key::Char('q'))]);
        assert_eq!(reading.filter, "impq");
        let (screen, _) = drawn(&reading, map, Capabilities::FULL, (80, 25));
        assert!(screen.contains("No line holds “impq”"), "{screen}");
        after(&mut reading, map, &[plain(Key::Backspace)]);
        assert_eq!(reading.filter, "imp");
        // Enter keeps the filter and gives the keys back to the pager.
        after(&mut reading, map, &[plain(Key::Enter)]);
        assert!(!reading.typing);
        assert_eq!(reading.kept().len(), 2);
        // Esc clears it before it leaves.
        assert!(!after(&mut reading, map, &[plain(Key::Escape)]));
        assert_eq!(reading.filter, "");
        assert_eq!(reading.kept().len(), 4);
        // Esc in the field clears it and ends the typing; so does `q` out
        // of the field, whose filter is kept.
        after(
            &mut reading,
            map,
            &[plain(Key::Char('/')), plain(Key::Char('b'))],
        );
        assert_eq!(reading.kept(), ["boot"]);
        after(&mut reading, map, &[plain(Key::Escape)]);
        assert!(!reading.typing);
        assert_eq!(reading.filter, "");
        after(&mut reading, map, &[plain(Key::Char('/')), ctrl('u')]);
        assert_eq!(reading.filter, "");
        let keys = [plain(Key::Char('z')), plain(Key::Enter)];
        after(&mut reading, map, &keys);
        assert!(!after(&mut reading, map, &[plain(Key::Char('q'))]));
        assert_eq!(reading.filter, "");
        assert!(after(&mut reading, map, &[plain(Key::Char('q'))]));
    }
}

#[test]
fn f1_and_the_tab_keys_do_nothing_in_the_pager() {
    let pager = numbered(100);
    let keys = [
        plain(Key::F(1)),
        plain(Key::Tab),
        plain(Key::BackTab),
        plain(Key::Char('1')),
        plain(Key::Char('4')),
        plain(Key::Char('?')),
        plain(Key::Char(':')),
        ctrl('l'),
    ];
    for map in [Map::Classic, Map::Workspace] {
        let mut reading = Reading::new(&pager);
        after(&mut reading, map, &[plain(Key::Down)]);
        let before = drawn(&reading, map, Capabilities::FULL, (80, 25));
        for key in keys {
            assert!(!after(&mut reading, map, &[key]), "{map:?} {key:?}");
            assert_eq!(
                drawn(&reading, map, Capabilities::FULL, (80, 25)),
                before,
                "{map:?} {key:?}"
            );
        }
        // The toolbar offers neither help nor the tabs.
        assert!(!before.0.contains("More"), "{}", before.0);
        // In the filter's field F1 opens nothing either.
        after(&mut reading, map, &[plain(Key::Char('/'))]);
        assert!(!after(&mut reading, map, &[plain(Key::F(1))]));
        assert!(reading.typing);
    }
}

#[test]
fn without_a_terminal_or_with_term_dumb_the_lines_are_printed() {
    let dumb = Capabilities::detect(|name| (name == "TERM").then(|| "dumb".to_owned()));
    let unset = Capabilities::detect(|_| None);
    let linux = Capabilities::detect(|name| (name == "TERM").then(|| "linux".to_owned()));
    assert!(!is_drawn(dumb, true, true));
    assert!(!is_drawn(unset, true, true));
    assert!(!is_drawn(linux, true, false));
    // Keys come from standard input: without a terminal there, as with
    // `zbm logs </dev/null`, the lines are printed, not a frame that waits.
    assert!(!is_drawn(linux, false, true));
    assert!(is_drawn(linux, true, true));
    // As they are: the title is not printed, and nothing is escaped.
    let pager = Pager {
        topic: Topic::Commands,
        lines: vec!["zfs-chroot FILESYSTEM".to_owned(), "\tlogs".to_owned()],
    };
    let mut out = Vec::new();
    print(&pager, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "zfs-chroot FILESYSTEM\n\tlogs\n"
    );
}

#[test]
fn the_pager_draws_no_text_raw() {
    let pager = Pager {
        topic: Topic::Logs,
        lines: vec!["sdb: \u{1b}]0;owned\u{7}label\u{202e}gpj.exe".to_owned()],
    };
    let mut reading = Reading::new(&pager);
    let (screen, _) = drawn(&reading, Map::Workspace, Capabilities::FULL, (80, 25));
    assert!(
        screen.contains("sdb: \\x1b]0;owned\\x07label\\u{202e}gpj.exe"),
        "{screen}"
    );
    // A control character typed into the filter shows escaped too.
    after(&mut reading, Map::Workspace, &[plain(Key::Char('/'))]);
    after(&mut reading, Map::Workspace, &[plain(Key::Char('\u{7}'))]);
    let (screen, _) = drawn(&reading, Map::Workspace, Capabilities::FULL, (80, 25));
    assert!(screen.contains("/ \\x07"), "{screen}");
    assert!(screen.contains("1-1 of 1"), "{screen}");
    for c in screen.chars().filter(|c| *c != '\n') {
        assert!(!zbm_types::must_escape(c), "{c:?} in {screen}");
    }
}

#[test]
fn the_pager_binds_only_q_and_slash_and_keeps_one_meaning() {
    let pager = Scope::Only(&[Context::Pager]);
    for map in [Map::Classic, Map::Workspace] {
        let own: Vec<(Key, Intent)> = map
            .table()
            .into_iter()
            .filter(|binding: &&Binding| binding.scope == pager)
            .map(|binding| (binding.key, binding.intent))
            .collect();
        assert_eq!(
            own,
            [
                (Key::Char('q'), Intent::Back),
                (Key::Char('/'), Intent::Filter)
            ],
            "{map:?}"
        );
        // `q` means nothing elsewhere; `/` filters a list, as in the pager.
        let q = keymap::input(map, Context::Main, plain(Key::Char('q')));
        assert_eq!(q, None, "{map:?}");
        let slash = keymap::input(map, Context::Main, plain(Key::Char('/')));
        assert_eq!(slash, Some(keymap::Input::Intent(Intent::Filter)));
        // The pager has no command line.
        assert_eq!(commands(map, Context::Pager), [], "{map:?}");
    }
    // The classic `j`, `k` and `h` keep their meaning, and do nothing in
    // the pager.
    let classic = |context, c| keymap::input(Map::Classic, context, plain(Key::Char(c)));
    for c in ['j', 'k', 'h'] {
        assert_eq!(classic(Context::Pager, c), None, "{c}");
    }
    let chroot = Some(keymap::Input::Intent(Intent::Chroot));
    assert_eq!(classic(Context::Main, 'j'), chroot);
}

#[test]
fn the_workspace_movers_and_the_arrows_scroll_the_pager() {
    let pager = numbered(100);
    let mut reading = Reading::new(&pager);
    let map = Map::Workspace;
    let cases = [
        (plain(Key::Char('j')), 1),
        (plain(Key::Down), 2),
        (ctrl('n'), 3),
        (plain(Key::Char('k')), 2),
        (plain(Key::Up), 1),
        (ctrl('p'), 0),
        (plain(Key::PageDown), 22),
        (plain(Key::PageDown), 44),
        (plain(Key::PageUp), 22),
        (plain(Key::End), 78),
        (plain(Key::Down), 78),
        (plain(Key::Home), 0),
        (plain(Key::Up), 0),
    ];
    for (key, scroll) in cases {
        after(&mut reading, map, &[key]);
        assert_eq!(reading.scroll, scroll, "{key:?}");
    }
    // A page is the rows a frame gives the lines: 22 in 80×25, inside the
    // border, above the toolbar.
    after(&mut reading, map, &[plain(Key::End)]);
    let (screen, area) = drawn(&reading, map, Capabilities::FULL, (80, 25));
    assert_eq!(area.height, 22);
    assert!(screen.contains("79-100 of 100"), "{screen}");
    assert!(screen.contains("line 100"), "{screen}");
    assert!(screen.contains("Esc Leave"), "{screen}");
    assert!(screen.contains("/ Filter"), "{screen}");
    assert!(screen.contains("Scroll"), "{screen}");
}

#[test]
fn the_classic_pager_scrolls_with_the_arrows() {
    let pager = numbered(100);
    let mut reading = Reading::new(&pager);
    let map = Map::Classic;
    after(&mut reading, map, &[plain(Key::Down), plain(Key::Down)]);
    assert_eq!(reading.scroll, 2);
    after(
        &mut reading,
        map,
        &[plain(Key::Char('j')), plain(Key::Char('k'))],
    );
    assert_eq!(reading.scroll, 2);
    after(&mut reading, map, &[plain(Key::Up), plain(Key::PageDown)]);
    assert_eq!(reading.scroll, 23);
    after(&mut reading, map, &[plain(Key::End), plain(Key::PageUp)]);
    assert_eq!(reading.scroll, 56);
    // A long line wraps; the lines shown count it once.
    let long = Pager {
        topic: Topic::Options,
        lines: vec!["word ".repeat(40), "short".to_owned()],
    };
    let reading = Reading::new(&long);
    let (screen, _) = drawn(&reading, map, Capabilities::ASCII, (52, 20));
    assert!(screen.contains("1-2 of 2"), "{screen}");
    assert!(screen.contains("| short"), "{screen}");
}

#[test]
fn end_shows_the_last_line_whole_when_lines_wrap() {
    // The wireframe's log at 80×25: 22 rows, the last 44 lines two each.
    let pager = wireframe_pager();
    for map in [Map::Classic, Map::Workspace] {
        let mut reading = Reading::new(&pager);
        after(&mut reading, map, &[plain(Key::End)]);
        let (screen, _) = drawn(&reading, map, Capabilities::FULL, (80, 25));
        assert!(screen.contains("38-48 of 48"), "{screen}");
        assert!(screen.contains("warning 48 of a long log"), "{screen}");
        let rest = screen.split("warning 48").nth(1).unwrap_or_default();
        assert!(rest.contains("for the screen"), "{screen}");
        // The last page shows the last line whole: Down and PgDn stay.
        let last = reading.scroll;
        after(&mut reading, map, &[plain(Key::Down), plain(Key::PageDown)]);
        assert_eq!(reading.scroll, last);
    }
    // One-row lines, then a last line of two rows in 20 rows: the last
    // page starts where both its rows fit.
    let mut lines: Vec<String> = (1..=40).map(|n| format!("line {n}")).collect();
    lines.push(format!("last {}", "#".repeat(100)));
    let pager = Pager {
        topic: Topic::Trace,
        lines,
    };
    let mut reading = Reading::new(&pager);
    // 80×23: 20 rows inside the border, above the toolbar.
    after_in(&mut reading, Map::Workspace, (80, 23), &[plain(Key::End)]);
    assert_eq!(reading.scroll, 22);
    let (screen, _) = drawn(&reading, Map::Workspace, Capabilities::FULL, (80, 23));
    assert!(screen.contains("23-41 of 41"), "{screen}");
    assert_eq!(screen.matches('#').count(), 100, "{screen}");
}

#[test]
fn a_page_down_starts_at_the_line_the_page_cut() {
    // At 80×24 the wireframe's lines fill 21 rows: 4 short ones and 8 of
    // two rows whole, and the first row of the 13th.
    let pager = wireframe_pager();
    let mut reading = Reading::new(&pager);
    let map = Map::Workspace;
    let (screen, _) = drawn(&reading, map, Capabilities::FULL, (80, 24));
    assert!(screen.contains("1-13 of 48"), "{screen}");
    after_in(&mut reading, map, (80, 24), &[plain(Key::PageDown)]);
    assert_eq!(reading.scroll, 12);
    let (screen, _) = drawn(&reading, map, Capabilities::FULL, (80, 24));
    assert!(screen.contains("warning 13 of a long log"), "{screen}");
    // A page up shows again the lines above, whole.
    after_in(&mut reading, map, (80, 24), &[plain(Key::PageUp)]);
    assert_eq!(reading.scroll, 0);
    // A line taller than the area is passed a line at a time.
    let tall = Pager {
        topic: Topic::Trace,
        lines: vec!["y".repeat(80 * 30), "end".to_owned()],
    };
    let mut reading = Reading::new(&tall);
    after(&mut reading, map, &[plain(Key::PageDown)]);
    assert_eq!(reading.scroll, 1);
    after(&mut reading, map, &[plain(Key::PageUp)]);
    assert_eq!(reading.scroll, 0);
}

#[test]
fn wide_characters_wrap_by_their_columns() {
    // A pool named in a wide script: 60 characters of two columns each in
    // the 50 columns inside a 52-column pager.
    let name = "池".repeat(60);
    let pager = Pager {
        topic: Topic::Logs,
        lines: vec![format!("pool {name}"), "after".to_owned()],
    };
    let reading = Reading::new(&pager);
    let (screen, _) = drawn(&reading, Map::Workspace, Capabilities::FULL, (52, 20));
    assert_eq!(screen.matches('池').count(), 60, "{screen}");
    assert!(screen.contains("1-2 of 2"), "{screen}");
    assert!(screen.contains("after"), "{screen}");
    // Rows are cut by columns: "pool " and 22 wide characters, then 25 and
    // 13; an empty line is one row.
    assert_eq!(view::height(&format!("pool {name}"), 50), 3);
    assert_eq!(view::height("", 50), 1);
    // A wide character wider than the area takes a row of its own.
    assert_eq!(view::height("池池", 1), 2);
}

#[test]
fn the_pager_s_actions_have_titles() {
    for map in [Map::Classic, Map::Workspace] {
        for context in [Context::Pager, Context::Text] {
            for binding in map.table() {
                if !map.is_bound(binding, context) {
                    continue;
                }
                let what = describe(context, binding.intent);
                let title = text::title(what);
                assert_ne!(title, format!("{}.title", what.as_str()), "{what:?}");
            }
        }
    }
    assert_eq!(text::title(describe(Context::Pager, Intent::Back)), "Leave");
    assert_eq!(
        text::title(describe(Context::Pager, Intent::Down)),
        "Scroll"
    );
}

#[test]
fn every_topic_has_a_title() {
    let topics = [
        Topic::Commands,
        Topic::Keys,
        Topic::Options,
        Topic::Logs,
        Topic::Trace,
    ];
    let titles: Vec<String> = topics.iter().map(|topic| topic.title()).collect();
    assert_eq!(
        titles,
        ["Commands", "Keys", "Options", "Kernel Log", "Trace"]
    );
    // Each in the pager's border.
    for topic in topics {
        let pager = Pager {
            topic,
            lines: Vec::new(),
        };
        let reading = Reading::new(&pager);
        let (screen, _) = drawn(&reading, Map::Workspace, Capabilities::FULL, (52, 20));
        let header = format!(" {} ", topic.title().to_uppercase());
        assert!(screen.contains(&header), "{screen}");
        assert!(screen.contains("0-0 of 0"), "{screen}");
    }
}
