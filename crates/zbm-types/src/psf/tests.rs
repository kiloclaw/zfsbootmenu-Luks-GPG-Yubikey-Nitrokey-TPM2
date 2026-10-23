use pretty_assertions::assert_eq;

use super::*;

/// The Terminus fonts of the boot image, with their cells.
const TERMINUS: [(&str, &[u8], (u32, u32)); 6] = [
    (
        "ter-v12n",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v12n.psf"),
        (6, 12),
    ),
    (
        "ter-v14b",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf"),
        (8, 14),
    ),
    (
        "ter-v20b",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v20b.psf"),
        (10, 20),
    ),
    (
        "ter-v24b",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v24b.psf"),
        (12, 24),
    ),
    (
        "ter-v28b",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v28b.psf"),
        (14, 28),
    ),
    (
        "ter-v32b",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v32b.psf"),
        (16, 32),
    ),
];

/// The Spleen fonts of images built with `zfsbootmenu_console_font=spleen`.
const SPLEEN: [(&str, &[u8], (u32, u32)); 4] = [
    (
        "spleen-8x16",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/spleen/spleen-8x16.psf"),
        (8, 16),
    ),
    (
        "spleen-12x24",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/spleen/spleen-12x24.psf"),
        (12, 24),
    ),
    (
        "spleen-16x32",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/spleen/spleen-16x32.psf"),
        (16, 32),
    ),
    (
        "spleen-32x64",
        include_bytes!("../../../../legacy/zfsbootmenu/fonts/spleen/spleen-32x64.psf"),
        (32, 64),
    ),
];

/// Every font of the image, of both families.
fn every_font() -> impl Iterator<Item = (&'static str, &'static [u8], (u32, u32))> {
    TERMINUS.into_iter().chain(SPLEEN)
}

