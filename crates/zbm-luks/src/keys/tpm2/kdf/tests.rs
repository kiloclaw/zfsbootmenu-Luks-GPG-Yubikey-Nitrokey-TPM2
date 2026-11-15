//! The fixture `token-tpm2-argon2id.json` is the PIN token of
//! `token-tpm2-pin.json` with the salt and costs of known-answer vector V3
//! of the SB21 plan (salt SHA-256 of `salt`, 1049599 bytes, 3 passes, one
//! lane); `token-tpm2-argon2id-f45.json` is the token 0 that systemd 262.1
//! enrolled with `--tpm2-with-pin=yes` on a Fedora 45 host.

use super::*;
use crate::crypto::sha256;
use zbm_testkit::repo_root;

/// The token of the fixture `name`.
fn fixture(name: &str) -> Token {
    let path = repo_root()
        .join("crates/zbm-luks/tests/fixtures")
        .join(name);
    Token::from_export("0", &std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A PIN token with `fields` added.
fn token(fields: &str) -> Token {
    let json = format!(r#"{{"type":"systemd-tpm2","tpm2-pin":true{fields}}}"#);
    Token::from_export("0", &json).unwrap()
}

/// A PIN token with a 32-byte salt and the Argon2id costs given.
fn costs(memcost: u64, iterations: u32, lanes: u32) -> Token {
    token(&format!(
        r#","tpm2_salt":"Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=",
           "tpm2_argon2id_memcost":{memcost},"tpm2_argon2id_iterations":{iterations},
           "tpm2_argon2id_lanes":{lanes}"#
    ))
}

#[test]
fn three_argon2id_fields_select_argon2id_with_kib_rounded_down() {
    let kdf = PinKdf::of(&fixture("token-tpm2-argon2id.json")).unwrap();
    assert_eq!(
        kdf,
        PinKdf::Argon2id {
            salt: sha256(b"salt").to_vec(),
            params: Argon2idParams {
                memcost_bytes: 1_049_599,
                memory_kib: 1024,
                iterations: 3,
                lanes: 1,
            },
        }
    );
    // 1023 bytes over a whole KiB are left out, as systemd leaves them.
    assert!(matches!(
        PinKdf::of(&costs(1024 * 1024 + 1023, 3, 1)),
        Ok(PinKdf::Argon2id { params, .. }) if params.memory_kib == 1024
    ));
}

#[test]
fn a_salt_alone_keeps_pbkdf2_and_no_salt_keeps_the_pin() {
    let salted = fixture("token-tpm2-pin.json");
    let salt = base64_decode(salted.text("tpm2_salt").unwrap()).unwrap();
    assert_eq!(PinKdf::of(&salted), Ok(PinKdf::Pbkdf2 { salt }));
    assert_eq!(PinKdf::of(&token("")), Ok(PinKdf::Plain));
    assert_eq!(PinKdf::of(&token(r#","tpm2_salt":"""#)), Ok(PinKdf::Plain));
    // A salt that is not base64 is empty, as it always was here.
    assert_eq!(
        PinKdf::of(&token(r#","tpm2_salt":"!""#)),
        Ok(PinKdf::Pbkdf2 { salt: Vec::new() })
    );
}

#[test]
fn pin_tokens_refuse_what_argon2_cannot_derive() {
    let four_tib = 1_u64 << 42;
    assert_eq!(PinKdf::of(&costs(four_tib, 1, 1)), Err(Malformed::Memory));
    assert!(PinKdf::of(&costs(four_tib - 1, 1, 1)).is_ok());
    assert_eq!(
        PinKdf::of(&costs(1 << 40, 1, 1 << 24)),
        Err(Malformed::Lanes)
    );
    assert!(PinKdf::of(&costs(1 << 40, 1, (1 << 24) - 1)).is_ok());
    assert_eq!(
        PinKdf::of(&costs(8 * 1024 * 4 - 1, 1, 4)),
        Err(Malformed::MemoryPerLane)
    );
    assert!(PinKdf::of(&costs(8 * 1024 * 4, 1, 4)).is_ok());
    // systemd's own parser comes first, and is named.
    let zero = PinKdf::of(&costs(1024, 1, 0)).unwrap_err();
    assert_eq!(
        zero,
        Malformed::Field(BadField::Zero("tpm2_argon2id_lanes"))
    );
    assert_eq!(zero.to_string(), "tpm2_argon2id_lanes is 0");
    assert_eq!(
        Malformed::Lanes.to_string(),
        "tpm2_argon2id_lanes is more than 16777215"
    );
}

#[test]
fn an_argon2id_pin_token_without_a_long_enough_salt_is_malformed() {
    let fields = r#""tpm2_argon2id_memcost":1048576,"tpm2_argon2id_iterations":2,
                    "tpm2_argon2id_lanes":1"#;
    assert_eq!(
        PinKdf::of(&token(&format!(",{fields}"))),
        Err(Malformed::NoSalt)
    );
    assert_eq!(
        PinKdf::of(&token(&format!(r#","tpm2_salt":"!",{fields}"#))),
        Err(Malformed::SaltNotBase64)
    );
    let seven = PinKdf::of(&token(&format!(r#","tpm2_salt":"MTIzNDU2Nw==",{fields}"#)));
    assert_eq!(seven, Err(Malformed::ShortSalt(7)));
    assert_eq!(
        seven.unwrap_err().to_string(),
        "tpm2_salt has 7 bytes, fewer than 8"
    );
    assert!(PinKdf::of(&token(&format!(r#","tpm2_salt":"MTIzNDU2Nzg=",{fields}"#))).is_ok());
}

#[test]
fn the_fedora_45_token_reads_as_3256447_kib_2_passes_24_lanes() {
    let token = fixture("token-tpm2-argon2id-f45.json");
    let Ok(PinKdf::Argon2id { salt, params }) = PinKdf::of(&token) else {
        panic!("the token is not Argon2id");
    };
    assert_eq!(salt.len(), 32);
    assert_eq!(
        params,
        Argon2idParams {
            memcost_bytes: 3_334_601_937,
            memory_kib: 3_256_447,
            iterations: 2,
            lanes: 24,
        }
    );
}

#[test]
fn an_argon2id_salt_is_read_with_systemd_s_whitespace_skipped() {
    let fields = r#""tpm2_argon2id_memcost":1048576,"tpm2_argon2id_iterations":2,
                    "tpm2_argon2id_lanes":1"#;
    let spaced = token(&format!(r#","tpm2_salt":" MTIz NDU2\tNzg=\r\n",{fields}"#));
    assert!(matches!(
        PinKdf::of(&spaced),
        Ok(PinKdf::Argon2id { salt, .. }) if salt == b"12345678"
    ));
    // A PBKDF2 salt keeps the old reading: not base64, so empty.
    assert_eq!(
        PinKdf::of(&token(r#","tpm2_salt":"MTIz NDU2Nzg=""#)),
        Ok(PinKdf::Pbkdf2 { salt: Vec::new() })
    );
}

#[test]
fn systemd_s_rules_alone_hold_for_every_token() {
    let costs = |lanes: &str| {
        format!(
            r#","tpm2_argon2id_memcost":1024,"tpm2_argon2id_iterations":1,
               "tpm2_argon2id_lanes":{lanes}"#
        )
    };
    assert_eq!(check_systemd_rules(&token("")), Ok(()));
    assert_eq!(
        check_systemd_rules(&token(&costs("0"))),
        Err(Malformed::Field(BadField::Zero("tpm2_argon2id_lanes")))
    );
    assert_eq!(
        check_systemd_rules(&token(r#","tpm2_argon2id_lanes":1"#)),
        Err(Malformed::Field(BadField::Incomplete))
    );
    assert_eq!(
        check_systemd_rules(&token(&costs("1"))),
        Err(Malformed::NoSalt)
    );
    let empty_salt = format!(r#","tpm2_salt":""{}"#, costs("1"));
    assert_eq!(
        check_systemd_rules(&token(&empty_salt)),
        Err(Malformed::NoSalt)
    );
    // What only a derivation could not use is left to the PIN's own rules.
    let short = format!(r#","tpm2_salt":"AAAAAA=="{}"#, costs("16777216"));
    assert_eq!(check_systemd_rules(&token(&short)), Ok(()));
    assert!(PinKdf::of(&token(&short)).is_err());
}

/// Costs of `memory_kib` KiB, `iterations` passes and `lanes` lanes, with a
/// memory cost of exactly that many KiB.
fn params(memory_kib: u32, iterations: u32, lanes: u32) -> Argon2idParams {
    Argon2idParams {
        memcost_bytes: u64::from(memory_kib) * 1024,
        memory_kib,
        iterations,
        lanes,
    }
}

#[test]
fn argon2_uses_whole_groups_of_four_blocks_a_lane() {
    // Fedora 45's token: 3256447 KiB in 24 lanes, of which 3256416 are used.
    let f45 = Argon2idParams {
        memcost_bytes: 3_334_601_937,
        ..params(3_256_447, 2, 24)
    };
    assert_eq!(f45.need(), 3_334_569_984);
    assert_eq!(f45.work(), 2 * 3_334_569_984);
    assert_eq!(params(1024, 3, 1).need(), 1 << 20);
    // The largest costs a token can have do not overflow.
    let largest = params(u32::MAX, u32::MAX, 1);
    assert_eq!(largest.need(), (u64::from(u32::MAX) - 3) * 1024);
    assert_eq!(
        largest.work(),
        u128::from(largest.need()) * u128::from(u32::MAX)
    );
}

#[test]
fn meminfo_gives_the_total_and_the_available_memory() {
    let meminfo = "MemTotal:        4028248 kB\nMemFree:          123456 kB\n\
                   MemAvailable:    3500000 kB\nBuffers:            1000 kB\n";
    assert_eq!(
        parse_meminfo(meminfo),
        Some(Memory {
            machine: 4_028_248 * 1024,
            total: 4_028_248 * 1024,
            available: 3_500_000 * 1024,
        })
    );
    for broken in [
        "",
        "MemTotal: 1 kB\n",
        "MemTotal: 1 kB\nMemAvailable: lots kB\n",
        "MemTotal: 1 MB\nMemAvailable: 1 kB\n",
        "MemTotal 1 kB\nMemAvailable: 1 kB\n",
        "MemTotal: 18446744073709551615 kB\nMemAvailable: 1 kB\n",
    ] {
        assert_eq!(parse_meminfo(broken), None, "{broken:?}");
    }
}

#[test]
fn the_cgroup_path_gives_each_group_up_to_the_root() {
    let dirs = cgroup_dirs("1:name=systemd:/x\n0::/system.slice/zbm.service\n").unwrap();
    assert_eq!(
        dirs,
        [
            PathBuf::from("/sys/fs/cgroup/system.slice/zbm.service"),
            PathBuf::from("/sys/fs/cgroup/system.slice"),
            PathBuf::from("/sys/fs/cgroup"),
        ]
    );
    // The root, which a cgroup namespace may limit.
    assert_eq!(
        cgroup_dirs("0::/\n"),
        Some(vec![PathBuf::from("/sys/fs/cgroup")])
    );
    // Only cgroup v1: nothing caps the memory.
    assert_eq!(cgroup_dirs("4:memory:/x\n"), Some(Vec::new()));
    for broken in ["0::relative\n", "0::/a/../b\n", "0::/a/./b\n", "0::\n"] {
        assert_eq!(cgroup_dirs(broken), None, "{broken:?}");
    }
}

#[test]
fn a_cgroup_s_limit_is_read_or_refused() {
    let files = zbm_testkit::fakes::FakeFiles::new();
    let dir = Path::new("/sys/fs/cgroup/a");
    // No group there: cgroup2 is mounted elsewhere, or not at all.
    assert_eq!(cgroup_limit(&files, dir), Some(Limit::Unmounted));
    // A group without the memory controller.
    files.file(dir.join("cgroup.controllers"), "cpu io\n");
    assert_eq!(cgroup_limit(&files, dir), Some(Limit::Unlimited));
    files.file(dir.join("memory.max"), "max\n");
    assert_eq!(cgroup_limit(&files, dir), Some(Limit::Unlimited));
    files.file(dir.join("memory.max"), "1048576\n");
    assert_eq!(cgroup_limit(&files, dir), None);
    files.file(dir.join("memory.current"), "4096\n");
    assert_eq!(
        cgroup_limit(&files, dir),
        Some(Limit::Max {
            max: 1_048_576,
            current: 4096
        })
    );
    files.file(dir.join("memory.max"), "-1\n");
    assert_eq!(cgroup_limit(&files, dir), None);
    let denied = Path::new("/sys/fs/cgroup/b");
    files.fail(
        denied.join("memory.max"),
        std::io::ErrorKind::PermissionDenied,
    );
    assert_eq!(cgroup_limit(&files, denied), None);
    let hidden = Path::new("/sys/fs/cgroup/c");
    files.fail(
        hidden.join("cgroup.controllers"),
        std::io::ErrorKind::PermissionDenied,
    );
    assert_eq!(cgroup_limit(&files, hidden), None);
}

#[test]
fn the_margin_is_a_sixteenth_of_memtotal_and_at_least_64_mib() {
    let memory = |machine, total| Memory {
        machine,
        total,
        available: total,
    };
    assert_eq!(memory(4 << 30, 4 << 30).margin(), 256 << 20);
    assert_eq!(memory(512 << 20, 512 << 20).margin(), 64 << 20);
    // A cgroup's limit caps what is available, not the margin.
    assert_eq!(memory(64 << 30, 4 << 30).margin(), 4 << 30);
}

#[test]
fn a_derivation_must_fit_the_cap_the_budget_and_the_memory() {
    let roomy = Some(Memory {
        machine: 8 << 30,
        total: 8 << 30,
        available: 6 << 30,
    });
    // 1 GiB, 32 passes: the cap exactly.
    let at_cap = params(1 << 20, 32, 4);
    assert_eq!(check_cost(&at_cap, roomy, 0, RUN_BUDGET), Ok(()));
    let above = params(1 << 20, 33, 4);
    assert_eq!(
        check_cost(&above, roomy, 0, RUN_BUDGET),
        Err(TooCostly::Work { work: 33 << 30 })
    );
    // The budget: two at the cap, not three.
    assert_eq!(check_cost(&at_cap, roomy, 32 << 30, RUN_BUDGET), Ok(()));
    assert_eq!(
        check_cost(&at_cap, roomy, 64 << 30, RUN_BUDGET),
        Err(TooCostly::Budget {
            spent: 64 << 30,
            budget: RUN_BUDGET,
            work: 32 << 30,
        })
    );
    assert_eq!(
        check_cost(&at_cap, None, 0, RUN_BUDGET),
        Err(TooCostly::Unknown)
    );
    // The memory available, with 512 MiB kept for the system.
    let tight = |available| {
        Some(Memory {
            machine: 8 << 30,
            total: 8 << 30,
            available,
        })
    };
    let one_gib = params(1 << 20, 1, 4);
    assert_eq!(
        check_cost(&one_gib, tight((1 << 30) + (512 << 20)), 0, RUN_BUDGET),
        Ok(())
    );
    assert_eq!(
        check_cost(&one_gib, tight((1 << 30) + (511 << 20)), 0, RUN_BUDGET),
        Err(TooCostly::Memory {
            need: 1 << 30,
            margin: 512 << 20,
            available: (1 << 30) + (511 << 20),
        })
    );
}

#[test]
fn sizes_and_refusals_read_as_a_person_would_say_them() {
    assert_eq!(size(3_334_569_984), "3.1 GiB");
    assert_eq!(size(1 << 30), "1.0 GiB");
    assert_eq!(size(2_791_728_742), "2.6 GiB");
    assert_eq!(size(240 << 20), "240 MiB");
    assert_eq!(size(0), "0 MiB");
    let memory = TooCostly::Memory {
        need: 3_334_569_984,
        margin: 256 << 20,
        available: 2_791_728_742 + (256 << 20),
    };
    assert_eq!(
        memory.told(),
        "This volume's TPM2 PIN needs 3.1 GiB of memory to check; 2.6 GiB is free."
    );
    let failed = TooCostly::Failed(KdfError::OutOfMemory { blocks: 4 });
    assert_eq!(
        failed.told(),
        "The memory to check this volume's TPM2 PIN could not be reserved."
    );
    assert_eq!(
        failed.to_string(),
        "4 KiB of work memory could not be reserved"
    );
    assert_eq!(
        TooCostly::Work { work: 70 << 30 }.told(),
        "This volume's TPM2 PIN needs more work to check than ZFSBootMenu allows (70 \
         GiB-passes). Enrol it again with fixed --tpm2-argon2id-memory= and \
         --tpm2-argon2id-iterations=."
    );
}
