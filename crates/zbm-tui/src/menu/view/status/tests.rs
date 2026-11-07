use pretty_assertions::assert_eq;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::data::NOW;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};
use crate::menu::view::tests::screen;

/// The demo's menu, started, with the key map `map` and no countdown.
fn started(map: Map) -> Model {
    let options = DemoOptions {
        classic: map == Map::Classic,
        timeout: None,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.into_model()
}

/// The top line of the screen `model` draws at `width`.
fn top(model: &Model, width: u16) -> String {
    let text = screen(model, width, 30);
    text.lines().next().unwrap_or_default().to_owned()
}

/// What the top line shows, as (the name, the host, the time, the long
/// status).
fn shown(model: &Model, width: u16) -> (bool, bool, bool, bool) {
    let top = top(model, width);
    (
        top.contains("ZFSBootMenu"),
        top.contains("host 00bab10c"),
        top.contains("08:00 UTC"),
        top.contains("DEGRADED"),
    )
}

#[test]
fn as_the_tab_bar_narrows_the_name_goes_then_the_host_then_the_time() {
    let mut model = started(Map::Workspace);
    // A long pool name, for the forms with the host and time to differ.
    let tank = model
        .snapshot
        .pools
        .iter_mut()
        .find(|pool| pool.name.as_str() == "tank");
    tank.unwrap().name = "tank-of-twenty-letters".parse().unwrap();

    assert_eq!(shown(&model, 140), (true, true, true, true));
    assert_eq!(shown(&model, 130), (false, true, true, true));
    assert_eq!(shown(&model, 120), (false, false, true, true));
    // Below 120 columns neither shows, and the name has room again.
    assert_eq!(shown(&model, 119), (true, false, false, true));
    assert_eq!(shown(&model, 90), (false, false, false, true));
    assert_eq!(shown(&model, 60), (false, false, false, false));
    // Narrower than the menu draws, the tabs take short names, then keys.
    let bar = |width| -> String {
        let line = tab_bar(&model, width);
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    };
    assert!(
        bar(40).starts_with(" [1 Boot] 2 Snap  3 Pools  4 Log"),
        "{}",
        bar(40)
    );
    assert_eq!(bar(20).trim_end(), " [1] 2  3  4    ! 6");
    assert_eq!(bar(12).trim_end(), " [1] 2  3  4");
}

#[test]
fn the_tab_bar_counts_what_needs_attention_and_the_navigation_bar_the_log() {
    let workspace = started(Map::Workspace);
    let classic = started(Map::Classic);
    let count = crate::menu::view::workspace::attention_count(&workspace);

    let tabs = top(&workspace, 140);
    assert!(
        tabs.ends_with(&format!("! {count} · host 00bab10c · 08:00 UTC")),
        "{tabs}"
    );
    assert!(!tabs.contains("in the log"), "{tabs}");
    assert!(count > 1, "{tabs}");
    let bar = top(&classic, 140);
    assert!(bar.ends_with("! 1 in the log"), "{bar}");
    assert!(!bar.contains("UTC") && !bar.contains("host"), "{bar}");
}

#[test]
fn the_tab_bar_s_time_is_hidden_until_the_machine_says_it() {
    let mut model = started(Map::Workspace);
    model.snapshot.now = 0;
    let tabs = top(&model, 140);
    assert!(!tabs.contains("UTC"), "{tabs}");
    assert!(tabs.contains("host 00bab10c"), "{tabs}");
    model.snapshot.host = None;
    assert!(!top(&model, 140).contains("host"));
}

#[test]
fn the_tab_bar_s_time_advances_with_the_ticks() {
    let model = started(Map::Workspace);
    let (model, _) = update(model, Message::Tick(59_999));
    assert!(top(&model, 140).ends_with("08:00 UTC"));
    let (mut model, _) = update(model, Message::Tick(1));
    assert!(top(&model, 140).ends_with("08:01 UTC"));

    // A snapshot of another time starts the count again.
    let mut later = model.snapshot.clone();
    later.now = NOW + 16 * 3600 - 1;
    model.replace_snapshot(later);
    assert!(top(&model, 140).ends_with("23:59 UTC"));
    let (model, _) = update(model, Message::Tick(1000));
    assert!(top(&model, 140).ends_with("00:00 UTC"));
}
