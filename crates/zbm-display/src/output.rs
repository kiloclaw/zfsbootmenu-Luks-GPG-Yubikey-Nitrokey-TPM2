//! Which output to draw on: the connector, its mode and the CRTC that scans
//! it out, chosen from what the DRM device describes, without I/O.

/// A mode of a connector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeDesc {
    /// The width in pixels.
    pub width: u16,
    /// The height in pixels.
    pub height: u16,
    /// Whether the display says it is its own (its native resolution).
    pub preferred: bool,
}

/// Whether a display is on a connector, as the kernel knows it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Link {
    /// No display.
    #[default]
    Disconnected,
    /// A display.
    Connected,
    /// The kernel cannot tell, such as on a VGA output whose monitor does
    /// not answer, or behind some docks: the kernel console uses such a
    /// connector when none is connected, and so does the console.
    Unknown,
}

/// A connector, by what choosing an output needs to know of it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectorDesc {
    /// Whether a display is connected.
    pub link: Link,
    /// Its modes, in the device's order.
    pub modes: Vec<ModeDesc>,
    /// The CRTC it is scanned out by now, as an index into the device's
    /// CRTCs: the kernel console's.
    pub current_crtc: Option<usize>,
    /// The mode that CRTC shows now, as an index into `modes`, when the
    /// connector offers it.
    pub current_mode: Option<usize>,
    /// The CRTCs its encoders can use, by index.
    pub possible_crtcs: Vec<usize>,
}

/// The output chosen: indexes of the connector, of its mode and of the CRTC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Choice {
    /// The connector.
    pub connector: usize,
    /// The mode, among the connector's.
    pub mode: usize,
    /// The CRTC.
    pub crtc: usize,
}

/// The output the kernel console shows, else the first connected one, as
/// the kernel console chooses: the first connector, connected or of
/// unknown state, that has a mode and is scanned out now; else the first
/// connected one that has a mode and a CRTC; else the first of unknown
/// state. A closed laptop's panel is then left dark when the kernel console
/// is on another monitor, and a monitor the kernel cannot see is used when
/// there is no other.
///
/// Its mode is the one shown now, so that the monitor neither flickers nor
/// loses its signal; else its preferred mode, else its largest. Its CRTC is
/// the one that scans it out, else the first it can use that scans out no
/// other connector, so that no monitor goes dark.
pub fn choose(connectors: &[ConnectorDesc]) -> Option<Choice> {
    choose_mode(connectors, None)
}

/// [`choose`], in a mode of `wanted` (width, height) when the connector has
/// one, as `zbm.console_mode` asks (C60): its preferred one of that size,
/// else the first.
pub fn choose_mode(connectors: &[ConnectorDesc], wanted: Option<(u16, u16)>) -> Option<Choice> {
    let busy: Vec<usize> = connectors.iter().filter_map(|c| c.current_crtc).collect();
    let usable = |links: &'static [Link]| {
        let busy = &busy;
        connectors
            .iter()
            .enumerate()
            .filter(move |(_, connector)| links.contains(&connector.link))
            .filter_map(move |(index, connector)| choice(index, connector, busy, wanted))
    };
    usable(&[Link::Connected, Link::Unknown])
        .find(|choice| {
            connectors
                .get(choice.connector)
                .is_some_and(|connector| connector.current_crtc.is_some())
        })
        .or_else(|| usable(&[Link::Connected]).next())
        .or_else(|| usable(&[Link::Unknown]).next())
}

