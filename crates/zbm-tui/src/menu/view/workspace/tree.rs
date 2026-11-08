//! The Pools tab of the workspace (ADR 0007, decision 2; ADR 0016, point
//! 9): under the path, the tree of the pools, their vdevs and devices and
//! the LUKS volumes, each row indented by its depth, and the inspector of
//! the row selected; from 120 columns the inspector is to the right of
//! the tree, narrower below it. A device's kernel messages show in a panel
//! below them, in place of the inspector when narrow. While a filter ranks
//! the rows by how well each name matches, the matches show alone, without
//! the header of the volumes of no pool.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zbm_types::Printable;

use super::super::{device_log, lists, main_view};
use crate::menu::format::fit;
use crate::menu::model::{
    Item, Kind, Model, Node, class_heading, disk_name, host_text, pool_mode, vdev_kind,
};
use crate::menu::theme::Role;
use crate::menu::world::{Device, DeviceState, Health, Pool, PoolHost, Volume, VolumeState};
use crate::text::text;

/// From this width the inspector shows to the right of the tree.
const BESIDE_FROM: u16 = 120;

/// The width of the inspector beside the tree.
const INSPECTOR: u16 = 40;

/// The height of the inspector below the tree, and from [`TALL`] rows.
const BELOW: (u16, u16) = (5, 8);

/// From this height the inspector below the tree takes more lines.
const TALL: u16 = 20;

/// The columns a level of the tree is indented by.
const INDENT: usize = 2;

/// The columns a row keeps for its name, or fewer when its name is
/// shorter, before what it says on the right.
const NAME_LEAST: usize = 12;

/// Draws the Pools tab in `area`.
pub(super) fn render(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let path = super::path::height(model);
    let [top, area] = Layout::vertical([Constraint::Length(path), Constraint::Min(1)]).areas(area);
    super::path::render(model, frame, top);
    let beside = area.width >= BESIDE_FROM;
    let below = match (&model.device_log, beside) {
        (Some(_), _) => (area.height / 2).max(device_log::LEAST),
        (None, true) => 0,
        (None, false) if area.height >= TALL => BELOW.1,
        (None, false) => BELOW.0,
    };
    let [upper, lower] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(below)]).areas(area);
    if beside {
        let [tree, inspector] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(INSPECTOR)]).areas(upper);
        render_tree(model, frame, tree);
        render_inspector(model, frame, inspector);
    } else {
        render_tree(model, frame, upper);
    }
    match &model.device_log {
        Some(log) => device_log::render(model, log, frame, lower),
        None if !beside => render_inspector(model, frame, lower),
        None => {}
    }
}

/// The rows of the tree as the view draws them: the matches of the filter,
/// in their order, or every row, the header included; and the position of
/// the selected one among them.
fn shown<'m>(model: &'m Model, nodes: Vec<Node<'m>>) -> (Vec<Node<'m>>, Option<usize>) {
    let list = &model.pools;
    if !list.filter.is_empty() {
        let mut nodes: Vec<Option<Node<'m>>> = nodes.into_iter().map(Some).collect();
        let matched = (list.matches.iter())
            .filter_map(|ranked| nodes.get_mut(ranked.index).and_then(Option::take))
            .collect();
        return (matched, Some(list.selected));
    }
    let selected = list.selected_item();
    (nodes, selected)
}

/// The tree, its rows indented by depth, the selection's in view.
fn render_tree(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let width = lists::row_width(area);
    let nodes = model.tree();
    let stops = nodes.iter().filter(|node| node.is_stop()).count();
    let (shown, selected) = shown(model, nodes);
    let positions = |at: usize| {
        let ranked = model.pools.matches.get(at)?;
        Some(ranked.positions.as_slice())
    };
    let filtered = !model.pools.filter.is_empty();
    let rows = shown
        .iter()
        .enumerate()
        .map(|(at, node)| {
            let positions = positions(at).filter(|_| filtered).unwrap_or_default();
            row(model, node, positions, width)
        })
        .collect();
    let (title, hint) = (text!("pools-none"), text!("pools-none.text"));
    let empty = (title.as_str(), hint.as_str());
    let header = text!("pools-title");
    let total = (header.as_str(), stops);
    lists::render_rows(
        model,
        frame,
        area,
        total,
        &model.pools,
        (selected, rows),
        empty,
    );
}

