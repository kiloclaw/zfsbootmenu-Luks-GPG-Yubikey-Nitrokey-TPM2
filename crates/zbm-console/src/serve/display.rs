//! The console's seat: the display and the virtual terminal, taken once
//! another program that holds the display lets it go.

use std::io;
use std::path::{Path, PathBuf};

use zbm_display::display::first_card;
use zbm_display::seat::{self, Seat};

use super::{Places, Settings};

/// The seat of the console, taken: the one handed back before, else that
/// of the card of `settings` and the virtual terminal `vt`; said in the log
/// with how it draws. A seat that cannot be taken stays in `seat`.
pub(super) fn held_seat(
    settings: &Settings,
    places: &Places,
    vt: &Path,
    seat: &mut Option<Seat<seat::Released>>,
) -> io::Result<Seat<seat::Held>> {
    let options = zbm_display::display::Options {
        mode: settings.mode,
        single: false,
    };
    let released = match seat.take() {
        Some(released) => released,
        None => Seat::new(card(settings, places)?, Some(vt.to_path_buf())).with_options(options),
    };
    let mut held = take_seat(places, released, HOLD_WAIT).map_err(|(released, error)| {
        *seat = Some(released);
        error
    })?;
    let buffers = if held.screen().is_flipping() {
        "two buffers flipped on vertical blank"
    } else {
        "one buffer"
    };
    let format = held.screen().format();
    places.log(6, &format!("drawing in {format:?} pixels, {buffers}"));
    Ok(held)
}

/// How long the console waits for a display another program holds (C66).
const HOLD_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// How often it tries again meanwhile.
const HOLD_RETRY: std::time::Duration = std::time::Duration::from_millis(250);

/// Takes `seat`; when another program is the card's master, as Plymouth
/// may be until it quits, says which and tries again for up to `wait`.
pub(super) fn take_seat(
    places: &Places,
    seat: Seat<seat::Released>,
    wait: std::time::Duration,
) -> Result<Seat<seat::Held>, (Seat<seat::Released>, io::Error)> {
    let until = std::time::Instant::now() + wait;
    let mut seat = seat;
    let mut said = false;
    loop {
        match seat.take() {
            Ok(held) => return Ok(held),
            Err((back, error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy
                ) && std::time::Instant::now() < until =>
            {
                if !std::mem::replace(&mut said, true) {
                    let holders = zbm_display::display::holders::holders(
                        &places.sys.with_file_name("proc"),
                        back.card(),
                    );
                    let names: Vec<String> = holders.iter().map(ToString::to_string).collect();
                    let who = if names.is_empty() {
                        "another program".to_owned()
                    } else {
                        names.join(", ")
                    };
                    places.log(5, &format!("{who} holds the display; waiting for it"));
                }
                seat = back;
                std::thread::sleep(HOLD_RETRY);
            }
            Err(failed) => return Err(failed),
        }
    }
}

/// The DRM card the console draws on: the one `settings` names, else the
/// first.
fn card(settings: &Settings, places: &Places) -> io::Result<PathBuf> {
    match &settings.card {
        Some(card) => Ok(card.clone()),
        None => first_card(&places.dev)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no DRM card")),
    }
}
