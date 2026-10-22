//! A backend that never asks the terminal anything.
//!
//! Ratatui asks the terminal where its cursor is (the escape sequence
//! `ESC [ 6 n`) and how large it is. A serial console often has nobody on the
//! other end who answers: the question times out and the menu fails to start.
//! [`QuietBackend`] answers both questions itself: the cursor is where it was
//! last put, and the size is the one it was given.

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

/// A [`Backend`] that draws through another backend but answers questions
/// about the cursor and the size from what it knows.
#[derive(Debug)]
pub(crate) struct QuietBackend<B> {
    /// The backend that draws.
    inner: B,
    /// Where the cursor was last put.
    cursor: Position,
    /// The size of the screen.
    size: Size,
}

impl<B> QuietBackend<B> {
    /// `inner`, for a screen of `size`.
    pub(crate) fn new(inner: B, size: Size) -> QuietBackend<B> {
        QuietBackend {
            inner,
            cursor: Position::ORIGIN,
            size,
        }
    }

    /// Records a new size, after the terminal reported a resize.
    pub(crate) fn set_size(&mut self, size: Size) {
        self.size = size;
    }
}

impl<B: Backend> Backend for QuietBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.inner.set_cursor_position(position)?;
        self.cursor = position;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::ZERO,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn the_cursor_is_where_it_was_last_put() {
        let mut backend = QuietBackend::new(TestBackend::new(10, 4), Size::new(10, 4));

        backend.set_cursor_position(Position::new(3, 2)).unwrap();

        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(3, 2));
    }

    #[test]
    fn the_size_is_the_one_given_whatever_the_terminal_says() {
        let mut backend = QuietBackend::new(TestBackend::new(10, 4), Size::new(80, 24));
        assert_eq!(backend.size().unwrap(), Size::new(80, 24));

        backend.set_size(Size::new(100, 30));

        assert_eq!(
            backend.window_size().unwrap().columns_rows,
            Size::new(100, 30)
        );
    }
}
