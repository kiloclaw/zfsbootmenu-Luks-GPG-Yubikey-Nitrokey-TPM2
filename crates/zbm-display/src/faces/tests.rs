//! Characters come from their face, the regular one, or a fallback font.

use super::*;
use crate::outline::tests::DEJAVU;

fn dejavu() -> Outline {
    Outline::new(DEJAVU.to_vec(), 20).unwrap()
}

#[test]
fn faces_are_chosen_by_style() {
    let of = |bold, italic| FontFace::of(bold, italic);
    assert_eq!(
        [
            of(false, false),
            of(true, false),
            of(false, true),
            of(true, true)
        ],
        [
            FontFace::Regular,
            FontFace::Bold,
            FontFace::Italic,
            FontFace::BoldItalic
        ]
    );
    // A face the grid lacks falls back to the closest it has.
    let faces = Faces::new(dejavu()).with_face(FontFace::Italic, dejavu());
    assert_eq!(faces.order(FontFace::BoldItalic), [None, Some(1), Some(0)]);
    assert_eq!(faces.order(FontFace::Bold), [None, Some(0), None]);
    assert_eq!(faces.order(FontFace::Italic), [Some(1), Some(0), None]);
    assert_eq!(faces.with_face(FontFace::Regular, dejavu()).count(), 2);
}

#[test]
fn characters_come_from_the_face_the_regular_one_or_a_fallback() {
    let mut faces = Faces::new(dejavu())
        .with_fallback(dejavu())
        .with_face(FontFace::Bold, dejavu());

    // Faces come before the fallback fonts, whatever the order added.
    assert_eq!(faces.find(FontFace::Bold, 'a'), Some(1));
    assert_eq!(faces.find(FontFace::Regular, 'a'), Some(0));
    assert_eq!(faces.count(), 3);
    // A character none has is drawn as the replacement character.
    assert!(faces.glyph(FontFace::Regular, '\u{e000}').is_none());
    let replacement = faces.drawn(FontFace::Regular, '\u{fffd}').cloned();
    assert_eq!(
        faces.drawn(FontFace::Regular, '\u{e000}').cloned(),
        replacement
    );
    assert!(replacement.is_some());
    assert_eq!(faces.regular().cell(), dejavu().cell());
}
