//! The physical size of a display, from its EDID (VESA E-EDID 1.3 and 1.4,
//! the base block only).
//!
//! The bytes come from the display, through `/sys/class/drm/*/edid`, and are
//! not trusted: every field is checked, and a block that does not add up is
//! no answer at all.

/// The length of the EDID base block.
const BLOCK: usize = 128;

/// The fixed header of every EDID.
const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

/// Where the first detailed timing descriptor starts.
const TIMING: usize = 54;

/// What the EDID says about the display's size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Edid {
    /// The width of the picture, in millimetres.
    pub width_mm: u16,
    /// The height of the picture, in millimetres.
    pub height_mm: u16,
    /// The preferred mode, in pixels, when the EDID has one.
    pub mode: Option<(u16, u16)>,
}

/// The size in `bytes`, an EDID; `None` when they are not one, or give no
/// size (projectors, and displays that report only their aspect ratio).
pub fn parse(bytes: &[u8]) -> Option<Edid> {
    let block = bytes.get(..BLOCK)?;
    if block.get(..HEADER.len())? != HEADER {
        return None;
    }
    let sum = block.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
    if sum != 0 {
        return None;
    }
    let timing = block.get(TIMING..TIMING + 18)?;
    let Timing { mode, size } = detailed_timing(timing);
    let basic = (
        u16::from(*block.get(21)?) * 10,
        u16::from(*block.get(22)?) * 10,
    );
    let (width_mm, height_mm) = size.unwrap_or(basic);
    if width_mm == 0 || height_mm == 0 {
        return None;
    }
    Some(Edid {
        width_mm,
        height_mm,
        mode,
    })
}

/// What a detailed timing descriptor gives, each part when it is not zero.
struct Timing {
    /// The mode, in pixels.
    mode: Option<(u16, u16)>,
    /// The size of the picture, in millimetres.
    size: Option<(u16, u16)>,
}

/// The mode and size of a detailed timing descriptor; nothing when it is a
/// display descriptor instead, which a pixel clock of zero marks.
fn detailed_timing(timing: &[u8]) -> Timing {
    let byte = |index: usize| u16::from(timing.get(index).copied().unwrap_or(0));
    if byte(0) == 0 && byte(1) == 0 {
        return Timing {
            mode: None,
            size: None,
        };
    }
    let width = byte(2) | (byte(4) >> 4) << 8;
    let height = byte(5) | (byte(7) >> 4) << 8;
    let width_mm = byte(12) | (byte(14) >> 4) << 8;
    let height_mm = byte(13) | (byte(14) & 0x0f) << 8;
    Timing {
        mode: (width > 0 && height > 0).then_some((width, height)),
        size: (width_mm > 0 && height_mm > 0).then_some((width_mm, height_mm)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// An EDID of a `width`×`height` mode on a picture of `mm`, with the basic
    /// size in centimetres `cm`, its checksum right.
    pub(crate) fn edid(width: u16, height: u16, mm: (u16, u16), cm: (u8, u8)) -> Vec<u8> {
        let mut bytes = vec![0_u8; BLOCK];
        bytes[..8].copy_from_slice(&HEADER);
        bytes[21] = cm.0;
        bytes[22] = cm.1;
        let [clock_low, clock_high] = 59_400_u16.to_le_bytes();
        bytes[54] = clock_low;
        bytes[55] = clock_high;
        let [width_low, width_high] = width.to_le_bytes();
        let [height_low, height_high] = height.to_le_bytes();
        bytes[56] = width_low;
        bytes[58] = width_high << 4;
        bytes[59] = height_low;
        bytes[61] = height_high << 4;
        let [width_mm_low, width_mm_high] = mm.0.to_le_bytes();
        let [height_mm_low, height_mm_high] = mm.1.to_le_bytes();
        bytes[66] = width_mm_low;
        bytes[67] = height_mm_low;
        bytes[68] = width_mm_high << 4 | height_mm_high;
        let sum = bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        bytes[127] = 0_u8.wrapping_sub(sum);
        bytes
    }

    #[test]
    fn the_size_and_mode_come_from_the_detailed_timing() {
        let laptop = edid(3840, 2400, (302, 189), (30, 19));

        assert_eq!(
            parse(&laptop),
            Some(Edid {
                width_mm: 302,
                height_mm: 189,
                mode: Some((3840, 2400)),
            })
        );
        let large = edid(7680, 4320, (1210, 680), (121, 68));
        assert_eq!(
            parse(&large).map(|e| (e.width_mm, e.height_mm)),
            Some((1210, 680))
        );
    }

    #[test]
    fn without_a_size_in_the_timing_the_basic_size_counts() {
        let bytes = edid(1920, 1080, (0, 0), (53, 30));

        let parsed = parse(&bytes).unwrap();

        assert_eq!((parsed.width_mm, parsed.height_mm), (530, 300));
        assert_eq!(parsed.mode, Some((1920, 1080)));
    }

    #[test]
    fn a_display_descriptor_first_gives_no_mode() {
        let mut bytes = edid(1920, 1080, (0, 0), (53, 30));
        bytes[54] = 0;
        bytes[55] = 0;
        bytes[127] = bytes[127].wrapping_add(0x08).wrapping_add(0xe8);

        assert_eq!(
            parse(&bytes),
            Some(Edid {
                width_mm: 530,
                height_mm: 300,
                mode: None,
            })
        );
    }

    #[test]
    fn broken_or_sizeless_edids_give_nothing() {
        let good = edid(1920, 1080, (531, 299), (53, 30));
        let mut bad_sum = good.clone();
        bad_sum[100] ^= 1;
        let mut bad_header = good.clone();
        bad_header[0] = 1;
        let projector = edid(1920, 1080, (0, 0), (0, 0));
        let aspect_only = edid(1920, 1080, (0, 0), (0, 79));

        assert_eq!(parse(&good[..127]), None);
        assert_eq!(parse(&bad_sum), None);
        assert_eq!(parse(&bad_header), None);
        assert_eq!(parse(&projector), None);
        assert_eq!(parse(&aspect_only), None);
        assert_eq!(parse(&[]), None);
    }
}
