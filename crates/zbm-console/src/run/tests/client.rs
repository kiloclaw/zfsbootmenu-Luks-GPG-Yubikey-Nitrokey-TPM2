//! The client of the channel: its hello answered, its cells shown while it
//! has the screen, the keys its own meanwhile, and the terminal back when it
//! lends the screen or goes.

use pretty_assertions::assert_eq;
use zbm_display::channel::{Caps, Run, VERSION};
use zbm_display::decor::Decoration;
use zbm_display::faces::FontFace;
use zbm_display::frame::Rgb;
use zbm_display::grid::CellStyle;

use super::*;

fn screen() -> FakeScreen {
    FakeScreen {
        presented: 0,
        broken: false,
    }
}

fn client(message: FromClient) -> Input {
    Input::Client(message)
}

fn cells(text: &str) -> Input {
    let style = CellStyle {
        fg: Rgb(1, 2, 3),
        bg: Rgb(4, 5, 6),
        decoration: Decoration::default(),
        face: FontFace::Regular,
    };
    let run = Run {
        row: 0,
        column: 0,
        style,
        cells: text.chars().map(String::from).collect(),
    };
    client(FromClient::Cells(vec![run]))
}

/// The keys `text` types, as the client gets them.
fn keys(text: &str) -> Vec<FromConsole> {
    text.chars()
        .map(|c| FromConsole::Key {
            press: press(Key::Char(c)),
            layout: "de".to_owned(),
        })
        .collect()
}

#[test]
fn the_client_draws_and_gets_the_keys_until_it_lends_the_screen() {
    let mut console = console();
    let mut texts = Vec::new();
    let mut host = FakeHost::new(vec![
        output("$ "),
        client(FromClient::Hello { version: VERSION }),
        client(FromClient::Take),
        cells("Boot"),
        client(FromClient::Cursor(None)),
        chars("ab"),
        client(FromClient::Secret(true)),
        Input::Nothing,
        client(FromClient::SwitchLayout("fr".to_owned())),
        client(FromClient::Announce("Booting".to_owned())),
        client(FromClient::Lend),
        chars("ls"),
        Input::Hangup,
    ]);

    run(
        &mut console,
        &mut screen(),
        &mut host,
        Some(&mut |text: &str| {
            texts.push(text.to_owned());
        }),
    )
    .unwrap();

    let mut expected = vec![
        FromConsole::Hello { version: VERSION },
        FromConsole::Caps(Caps {
            columns: 40,
            rows: 10,
            true_colour: true,
        }),
    ];
    expected.extend(keys("ab"));
    assert_eq!(host.to_client, expected);
    // Only the keys typed after the screen was lent reach the programs.
    assert_eq!(host.sent, b"ls");
    assert_eq!(host.layouts, ["fr"]);
    assert_eq!(
        host.notes,
        [format!("{GREETED} (client channel version {VERSION})")]
    );
    assert!(
        texts.iter().any(|text| text.starts_with("Boot\n")),
        "{texts:?}"
    );
    assert!(texts.last().unwrap().starts_with("$\n"));
    assert!(!console.is_secret());
}

#[test]
fn a_secret_the_client_asks_for_keeps_nothing() {
    let mut console = console().with_scrollback(100);
    let mut host = FakeHost::new(vec![
        client(FromClient::Hello { version: VERSION }),
        client(FromClient::Take),
        client(FromClient::Secret(true)),
        Input::Nothing,
        Input::Release,
    ]);

    run(&mut console, &mut screen(), &mut host, None).unwrap();

    assert!(console.is_secret());
    assert!(console.is_showing_cells());
}

#[test]
fn a_client_of_another_version_or_without_a_hello_is_not_drawn() {
    let mut console = console();
    let mut host = FakeHost::new(vec![
        client(FromClient::Take),
        cells("x"),
        client(FromClient::Hello {
            version: VERSION + 1,
        }),
        client(FromClient::Take),
        chars("ok"),
        Input::Hangup,
    ]);

    run(&mut console, &mut screen(), &mut host, None).unwrap();

    assert_eq!(host.to_client, [FromConsole::Hello { version: VERSION }]);
    assert!(!console.is_showing_cells());
    assert_eq!(host.sent, b"ok");
    assert_eq!(
        host.notes,
        [format!(
            "a menu of client channel version {} draws on the terminal",
            VERSION + 1
        )]
    );
}

#[test]
fn a_client_gone_or_starting_again_gives_the_terminal_back() {
    for last in [
        Input::ClientGone,
        client(FromClient::Hello { version: VERSION }),
    ] {
        let mut console = console();
        let mut host = FakeHost::new(vec![
            client(FromClient::Hello { version: VERSION }),
            client(FromClient::Take),
            client(FromClient::Secret(true)),
            last,
            chars("x"),
            Input::Release,
        ]);

        run(&mut console, &mut screen(), &mut host, None).unwrap();

        assert!(!console.is_showing_cells());
        assert!(!console.is_secret());
        assert_eq!(host.sent, b"x");
    }
}

#[test]
fn the_client_is_told_the_new_size_after_the_font_changed() {
    let mut console = console();
    let mut host = FakeHost::new(vec![
        client(FromClient::Hello { version: VERSION }),
        client(FromClient::Take),
        Input::Nothing,
        Input::Hangup,
    ]);
    host.has_fonts = true;
    let mut zoomed = false;
    let mut host = Zooming {
        host,
        zoom: &mut zoomed,
    };

    run(&mut console, &mut screen(), &mut host, None).unwrap();

    let resized = host
        .host
        .to_client
        .iter()
        .find(|message| matches!(message, FromConsole::Resize { .. }));
    assert_eq!(
        resized,
        Some(&FromConsole::Resize {
            columns: 20,
            rows: 5
        })
    );
}

/// The fake host, which asks for a bigger font once the client said hello.
struct Zooming<'z> {
    host: FakeHost,
    zoom: &'z mut bool,
}

impl Host for Zooming<'_> {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        self.host.wait(timeout)
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.host.send(bytes)
    }

    fn grid(&mut self, pixels: (u32, u32), height: Option<u16>) -> Option<Grid> {
        self.host.grid(pixels, height)
    }

    fn font_request(&mut self) -> Option<Zoom> {
        let is_greeted = !self.host.to_client.is_empty();
        (is_greeted && !std::mem::replace(self.zoom, true)).then_some(Zoom::Bigger)
    }

    fn send_client(&mut self, message: &FromConsole) {
        self.host.send_client(message);
    }
}
