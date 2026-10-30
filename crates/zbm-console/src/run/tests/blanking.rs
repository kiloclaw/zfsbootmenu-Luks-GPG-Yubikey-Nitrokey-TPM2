//! Blanking (C64): the screen black and the display off after a while
//! without a key, and a key that turns it on and shows it again.

use pretty_assertions::assert_eq;

use super::*;

/// A screen that records the frames shown and its power.
struct PoweredScreen {
    presented: usize,
    power: Vec<bool>,
}

impl Screen for PoweredScreen {
    fn size(&self) -> (u32, u32) {
        (40 * 8, 10 * 14)
    }

    fn present(&mut self, frame: &mut Frame) -> io::Result<()> {
        frame.take_damage();
        self.presented += 1;
        Ok(())
    }

    fn set_power(&mut self, on: bool) {
        self.power.push(on);
    }
}

#[test]
fn the_screen_blanks_without_keys_and_a_key_shows_it_again() {
    let mut console = console().with_blank(Some(Duration::from_millis(20)));
    let mut screen = PoweredScreen {
        presented: 0,
        power: Vec::new(),
    };
    // Time passes, more than the blanking's; output comes, unseen; a key.
    let script = vec![
        output("before\r\n"),
        Input::Nothing,
        Input::Nothing,
        Input::Nothing,
        output("while blank\r\n"),
        Input::Nothing,
        chars("x"),
        Input::Hangup,
    ];
    let mut host = FakeHost::new(script);
    let mut texts = Vec::new();

    run(
        &mut console,
        &mut screen,
        &mut host,
        Some(&mut |text: &str| texts.push(text.to_owned())),
    )
    .unwrap();

    assert_eq!(screen.power, [false, true]);
    // The key reached the programs, and the screen came back whole.
    assert_eq!(host.sent, b"x");
    assert!(texts.last().unwrap().contains("while blank"), "{texts:?}");
    assert!(console.blank_after().is_some());
}

#[test]
fn without_blanking_the_power_stays_on() {
    let mut console = console();
    let mut screen = PoweredScreen {
        presented: 0,
        power: Vec::new(),
    };
    let mut host = FakeHost::new(vec![Input::Nothing, chars("x"), Input::Hangup]);

    run(&mut console, &mut screen, &mut host, None).unwrap();

    assert!(screen.power.is_empty());
    assert!(screen.presented > 0);
}
