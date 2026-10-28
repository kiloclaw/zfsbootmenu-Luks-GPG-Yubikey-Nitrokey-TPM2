use pretty_assertions::assert_eq;

use super::*;

#[derive(Debug, thiserror::Error)]
#[error("unable to read the key")]
struct Inner(#[source] std::io::Error);

#[derive(Debug, thiserror::Error)]
enum Demo {
    #[error("no kernel to boot")]
    NoKernel,
    #[error("unable to load {name}")]
    Load {
        name: String,
        #[source]
        cause: Inner,
    },
    #[error("cryptsetup failed: {0}")]
    Repeats(#[source] Inner),
    #[error(transparent)]
    Wrapped(Box<Demo>),
}

impl Coded for Box<Demo> {
    fn code(&self) -> ErrorCode {
        self.as_ref().code()
    }
}

crate::error_codes! {
    Demo, DEMO, delegate [Wrapped];
    NoKernel = 604 => "no kernel": "Install one.",
    Load = 602 => "a key could not be loaded": "Check the key.",
    Repeats = 506 => "cryptsetup failed": "Read its output.",
}

fn missing() -> Inner {
    Inner(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no such file",
    ))
}

#[test]
fn codes_read_and_print_as_zbm_e_and_four_digits() {
    for text in ["ZBM-E0412", "zbm-e0412", "E0412", "0412", "412", " 412 "] {
        assert_eq!(ErrorCode::parse(text), Some(ErrorCode::new(412)), "{text}");
    }
    for text in ["", "ZBM-", "E", "ZBM-E12345", "x412", "4-12", "ZBM-W0412"] {
        assert_eq!(ErrorCode::parse(text), None, "{text}");
    }
    assert_eq!(ErrorCode::new(7).to_string(), "ZBM-E0007");
    assert_eq!(ErrorCode::new(412).number(), 412);
}

#[test]
fn each_variant_has_its_code_and_a_wrapped_error_its_own() {
    assert_eq!(Demo::NoKernel.code(), ErrorCode::new(604));
    let load = Demo::Load {
        name: "zroot".to_owned(),
        cause: missing(),
    };
    assert_eq!(load.code(), ErrorCode::new(602));
    assert_eq!(Demo::Wrapped(Box::new(load)).code(), ErrorCode::new(602));
    let codes: Vec<u16> = DEMO.iter().map(|entry| entry.code.number()).collect();
    assert_eq!(codes, [604, 602, 506]);
    assert_eq!(DEMO[1].summary, "a key could not be loaded");
}

#[test]
fn a_report_has_the_code_and_every_cause_once() {
    let load = Demo::Load {
        name: "zroot".to_owned(),
        cause: missing(),
    };
    assert_eq!(
        report(&load),
        "ZBM-E0602: unable to load zroot: unable to read the key: no such file"
    );
    // The message already ends with its cause, which is not repeated.
    let repeats = Demo::Repeats(Inner(std::io::Error::other("permission denied")));
    assert_eq!(
        chain(&repeats),
        "cryptsetup failed: unable to read the key: permission denied"
    );
    assert_eq!(report(&Demo::NoKernel), "ZBM-E0604: no kernel to boot");
}