/// The connector at `index`, its mode and CRTC, when it has both; `busy`
/// are the CRTCs that scan out a connector now, which only that connector
/// may take.
fn choice(
    index: usize,
    connector: &ConnectorDesc,
    busy: &[usize],
    wanted: Option<(u16, u16)>,
) -> Option<Choice> {
    let sized = |preferred: bool| {
        let (width, height) = wanted?;
        connector.modes.iter().position(|mode| {
            (mode.width, mode.height) == (width, height) && (mode.preferred || !preferred)
        })
    };
    let mode = sized(true)
        .or_else(|| sized(false))
        .or(connector.current_mode)
        .filter(|&mode| mode < connector.modes.len())
        .or_else(|| connector.modes.iter().position(|mode| mode.preferred))
        .or_else(|| {
            let area = |mode: &ModeDesc| u32::from(mode.width) * u32::from(mode.height);
            let largest = connector.modes.iter().map(area).max()?;
            connector
                .modes
                .iter()
                .position(|mode| area(mode) == largest)
        })?;
    let crtc = connector.current_crtc.or_else(|| {
        connector
            .possible_crtcs
            .iter()
            .copied()
            .find(|crtc| !busy.contains(crtc))
    })?;
    Some(Choice {
        connector: index,
        mode,
        crtc,
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn mode(width: u16, height: u16, preferred: bool) -> ModeDesc {
        ModeDesc {
            width,
            height,
            preferred,
        }
    }

    #[test]
    fn a_mode_of_the_size_asked_for_is_chosen_when_there_is_one() {
        let display = ConnectorDesc {
            link: Link::Connected,
            modes: vec![
                mode(1920, 1080, true),
                mode(1024, 768, false),
                mode(800, 600, false),
            ],
            current_crtc: Some(0),
            current_mode: Some(0),
            possible_crtcs: vec![0],
        };
        let mode_of = |wanted| {
            choose_mode(std::slice::from_ref(&display), wanted)
                .unwrap()
                .mode
        };

        assert_eq!(mode_of(Some((1024, 768))), 1);
        // A size it does not have: the mode shown now.
        assert_eq!(mode_of(Some((640, 480))), 0);
        assert_eq!(mode_of(None), 0);
    }

    #[test]
    fn the_first_connected_display_in_its_preferred_mode_on_its_crtc() {
        let unplugged = ConnectorDesc {
            modes: vec![mode(640, 480, true)],
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };
        let display = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(3840, 2400, false), mode(1920, 1200, true)],
            current_crtc: Some(1),
            possible_crtcs: vec![0, 1],
            ..ConnectorDesc::default()
        };

        assert_eq!(
            choose(&[unplugged, display]),
            Some(Choice {
                connector: 1,
                mode: 1,
                crtc: 1
            })
        );
    }

    #[test]
    fn without_a_preferred_mode_or_current_crtc_the_largest_on_the_first() {
        let display = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(1024, 768, false), mode(1280, 800, false)],
            current_crtc: None,
            possible_crtcs: vec![2, 0],
            ..ConnectorDesc::default()
        };
        let no_crtc = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(1024, 768, false)],
            ..ConnectorDesc::default()
        };
        let no_mode = ConnectorDesc {
            link: Link::Connected,
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };

        assert_eq!(
            choose(std::slice::from_ref(&display)),
            Some(Choice {
                connector: 0,
                mode: 1,
                crtc: 2
            })
        );
        assert_eq!(choose(&[no_crtc, no_mode]), None);
    }
    #[test]
    fn the_output_shown_now_wins_in_the_mode_it_shows() {
        // A closed laptop's panel, connected but dark, and the monitor the
        // kernel console is on, at a mode below its preferred one.
        let panel = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(2880, 1800, true)],
            possible_crtcs: vec![0, 1],
            ..ConnectorDesc::default()
        };
        let monitor = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(3840, 2160, true), mode(1920, 1080, false)],
            current_crtc: Some(1),
            current_mode: Some(1),
            possible_crtcs: vec![0, 1],
        };

        assert_eq!(
            choose(&[panel.clone(), monitor]),
            Some(Choice {
                connector: 1,
                mode: 1,
                crtc: 1
            })
        );
        // With nothing shown, the first; a mode out of range is ignored.
        let odd = ConnectorDesc {
            current_mode: Some(7),
            ..panel.clone()
        };
        assert_eq!(
            choose(&[odd, panel]),
            Some(Choice {
                connector: 0,
                mode: 0,
                crtc: 0
            })
        );
    }

    #[test]
    fn a_monitor_of_unknown_state_is_used_when_none_is_connected() {
        let vga = ConnectorDesc {
            link: Link::Unknown,
            modes: vec![mode(1024, 768, false)],
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };
        let unplugged = ConnectorDesc {
            modes: vec![mode(640, 480, true)],
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };
        let hdmi = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(1920, 1080, true)],
            possible_crtcs: vec![1],
            ..ConnectorDesc::default()
        };
        let first = Choice {
            connector: 0,
            mode: 0,
            crtc: 0,
        };

        assert_eq!(choose(&[vga.clone(), unplugged.clone()]), Some(first));
        // A connected monitor comes first, unless the other one is shown.
        assert_eq!(
            choose(&[vga.clone(), hdmi.clone()]),
            Some(Choice {
                connector: 1,
                mode: 0,
                crtc: 1
            })
        );
        let shown = ConnectorDesc {
            current_crtc: Some(0),
            ..vga
        };
        assert_eq!(choose(&[shown, hdmi]), Some(first));
        assert_eq!(choose(&[unplugged]), None);
    }

    #[test]
    fn the_crtc_of_another_monitor_is_not_taken() {
        // The kernel console on a monitor of unknown state, on CRTC 0; a
        // connected one that could use CRTC 0 or 1 is not chosen over it,
        // and when it is the only choice it gets CRTC 1.
        let shown = ConnectorDesc {
            link: Link::Unknown,
            modes: vec![mode(1024, 768, true)],
            current_crtc: Some(0),
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };
        let other = ConnectorDesc {
            link: Link::Connected,
            modes: vec![mode(1920, 1080, true)],
            possible_crtcs: vec![0, 1],
            ..ConnectorDesc::default()
        };
        let without_mode = ConnectorDesc {
            modes: Vec::new(),
            ..shown.clone()
        };

        assert_eq!(
            choose(&[other.clone(), shown]),
            Some(Choice {
                connector: 1,
                mode: 0,
                crtc: 0
            })
        );
        assert_eq!(
            choose(&[other.clone(), without_mode.clone()]),
            Some(Choice {
                connector: 0,
                mode: 0,
                crtc: 1
            })
        );
        let only_busy = ConnectorDesc {
            possible_crtcs: vec![0],
            ..other
        };
        assert_eq!(choose(&[only_busy, without_mode]), None);
    }
}
