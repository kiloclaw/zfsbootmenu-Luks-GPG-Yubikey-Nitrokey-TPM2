use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::*;

/// A record of a boot of `zroot/ROOT/void`.
fn record() -> BootRecord {
    BootRecord {
        time: 1_793_246_955,
        environment: Untrusted::new("zroot/ROOT/void".to_owned()),
        kernel: Untrusted::new("/boot/vmlinuz-6.12.9_1".to_owned()),
        initramfs: Untrusted::new("/boot/initramfs-6.12.9_1.img".to_owned()),
        command_line: Untrusted::new("root=zfs:zroot/ROOT/void quiet".to_owned()),
        devicetree: Some(Untrusted::new("/boot/dtbs/board.dtb".to_owned())),
        loaded_by: Untrusted::new("kexec".to_owned()),
        measured: true,
        command_line_edited: true,
    }
}

#[test]
fn a_record_is_one_line_of_json_with_its_schema_first() {
    let text = record().to_json().unwrap();

    assert_eq!(
        text,
        concat!(
            r#"{"schema":1,"time":1793246955,"environment":"zroot/ROOT/void","#,
            r#""kernel":"/boot/vmlinuz-6.12.9_1","initramfs":"/boot/initramfs-6.12.9_1.img","#,
            r#""command_line":"root=zfs:zroot/ROOT/void quiet","#,
            r#""devicetree":"/boot/dtbs/board.dtb","loaded_by":"kexec","measured":true,"#,
            r#""command_line_edited":true}"#
        )
    );
    assert_eq!(BootRecord::parse(text.as_bytes()), Ok(record()));
}

#[test]
fn control_characters_are_escaped_in_the_file_and_kept_untrusted() {
    let hostile = BootRecord {
        command_line: Untrusted::new("quiet\u{1b}]0;owned\u{7}\nroot=x".to_owned()),
        ..record()
    };

    let text = hostile.to_json().unwrap();

    assert!(!text.contains(['\u{1b}', '\u{7}', '\n']), "{text}");
    let read = BootRecord::parse(text.as_bytes()).unwrap();
    assert_eq!(
        read.command_line.printable().to_string(),
        "quiet\\x1b]0;owned\\x07\\x0aroot=x"
    );
}

#[test]
fn fields_a_reader_does_not_know_are_left_out() {
    let text = record()
        .to_json()
        .unwrap()
        .replace(r#""time":"#, r#""origin":"countdown","time":"#);

    assert_eq!(BootRecord::parse(text.as_bytes()), Ok(record()));
}

#[test]
fn another_schema_is_refused() {
    let text = record()
        .to_json()
        .unwrap()
        .replace(r#""schema":1"#, r#""schema":2"#);

    let error = BootRecord::parse(text.as_bytes()).unwrap_err();

    assert_eq!(error, RecordError::Schema(2));
    assert_eq!(error.to_string(), "the record has schema 2, not 1");
}

#[test]
fn a_file_that_is_no_record_is_refused_with_where() {
    for text in [
        "",
        "not json",
        "{}",
        r#"{"schema":"1"}"#,
        r#"{"schema":1}"#,
        r#"{"schema":-1}"#,
        "[1]",
    ] {
        assert!(
            matches!(
                BootRecord::parse(text.as_bytes()),
                Err(RecordError::Invalid { .. })
            ),
            "{text}"
        );
    }
    let without_time = record()
        .to_json()
        .unwrap()
        .replace(r#""time":1793246955,"#, "");
    let error = BootRecord::parse(without_time.as_bytes()).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("the record cannot be read (line 1, column "),
        "{error}"
    );
    assert!(BootRecord::parse(&[0xff, 0xfe]).is_err());
}

#[test]
fn a_file_that_ends_early_is_refused_at_column_0() {
    assert_eq!(
        BootRecord::parse(b""),
        Err(RecordError::Invalid { line: 1, column: 0 })
    );
    assert_eq!(
        BootRecord::parse(b"{\n"),
        Err(RecordError::Invalid { line: 2, column: 0 })
    );
}

#[test]
fn a_record_without_a_device_tree_had_none() {
    let none = BootRecord {
        devicetree: None,
        ..record()
    };
    let text = record()
        .to_json()
        .unwrap()
        .replace(r#""devicetree":"/boot/dtbs/board.dtb","#, "");

    assert_eq!(BootRecord::parse(text.as_bytes()), Ok(none.clone()));
    assert!(none.to_json().unwrap().contains(r#""devicetree":null,"#));
}

#[test]
fn a_file_longer_than_a_record_is_not_read() {
    let mut text = record().to_json().unwrap();
    text.push_str(&" ".repeat(BootRecord::MAX_LEN));

    let error = BootRecord::parse(text.as_bytes()).unwrap_err();

    assert_eq!(error, RecordError::TooLong { max: 65_536 });
    assert_eq!(error.to_string(), "the record is longer than 65536 bytes");
    let padded = format!("{}{}", record().to_json().unwrap(), " ".repeat(100));
    assert_eq!(BootRecord::parse(padded.as_bytes()), Ok(record()));
}

proptest! {
    #[test]
    fn any_record_reads_back_as_it_was_written(
        time in any::<u64>(),
        texts in proptest::collection::vec(any::<String>(), 6),
        devicetree in proptest::option::of(any::<String>()),
        measured in any::<bool>(),
        command_line_edited in any::<bool>(),
    ) {
        let text = |at: usize| Untrusted::new(texts[at].clone());
        let written = BootRecord {
            time,
            environment: text(0),
            kernel: text(1),
            initramfs: text(2),
            command_line: text(3),
            devicetree: devicetree.map(Untrusted::new),
            loaded_by: text(4),
            measured,
            command_line_edited,
        };

        let json = written.to_json().unwrap();

        // C0 controls, line breaks among them, are escaped: the file is
        // one line.
        prop_assert!(!json.contains(|c: char| c < ' '));
        prop_assert_eq!(BootRecord::parse(json.as_bytes()), Ok(written));
    }

    #[test]
    fn parsing_any_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = BootRecord::parse(&bytes);
    }
}
