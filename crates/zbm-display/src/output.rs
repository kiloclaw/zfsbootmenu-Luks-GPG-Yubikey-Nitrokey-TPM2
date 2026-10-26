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

/// A connector, by what choosing an output needs to know of it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectorDesc {
    /// Whether a display is connected.
    pub connected: bool,
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

/// The output the kernel console shows, else the first connected one:
/// the first connected connector that has a mode and a CRTC and is scanned
/// out now, else the first that has a mode and a CRTC. A closed laptop's
/// panel is then left dark when the kernel console is on another monitor.
///
/// Its mode is the one shown now, so that the monitor neither flickers nor
/// loses its signal; else its preferred mode, else its largest. Its CRTC is
/// the one that scans it out, else the first it can use.
pub fn choose(connectors: &[ConnectorDesc]) -> Option<Choice> {
    let usable = || {
        connectors
            .iter()
            .enumerate()
            .filter_map(|(index, connector)| choice(index, connector))
    };
    usable()
        .find(|choice| {
            connectors
                .get(choice.connector)
                .is_some_and(|connector| connector.current_crtc.is_some())
        })
        .or_else(|| usable().next())
}

/// The connector at `index`, its mode and CRTC, when it is connected and
/// has both.
fn choice(index: usize, connector: &ConnectorDesc) -> Option<Choice> {
    if !connector.connected {
        return None;
    }
    let mode = connector
        .current_mode
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
    let crtc = connector
        .current_crtc
        .or_else(|| connector.possible_crtcs.first().copied())?;
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
    fn the_first_connected_display_in_its_preferred_mode_on_its_crtc() {
        let unplugged = ConnectorDesc {
            modes: vec![mode(640, 480, true)],
            possible_crtcs: vec![0],
            ..ConnectorDesc::default()
        };
        let display = ConnectorDesc {
            connected: true,
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
            connected: true,
            modes: vec![mode(1024, 768, false), mode(1280, 800, false)],
            current_crtc: None,
            possible_crtcs: vec![2, 0],
            ..ConnectorDesc::default()
        };
        let no_crtc = ConnectorDesc {
            connected: true,
            modes: vec![mode(1024, 768, false)],
            ..ConnectorDesc::default()
        };
        let no_mode = ConnectorDesc {
            connected: true,
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
            connected: true,
            modes: vec![mode(2880, 1800, true)],
            possible_crtcs: vec![0, 1],
            ..ConnectorDesc::default()
        };
        let monitor = ConnectorDesc {
            connected: true,
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
}
