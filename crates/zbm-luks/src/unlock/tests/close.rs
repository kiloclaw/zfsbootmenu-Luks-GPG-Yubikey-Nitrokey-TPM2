//! Closing the mappings before the boot: pools exported first.

use super::*;

#[test]
fn closing_exports_pools_first_and_keeps_busy_mappings() {
    let fake = machine(
        &[A, B],
        &[("zroot", "/dev/mapper/a1"), ("other", "/dev/vda")],
    );
    fake.console.secret(b"secret1");
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        "a1 /dev/loop1 none\nb1 /dev/loop2 none\n",
    );
    let prompts = Prompts::new(&fake);
    assert!(with_session(&fake, &settings(), &prompts, unlock_all).is_ok());
    fake.files.file("/fake/busy/b1", "");
    // A pool that cannot be exported does not keep its mapping open.
    fake.runner.respond("zpool export zroot", 1, "");
    assert_eq!(
        with_session(&fake, &settings(), &prompts, |session| session
            .close_all(None)),
        Err(LuksError::NotClosed {
            names: vec!["b1".to_owned()]
        })
    );
    let lines = fake.runner.lines();
    let export = lines
        .iter()
        .position(|line| line == "zpool export zroot")
        .unwrap();
    let close = lines
        .iter()
        .position(|line| line == "cryptsetup close a1")
        .unwrap();
    assert!(export < close);
    assert!(!lines.contains(&"zpool export other".to_owned()));
    assert_eq!(mappings(&fake), "b1\t/dev/loop2\t0\n");
    assert!(infos(&fake).contains(&"closed LUKS mapping a1".to_owned()));
    assert!(warnings(&fake).contains(&"unable to close LUKS mapping b1".to_owned()));
    assert!(warnings(&fake).contains(&"unable to export zroot, which uses a1".to_owned()));
    let missing = with_session(&fake, &settings(), &prompts, |session| {
        session.pools_on_device("/dev/missing")
    });
    assert!(missing.is_empty());

    // Only the named mapping is closed.
    let _ = fake.files.remove_file(Path::new("/fake/busy/b1"));
    let close = |only: Option<&str>| {
        let only = only.map(|name| name.parse::<MappingName>().unwrap());
        with_session(&fake, &settings(), &prompts, |session| {
            session.close_all(only.as_ref())
        })
    };
    assert_eq!(close(Some("a1")), Ok(()));
    assert_eq!(mappings(&fake), "b1\t/dev/loop2\t0\n");
    assert_eq!(close(Some("b1")), Ok(()));
    assert_eq!(mappings(&fake), "");

    // A mapping another session records while the close runs stays
    // recorded, to be closed before the boot.
    fake.files
        .file("/zfsbootmenu/luks-mappings", "a1\t/dev/loop1\t0\n")
        .block_device("/dev/mapper/a1");
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        if command.display() == "cryptsetup close a1" {
            let _ = files.append(
                Path::new("/zfsbootmenu/luks-mappings"),
                b"c1\t/dev/loop3\t0\n",
            );
        }
        None
    });
    assert_eq!(close(None), Ok(()));
    assert_eq!(mappings(&fake), "c1\t/dev/loop3\t0\n");
}
