//! The terminal's events: the answers the programs get, the title they
//! set (OSC 0 and 2), and the bell (BEL).

use alacritty_terminal::event::Event;
use alacritty_terminal::vte::ansi::Rgb as TermRgb;

use super::Console;
use crate::colour::{self, Table};

impl Console {
    /// Turns the terminal's events into the answers the programs get, and
    /// keeps the title and the bell.
    pub(super) fn answer(&mut self) {
        let events: Vec<Event> = self.events.0.borrow_mut().drain(..).collect();
        for event in events {
            let text = match event {
                Event::PtyWrite(text) => text,
                Event::TextAreaSizeRequest(format) => format(self.window_size()),
                Event::ColorRequest(index, format) => {
                    let colours = Table::new(self.term.colors(), self.scheme);
                    let colour = colour::by_index(colours, index);
                    format(TermRgb {
                        r: colour.0,
                        g: colour.1,
                        b: colour.2,
                    })
                }
                Event::Title(title) => {
                    self.title = Some(title);
                    continue;
                }
                Event::ResetTitle => {
                    self.title = None;
                    continue;
                }
                Event::Bell => {
                    self.bell.ring();
                    continue;
                }
                _ => continue,
            };
            self.replies.extend_from_slice(text.as_bytes());
        }
    }

    /// Takes the answers owed to the programs.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// The title the programs set, if they did.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}
