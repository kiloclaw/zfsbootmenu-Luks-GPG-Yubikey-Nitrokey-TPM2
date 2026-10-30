//! Frames at the display's refresh rate at most (C75).

use pretty_assertions::assert_eq;

use super::*;

/// A screen that refreshes once a second, and counts the frames shown.
struct SlowScreen {
    presented: usize,
}

impl Screen for SlowScreen {
    fn size(&self) -> (u32, u32) {
        (40 * 8, 10 * 14)
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn present(&mut self, frame: &mut Frame) -> io::Result<()> {
        frame.take_damage();
        self.presented += 1;
        Ok(())
    }
}

#[test]
fn frames_come_no_faster_than_the_display_refreshes_and_the_last_is_shown() {
    let mut console = console();
    let mut screen = SlowScreen { presented: 0 };
    let script = vec![
        output("one\r\n"),
        Input::Nothing,
        output("two\r\n"),
        Input::Nothing,
        output("three\r\n"),
        Input::Nothing,
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

    // The first frame, the one of `one` at once, then none within the
    // second; the frame that waited is shown as the console ends.
    assert_eq!(screen.presented, 3, "{texts:?}");
    assert!(texts.last().unwrap().contains("three"));
}