/// A PSF 2 font of `chars`, one 8×2 glyph each whose rows are the
/// character's code, with or without a table.
fn psf2(chars: &[char], table: bool) -> Vec<u8> {
    let count = u32::try_from(chars.len()).unwrap();
    let mut bytes = PSF2.to_vec();
    for field in [0, 32, u32::from(table), count, 2, 2, 8] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    for &c in chars {
        let code = u8::try_from(u32::from(c) & 0xff).unwrap();
        bytes.extend_from_slice(&[code, code]);
    }
    if table {
        for &c in chars {
            let mut utf8 = [0; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
            bytes.push(0xff);
        }
    }
    bytes
}

#[test]
fn every_font_of_the_image_parses_with_its_table() {
    for (name, bytes, cell) in every_font() {
        let font = Font::parse(bytes).unwrap();

        assert_eq!(font.cell(), cell, "{name}");
        assert_eq!(font.glyph_count(), 512, "{name}");
        assert!(font.has('A') && font.has('─') && font.has('ü'), "{name}");
    }
}

#[test]
fn a_subset_has_256_glyphs_and_the_whole_repertoire() {
    for (name, bytes, cell) in every_font() {
        let font = Font::parse(bytes).unwrap();

        let subset = font.subset(&repertoire());

        assert_eq!(subset.missing, Vec::<char>::new(), "{name}");
        let back = Font::parse(&subset.font.to_bytes()).unwrap();
        assert_eq!(back, subset.font, "{name}");
        assert_eq!(back.glyph_count(), SUBSET_GLYPHS, "{name}");
        assert_eq!(back.cell(), cell, "{name}");
        for c in repertoire() {
            let kept = back.glyph_of(c).and_then(|g| back.glyphs.get(g));
            let source = font.glyph_or_look_alike(c).and_then(|g| font.glyphs.get(g));
            assert_eq!(kept, source, "{name}: {c}");
        }
    }
}

#[test]
fn ascii_keeps_its_codes_and_lines_their_codes_of_code_page_437() {
    let font = Font::parse(TERMINUS[5].1).unwrap();

    let subset = font.subset(&repertoire()).font;

    assert_eq!(subset.glyph_of('A'), Some(0x41));
    assert_eq!(subset.glyph_of(' '), Some(0x20));
    assert_eq!(subset.glyph_of('~'), Some(0x7e));
    assert_eq!(subset.glyph_of('─'), Some(0xc4));
    assert_eq!(subset.glyph_of('┼'), Some(0xc5));
    assert_eq!(subset.glyph_of('│'), Some(0xb3));
}

#[test]
fn look_alikes_stand_in_for_what_a_font_lacks() {
    let ascii: Vec<char> = (0x20_u8..0x7f).map(char::from).collect();
    let font = Font::parse(&psf2(&ascii, true)).unwrap();

    let subset = font.subset(&repertoire());

    let glyph = |c| subset.font.glyph_of(c);
    assert_eq!(glyph('▶'), glyph('>'));
    assert_eq!(glyph('“'), glyph('"'));
    assert_eq!(glyph('…'), glyph('.'));
    assert_eq!(glyph('\u{fffd}'), glyph('?'));
    assert!(glyph('▶').is_some());
    assert!(subset.missing.contains(&'ñ') && subset.missing.contains(&'─'));
    assert!(!subset.missing.contains(&'▶'));
}

#[test]
fn every_character_the_source_draws_with_a_kept_glyph_is_kept() {
    let mut bytes = psf2(&['a', 'b'], true);
    // Glyph 0 also draws U+0430, the Cyrillic a, as Terminus does.
    let table = bytes.len() - 4;
    bytes.splice(table..table, "а".bytes());
    let font = Font::parse(&bytes).unwrap();

    let subset = font.subset(&['a']).font;

    assert_eq!(subset.glyph_of('а'), subset.glyph_of('a'));
    assert!(!subset.has('b'));
}

#[test]
fn invalid_utf8_in_a_table_is_left_out() {
    let mut bytes = psf2(&['a', 'b'], true);
    let table = bytes.len() - 4;
    bytes.splice(table..table, [0xc3, b'z']);

    let font = Font::parse(&bytes).unwrap();

    assert_eq!(font.glyph_of('a'), Some(0));
    assert_eq!(font.glyph_of('z'), Some(0));
    assert!(!font.has(char::REPLACEMENT_CHARACTER));
}

#[test]
fn a_repertoire_beyond_256_glyphs_reports_what_did_not_fit() {
    let chars: Vec<char> = (0x100_u32..0x240).filter_map(char::from_u32).collect();
    let font = Font::parse(&psf2(&chars, true)).unwrap();

    let subset = font.subset(&chars);

    assert_eq!(subset.font.glyph_count(), SUBSET_GLYPHS);
    // 0x20 to 0x7e stay free for ASCII.
    assert_eq!(subset.missing.len(), chars.len() - (SUBSET_GLYPHS - 95));
}

#[test]
fn fonts_without_a_table_draw_their_code_page() {
    let chars: Vec<char> = (0_u8..=255).map(char::from).collect();
    let psf2_font = Font::parse(&psf2(&chars, false)).unwrap();
    let mut psf1 = vec![0x36, 0x04, 0x00, 2];
    psf1.extend((0_u8..=255).flat_map(|code| [code, code]));
    let psf1_font = Font::parse(&psf1).unwrap();

    for font in [psf2_font, psf1_font] {
        assert_eq!(font.glyph_of('A'), Some(0x41));
        assert_eq!(font.glyph_count(), 256);
        assert_eq!(font.glyphs.get(0x41), Some(&vec![0x41, 0x41]));
    }
}

#[test]
fn psf1_tables_and_their_sequences_are_read() {
    // 512 glyphs of height 1, a table: glyph 0 draws 'x' and 'y', then a
    // sequence; the others draw nothing.
    let mut bytes = vec![0x36, 0x04, 0x03, 1];
    bytes.extend(std::iter::repeat_n(0, 512));
    for point in [0x78_u16, 0x79, 0xfffe, 0x41, 0x301, 0xffff] {
        bytes.extend_from_slice(&point.to_le_bytes());
    }
    for _ in 1..512 {
        bytes.extend_from_slice(&0xffff_u16.to_le_bytes());
    }

    let font = Font::parse(&bytes).unwrap();

    assert_eq!(font.glyph_count(), 512);
    assert_eq!(font.cell(), (8, 1));
    assert_eq!(font.glyph_of('y'), Some(0));
    assert!(!font.has('A'));
}

#[test]
fn what_is_not_a_whole_font_is_an_error() {
    let good = psf2(&['a', 'b'], true);
    let header = |at: usize, value: u32| {
        let mut bytes = good.clone();
        bytes.splice(at..at + 4, value.to_le_bytes());
        bytes
    };

    assert_eq!(Font::parse(b"not a font"), Err(PsfError::NotPsf));
    assert_eq!(Font::parse(&good[..20]), Err(PsfError::Truncated));
    assert_eq!(Font::parse(&good[..34]), Err(PsfError::Truncated));
    assert_eq!(Font::parse(&header(28, 0)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(24, 200)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(20, 3)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(16, 0)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(16, u32::MAX)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(8, 4)), Err(PsfError::BadHeader));
    assert_eq!(Font::parse(&header(8, u32::MAX)), Err(PsfError::Truncated));
    assert_eq!(Font::parse(&[0x36, 0x04, 0x00]), Err(PsfError::Truncated));
    assert_eq!(
        Font::parse(&[0x36, 0x04, 0x00, 0]),
        Err(PsfError::BadHeader)
    );
    assert_eq!(
        Font::parse(&[0x36, 0x04, 0x00, 1]),
        Err(PsfError::Truncated)
    );
    assert_eq!(PsfError::NotPsf.to_string(), "not a PSF font");
}

#[test]
fn the_repertoire_fits_in_a_subset_and_holds_the_letters_of_the_four_languages() {
    let repertoire = repertoire();

    assert!(repertoire.len() <= SUBSET_GLYPHS, "{}", repertoire.len());
    for c in "ñ¿¡áéíóúüçàâêîôûëïœŒ«»ãõÿŸ€".chars() {
        assert!(repertoire.contains(&c), "{c}");
    }
    let mut unique = repertoire.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), repertoire.len());
}
