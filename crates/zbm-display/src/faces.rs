//! The TrueType faces a grid draws with: the regular one, which sets the
//! cells' size, its bold, italic and bold italic forms when the image has
//! them, and fallback fonts for the characters the faces lack.
//!
//! A character is drawn from the face its cell asks for, else from the
//! regular face, else from the first fallback font that has it, else as
//! the regular face's replacement character, or `?`.

use crate::outline::{Glyph, Outline};

/// The face a cell asks for: bold, italic, both or neither.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FontFace {
    /// Upright and of normal weight.
    #[default]
    Regular,
    /// Bold.
    Bold,
    /// Italic (oblique).
    Italic,
    /// Bold and italic.
    BoldItalic,
}

impl FontFace {
    /// The face of text that is `bold`, `italic`, both or neither.
    pub fn of(bold: bool, italic: bool) -> FontFace {
        match (bold, italic) {
            (false, false) => FontFace::Regular,
            (true, false) => FontFace::Bold,
            (false, true) => FontFace::Italic,
            (true, true) => FontFace::BoldItalic,
        }
    }
}

/// The faces of a grid drawn with TrueType fonts. Outlines are numbered:
/// 0 is the regular face, then come the others, then the fallback fonts.
#[derive(Debug)]
pub struct Faces {
    /// The regular face.
    regular: Outline,
    /// The other faces, then the fallback fonts.
    others: Vec<Outline>,
    /// The numbers of the bold, italic and bold italic faces.
    styled: [Option<usize>; 3],
    /// How many faces there are, the regular one included.
    face_count: usize,
}

impl Faces {
    /// Draws with `regular` alone.
    pub fn new(regular: Outline) -> Faces {
        Faces {
            regular,
            others: Vec::new(),
            styled: [None; 3],
            face_count: 1,
        }
    }

    /// The outline numbered `index`.
    fn outline_mut(&mut self, index: usize) -> Option<&mut Outline> {
        match index.checked_sub(1) {
            None => Some(&mut self.regular),
            Some(other) => self.others.get_mut(other),
        }
    }

    /// Draws `face` with `outline`, the face of that name of the regular
    /// one. Faces are added before fallback fonts.
    #[must_use]
    pub fn with_face(mut self, face: FontFace, outline: Outline) -> Faces {
        let slot = match face {
            FontFace::Regular => return self,
            FontFace::Bold => 0,
            FontFace::Italic => 1,
            FontFace::BoldItalic => 2,
        };
        self.others.insert(self.face_count - 1, outline);
        if let Some(index) = self.styled.get_mut(slot) {
            *index = Some(self.face_count);
        }
        self.face_count += 1;
        self
    }

    /// Draws the characters the faces lack with `outline`, after the
    /// fallback fonts added before it.
    #[must_use]
    pub fn with_fallback(mut self, outline: Outline) -> Faces {
        self.others.push(outline);
        self
    }

    /// The regular face, which sets the cells and the lines.
    pub fn regular(&self) -> &Outline {
        &self.regular
    }

    /// How many faces and fallback fonts there are.
    pub fn count(&self) -> usize {
        1 + self.others.len()
    }

    /// The faces tried for `face`, in order: the styled ones, from the
    /// closest, then the regular one.
    fn order(&self, face: FontFace) -> [Option<usize>; 3] {
        let [bold, italic, both] = self.styled;
        match face {
            FontFace::Regular => [Some(0), None, None],
            FontFace::Bold => [bold, Some(0), None],
            FontFace::Italic => [italic, Some(0), None],
            FontFace::BoldItalic => [both, bold.or(italic), Some(0)],
        }
    }

    /// Which outline draws `c` in `face`, if any has it.
    fn find(&mut self, face: FontFace, c: char) -> Option<usize> {
        let fallbacks = self.face_count..self.count();
        let mut candidates = self.order(face).into_iter().flatten().chain(fallbacks);
        candidates.find(|&index| {
            self.outline_mut(index)
                .is_some_and(|outline| outline.glyph(c).is_some())
        })
    }

    /// The glyph of `c` in `face`, from the face, the regular one or a
    /// fallback font; `None` when none has it.
    pub fn glyph(&mut self, face: FontFace, c: char) -> Option<&Glyph> {
        let index = self.find(face, c)?;
        self.outline_mut(index)?.glyph(c)
    }

    /// The glyph `c` is drawn with in `face`: its own, else the regular
    /// face's replacement character, else its `?`.
    pub fn drawn(&mut self, face: FontFace, c: char) -> Option<&Glyph> {
        let shown = [c, '\u{fffd}', '?']
            .into_iter()
            .find(|&shown| self.find(face, shown).is_some())?;
        self.glyph(face, shown)
    }
}

#[cfg(test)]
mod tests;