/// The row of `node`, `width` columns wide: indented by its depth, its
/// name, with the characters the filter matched at `positions`, then what
/// it says of its state on the right. A heading or a header is its name
/// alone.
fn row(model: &Model, node: &Node<'_>, positions: &[usize], width: usize) -> Line<'static> {
    let theme = model.theme;
    let indent = node.depth * INDENT;
    let width = width.saturating_sub(indent);
    let mut line = if node.is_stop() {
        let least = node.name.chars().count().min(NAME_LEAST);
        let facts = row_facts(model, node, width.saturating_sub(least + 2));
        let used = facts.iter().map(Span::width).sum::<usize>() + 2;
        let name = fit(&node.name, width.saturating_sub(used), theme.is_ascii());
        lists::row(theme, &name, positions, width, facts)
    } else {
        let name = fit(&node.name, width, theme.is_ascii());
        Line::styled(name, theme.style(Role::Header))
    };
    line.spans.insert(0, Span::raw(" ".repeat(indent)));
    line
}

/// What the row of `node` says on the right in at most `room` columns,
/// apart by the theme's dot. Its last fact, the state, shows whole; the
/// others, as a device's note, which may be long, are cut to what is left,
/// and left out when nothing is.
fn row_facts(model: &Model, node: &Node<'_>, room: usize) -> Vec<Span<'static>> {
    let theme = model.theme;
    let dot = theme.dot().chars().count();
    let mut facts = facts(node);
    let Some((state, role)) = facts.pop() else {
        return Vec::new();
    };
    let mut left = room.saturating_sub(state.chars().count());
    let mut spans = Vec::new();
    for (fact, role) in facts {
        let fact = fit(&fact, left.saturating_sub(dot), theme.is_ascii());
        if fact.chars().count() < 2 {
            break;
        }
        left = left.saturating_sub(fact.chars().count() + dot);
        let said = format!("{fact}{}", theme.dot());
        spans.push(Span::styled(said, theme.style(role)));
    }
    spans.push(Span::styled(state, theme.style(role)));
    spans
}

/// What the row of `node` says on the right, each with its role: a pool's
/// last host when another, its mode and its health; a device's note and
/// state; that a volume is one, whether its TPM2 key is out of date, and
/// whether it is open.
fn facts(node: &Node<'_>) -> Vec<(String, Role)> {
    match node.item {
        Item::Pool(pool) => {
            let mut facts = Vec::new();
            if let Some(host) = &pool.host {
                let host = match &host.hostid {
                    Some(hostid) => text!("pool-host", hostid = hostid.to_string()),
                    None => host_text(host),
                };
                facts.push((host, Role::Warning));
            }
            facts.push((pool_mode(pool), Role::Muted));
            let role = health_role(pool.health == Health::Online);
            facts.push((pool.health.as_str().to_owned(), role));
            facts
        }
        Item::Device(_, device) => {
            let remark =
                (device.note.as_ref()).map(|remark| (remark.printable().to_string(), Role::Muted));
            remark
                .into_iter()
                .chain([state_word(device.state)])
                .collect()
        }
        Item::Volume(volume) => {
            let changed = volume.tpm2.as_ref().is_some_and(|seal| !seal.is_current());
            let changed = changed.then(|| (text!("volume-tpm2-changed"), Role::Warning));
            [(text!("volume-label"), Role::Muted)]
                .into_iter()
                .chain(changed)
                .chain([volume_state(volume)])
                .collect()
        }
        Item::Header => Vec::new(),
    }
}

/// The tree in words, for the line view: each row indented by its depth,
/// then what it says on the right; after it, the kernel's messages about
/// a device under their title. Then the rows that are no stop, which the
/// line view does not number: the headings of the classes of devices, the
/// header of the volumes of no pool, and the messages.
pub(super) fn words(model: &Model) -> (Vec<String>, Vec<usize>) {
    let (shown, _) = shown(model, model.tree());
    let mut rows = Vec::new();
    let mut headers = Vec::new();
    for node in &shown {
        if !node.is_stop() {
            headers.push(rows.len());
        }
        let mut said = vec![format!("{}{}", " ".repeat(node.depth * INDENT), node.name)];
        said.extend(facts(node).into_iter().map(|(fact, _)| fact));
        rows.push(said.join(", "));
    }
    if let Some(log) = &model.device_log {
        let lines = device_log::said(log).into_iter().map(|said| match said {
            device_log::Said::Note(text) | device_log::Said::Line(text) => text,
        });
        for line in std::iter::once(device_log::title(log)).chain(lines) {
            headers.push(rows.len());
            rows.push(line);
        }
    }
    (rows, headers)
}

/// The role of a health word: good when `fine`.
fn health_role(fine: bool) -> Role {
    if fine { Role::Good } else { Role::Warning }
}

/// The word of the state of a device, and its role: missing devices are a
/// danger, others not online a warning; none when `zpool` gives none.
fn state_word(state: Option<DeviceState>) -> (String, Role) {
    match state {
        Some(state) if state.is_missing() => (state.as_str().to_owned(), Role::Danger),
        Some(state) => (
            state.as_str().to_owned(),
            health_role(state == DeviceState::Online),
        ),
        None => (text!("value-none"), Role::Muted),
    }
}

/// Whether `volume` is open, as a word, and its role.
fn volume_state(volume: &Volume) -> (String, Role) {
    match &volume.state {
        VolumeState::Open => (text!("volume-open"), Role::Good),
        VolumeState::Locked => (text!("volume-locked"), Role::Locked),
        VolumeState::Skipped => (text!("volume-skipped"), Role::Muted),
        VolumeState::Failed(reason) => (
            text!("volume-failed", reason = Printable(reason).to_string()),
            Role::Danger,
        ),
    }
}

/// The inspector of the row selected: its name as the title, its facts,
/// then its next steps when they fit.
fn render_inspector(model: &Model, frame: &mut Frame<'_>, area: Rect) {
    let Some(node) = model.selected_node() else {
        main_view::render_nothing(model, frame, area);
        return;
    };
    let facts = match node.item {
        Item::Pool(pool) => pool_facts(pool),
        Item::Device(pool, device) => device_facts(pool, device, node.kind),
        Item::Volume(volume) => volume_facts(volume),
        Item::Header => Vec::new(),
    };
    let block = main_view::inspector(model, &node.name, None);
    let width = usize::from(area.width.saturating_sub(4));
    let mut lines = main_view::fact_lines(model, facts, width);
    let steps = super::next_steps(model, width);
    let room = usize::from(area.height.saturating_sub(2));
    if !steps.is_empty() && lines.len() + steps.len() < room {
        lines.push(Line::default());
        lines.extend(steps);
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// A fact whose value is `value`.
fn fact(label: String, value: String) -> main_view::Fact {
    (label, vec![Span::raw(value)])
}

/// What the inspector says of `pool`: its health and mode, the host that
/// used it last when another did, what `zpool` says is wrong and what to
/// do about it, and its data errors when it counts them.
fn pool_facts(pool: &Pool) -> Vec<main_view::Fact> {
    let health = format!("{}, {}", pool.health.as_str(), pool_mode(pool));
    let mut facts = vec![fact(text!("fact-health"), health)];
    if let Some(host) = &pool.host {
        facts.push(fact(text!("fact-host"), last_host(host)));
    }
    if let Some(status) = &pool.status {
        facts.push(fact(text!("fact-status"), status.printable().to_string()));
    }
    if let Some(action) = &pool.action {
        facts.push(fact(text!("fact-action"), action.printable().to_string()));
    }
    if let Some(errors) = pool.errors {
        facts.push(fact(text!("fact-data-errors"), errors.to_string()));
    }
    facts
}

/// The host that used a pool last, under the label that says so: its
/// name and host ID, as much as is known of them.
fn last_host(host: &PoolHost) -> String {
    let hostid = host.hostid.as_ref().map(ToString::to_string);
    let hostname = (host.hostname.as_ref()).map(|name| name.printable().to_string());
    match (hostname, hostid) {
        (Some(hostname), Some(hostid)) => {
            text!("fact-host.named", hostname = hostname, hostid = hostid)
        }
        (Some(known), None) | (None, Some(known)) => known,
        (None, None) => text!("fact-host.unknown"),
    }
}

/// What the inspector says of `device` of `pool`, of `kind`: a vdev's
/// kind and how many devices it holds, a device's path; the class of
/// what it holds when not the data's; then its state, what `zpool` says
/// after it, and its error counts when it has them.
fn device_facts(pool: &Pool, device: &Device, kind: Kind) -> Vec<main_view::Fact> {
    let mut facts = if kind == Kind::Vdev {
        vec![
            fact(text!("fact-kind"), vdev_kind(device)),
            fact(text!("fact-devices"), device.children.len().to_string()),
        ]
    } else {
        let path = Printable(device.path()).to_string();
        vec![fact(text!("fact-path"), path)]
    };
    if let Some(heading) = class_heading(device.class) {
        facts.push(fact(text!("fact-class"), heading));
    }
    facts.push(fact(text!("fact-health"), state_word(device.state).0));
    if let Some(note) = &device.note {
        facts.push(fact(text!("fact-note"), note.printable().to_string()));
    }
    if let Some(errors) = device.errors {
        let counts = text!(
            "fact-errors.counts",
            read = errors.read,
            write = errors.write,
            cksum = errors.cksum
        );
        facts.push(fact(text!("fact-errors"), counts));
    }
    facts.push(fact(text!("fact-pool"), pool.name.to_string()));
    facts
}

/// What the inspector says of `volume`: whether it is open, its device and
/// the pool it holds a device of.
fn volume_facts(volume: &Volume) -> Vec<main_view::Fact> {
    let mut facts = vec![
        fact(text!("fact-state"), volume_state(volume).0),
        fact(text!("fact-device"), disk_name(volume.device.get())),
    ];
    if let Some(pool) = &volume.pool {
        facts.push(fact(text!("fact-pool"), pool.to_string()));
    }
    facts
}

#[cfg(test)]
mod tests;
