//! The memory and work an Argon2id derivation may take, checked before the
//! PIN is asked for and again before each derivation, and the run's budget.

use super::*;

/// V3's work: 1 MiB, 3 passes, in bytes · passes.
const V3_WORK: u128 = 3 << 20;

/// `/proc/meminfo` with `total` and `available`, in KiB.
fn meminfo(total: u64, available: u64) -> String {
    format!("MemTotal: {total} kB\nMemFree: 0 kB\nMemAvailable: {available} kB\n")
}

/// Whether the person at the console was told that the PIN costs too
/// much, with `why`.
fn told_too_costly(fake: &FakeSystem, why: &str) -> bool {
    let output = fake.console.output_text();
    output.contains(why) && output.contains("Use another unlock method.")
}

/// The number of derivations `fake` saw start.
fn derivations(fake: &FakeSystem) -> usize {
    fake.console
        .output_text()
        .matches("Checking the PIN (Argon2id")
        .count()
}

#[test]
fn too_little_free_memory_refuses_the_token_before_any_pin() {
    let fake = v3_machine();
    // 1 GiB, of which 64 MiB are kept for the system and 60 MiB are free.
    fake.files.file("/proc/meminfo", meminfo(1 << 20, 60 << 10));
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert_eq!(fake.console.remaining().len(), 1);
    assert!(!ran_tpm2_tools(&fake));
    assert!(fake.kmsg().contains(
        "TPM2 token 0 on /dev/sda was not tried: it needs 1 MiB of memory and 64 MiB are kept \
         for the system, but 60 MiB are available"
    ));
    assert!(told_too_costly(
        &fake,
        "This volume's TPM2 PIN needs 1 MiB of memory to check; 0 MiB is free."
    ));
    assert!(fake.files.contents("/proc/self/oom_score_adj").is_none());
}

#[test]
fn memory_that_shrinks_after_the_prompt_ends_the_token_without_asking_again() {
    let fake = v3_machine();
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        if command.program() == "tpm2_load" {
            files.file("/proc/meminfo", meminfo(1 << 20, 1 << 10));
        }
        None
    });
    let result = try_v3(&fake, &simulated(), &[PIN, PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    // Not asked again, not counted as a wrong PIN, and the TPM not asked.
    assert_eq!(fake.console.remaining().len(), 1);
    let output = fake.console.output_text();
    assert!(!output.contains("The TPM did not release the key"));
    assert!(output.contains("0 MiB is free"));
    assert_eq!(derivations(&fake), 0);
    assert!(stdins(&fake, "tpm2_unseal").is_empty());
    let probes = fake.runner.lines();
    let probes = probes.iter().filter(|line| line.starts_with("tpm2_getcap"));
    assert_eq!(probes.count(), 1);
    assert!(fake.files.contents("/proc/self/oom_score_adj").is_none());
}

#[test]
fn a_cgroup_limit_caps_the_memory_available() {
    let cgroup = |fake: &FakeSystem, max: &str| {
        fake.files
            .file("/proc/self/cgroup", "0::/system.slice/zbm.service\n")
            .file("/sys/fs/cgroup/cgroup.controllers", "cpu io memory\n")
            .file(
                "/sys/fs/cgroup/system.slice/zbm.service/memory.max",
                "max\n",
            )
            .file("/sys/fs/cgroup/system.slice/memory.max", format!("{max}\n"))
            .file("/sys/fs/cgroup/system.slice/memory.current", "100663296\n");
    };
    // The parent's limit of 128 MiB, 96 MiB of it used, leaves 32 MiB.
    let fake = v3_machine();
    cgroup(&fake, "134217728");
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(fake.kmsg().contains(
        "it needs 1 MiB of memory and 512 MiB are kept for the system, but 32 MiB are available"
    ));
    // Without a limit, the machine's memory is what counts.
    let fake = v3_machine();
    cgroup(&fake, "max");
    assert!(try_v3(&fake, &simulated(), &[PIN]).is_ok());
    // A limit that cannot be read refuses the token.
    let fake = v3_machine();
    cgroup(&fake, "lots");
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(told_too_costly(
        &fake,
        "ZFSBootMenu cannot tell how much memory is free to check this volume's TPM2 PIN."
    ));
}

#[test]
fn a_cgroup_that_cannot_be_read_refuses_the_token() {
    let unknown = "the free memory could not be read from /proc/meminfo or the cgroup";
    // `/proc/self/cgroup` itself.
    let fake = v3_machine();
    fake.files
        .fail("/proc/self/cgroup", std::io::ErrorKind::PermissionDenied);
    assert!(ended_with(
        &try_v3(&fake, &simulated(), &[PIN]),
        TokenFailure::TooCostly
    ));
    assert!(fake.kmsg().contains(unknown));
    // A group below the root, where cgroup2 is not at /sys/fs/cgroup, as in
    // the hybrid layout.
    let fake = v3_machine();
    fake.files
        .file("/proc/self/cgroup", "0::/user.slice\n")
        .file("/sys/fs/cgroup/unified/user.slice/memory.max", "1048576\n");
    assert!(ended_with(
        &try_v3(&fake, &simulated(), &[PIN]),
        TokenFailure::TooCostly
    ));
    assert!(fake.kmsg().contains(unknown));
    assert_eq!(fake.console.remaining().len(), 1);
    // The root group where cgroup2 is not mounted, as in the initramfs,
    // and a kernel without cgroups: nothing caps the memory.
    let fake = v3_machine();
    fake.files.file("/proc/self/cgroup", "0::/\n");
    assert!(try_v3(&fake, &simulated(), &[PIN]).is_ok());
}

#[test]
fn the_root_of_a_cgroup_namespace_caps_the_memory_available() {
    // In a container, the root group is the container's, with its limit:
    // 64 MiB, 32 MiB of it used.
    let fake = v3_machine();
    fake.files
        .file("/proc/self/cgroup", "0::/\n")
        .file("/sys/fs/cgroup/memory.max", "67108864\n")
        .file("/sys/fs/cgroup/memory.current", "33554432\n");
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(fake.kmsg().contains("but 32 MiB are available"));
}

#[test]
fn the_margin_is_reckoned_from_memtotal_not_the_cgroup_s_limit() {
    // 64 GiB of memory, 6 GiB free; the cgroup allows 1 GiB, all of it
    // free. A sixteenth of the machine, 4 GiB, is more than the cgroup
    // has, so the token is refused.
    let fake = v3_machine();
    fake.files
        .file("/proc/meminfo", meminfo(64 << 20, 6 << 20))
        .file("/proc/self/cgroup", "0::/zbm\n")
        .file("/sys/fs/cgroup/cgroup.controllers", "memory\n")
        .file("/sys/fs/cgroup/zbm/memory.max", "1073741824\n")
        .file("/sys/fs/cgroup/zbm/memory.current", "0\n");
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(fake.kmsg().contains(
        "it needs 1 MiB of memory and 4.0 GiB are kept for the system, but 1.0 GiB are \
         available"
    ));
}

#[test]
fn a_memcost_larger_than_memtotal_is_refused_before_any_pin() {
    // V3's memory cost is 1049599 bytes; the machine has 1048576.
    let fake = v3_machine();
    fake.files.file("/proc/meminfo", meminfo(1 << 10, 1 << 10));
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(!ran_tpm2_tools(&fake));
    assert!(fake.kmsg().contains(
        "was not tried: tpm2_argon2id_memcost is 1049599 bytes, more than the 1048576 of this \
         machine"
    ));
    assert!(told_too_costly(
        &fake,
        "This volume's TPM2 PIN needs 1 MiB of memory to check; this machine has 1 MiB."
    ));

    // systemd refuses the token without a PIN too, but tells nobody.
    let json =
        fixture("token-tpm2-argon2id.json").replace(r#""tpm2-pin":true"#, r#""tpm2-pin":false"#);
    let pinless = Token::from_export("0", &json).unwrap();
    let fake = tpm_machine(None, b"secret");
    fake.files.file("/proc/meminfo", meminfo(1 << 10, 1 << 10));
    let (result, _) = try_without_plugin(&fake, &pinless);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert!(!ran_tpm2_tools(&fake));
    assert!(
        fake.kmsg()
            .contains("more than the 1048576 of this machine")
    );
    assert!(!fake.console.output_text().contains("TPM2 PIN"));
    // Where the memory cannot be read, it derives nothing, so it is tried.
    let fake = tpm_machine(None, b"secret");
    let (result, _) = try_without_plugin(&fake, &pinless);
    assert!(result.is_ok());
}

#[test]
fn an_unreadable_meminfo_refuses_the_argon2id_token() {
    let fake = v3_machine();
    fake.files
        .fail("/proc/meminfo", std::io::ErrorKind::PermissionDenied);
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    assert_eq!(fake.console.remaining().len(), 1);
    assert!(!ran_tpm2_tools(&fake));
    assert!(fake.kmsg().contains(
        "was not tried: the free memory could not be read from /proc/meminfo or the cgroup"
    ));
    assert!(told_too_costly(
        &fake,
        "ZFSBootMenu cannot tell how much memory is free"
    ));
}

#[test]
fn work_above_the_cap_is_refused_before_any_pin() {
    // Nearly 4 TiB of memory and 2³² − 1 passes, on a machine of 8 TiB:
    // the product would overflow 64 bits.
    let huge = fixture("token-tpm2-argon2id.json")
        .replace("1049599", "4398046510080")
        .replace(
            r#""tpm2_argon2id_iterations":3"#,
            r#""tpm2_argon2id_iterations":4294967295"#,
        );
    let huge = Token::from_export("0", &huge).unwrap();
    let fake = v3_machine();
    fake.files.file("/proc/meminfo", meminfo(1 << 33, 1 << 33));
    let (result, asked) = try_without_plugin(&fake, &huge);
    assert!(ended_with(&result, TokenFailure::TooCostly) && !asked);
    assert!(!ran_tpm2_tools(&fake));
    assert!(told_too_costly(
        &fake,
        "This volume's TPM2 PIN needs more work to check than ZFSBootMenu allows \
         (17592186023937 GiB-passes). Enrol it again with fixed --tpm2-argon2id-memory= and \
         --tpm2-argon2id-iterations=."
    ));
    assert!(
        fake.kmsg()
            .contains("its work is 17592186023937 GiB-passes, more than the 32 allowed")
    );
}

#[test]
fn the_run_s_budget_ends_further_argon2id_tokens() {
    let fake = v3_machine();
    // Room for two of V3's derivations, not three.
    for pin in ["0000", "1111", "2222", "3333"] {
        fake.console.secret(pin.as_bytes());
    }
    let args = [];
    let (a, b) = with_session(&fake, &simulated(), |session| {
        session.argon2id.budget = V3_WORK * 2 + 1;
        let a = crate::testing::target("a", "/dev/sda", &args);
        let b = crate::testing::target("b", "/dev/sdb", &args);
        (
            session.try_tpm2_token(a, &v3()),
            session.try_tpm2_token(b, &v3()),
        )
    });
    // Two wrong PINs were derived; the third was not asked for, as its
    // derivation would pass the budget, and the next token was refused
    // before its PIN.
    assert!(ended_with(&a, TokenFailure::TooCostly));
    assert!(ended_with(&b, TokenFailure::TooCostly));
    assert_eq!(derivations(&fake), 2);
    assert_eq!(stdins(&fake, "tpm2_unseal").len(), 2);
    assert_eq!(fake.console.remaining().len(), 2);
    let output = fake.console.output_text();
    let spent = "ZFSBootMenu has spent its PIN-checking budget for this unlock.";
    assert_eq!(output.matches(spent).count(), 2);
    assert!(
        fake.kmsg()
            .contains("this unlock has spent 1 of its 1 GiB-passes")
    );
}

#[test]
fn thirty_two_argon2id_tokens_with_a_cached_pin_cost_at_most_the_budget() {
    // Volume a opens with V3's PIN, which is cached. Volumes b and c each
    // have 32 tokens that copy one sealed object with another salt, which
    // the cached PIN does not open.
    let other = fixture("token-tpm2-argon2id.json").replace(
        "Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=",
        "T3RoZXIgc2FsdCwgMzIgYnl0ZXMgbG9uZywgaGVyZS4=",
    );
    let copies: Vec<Token> = (0..32)
        .map(|id| Token::from_export(&id.to_string(), &other).unwrap())
        .collect();
    let fake = v3_machine();
    fake.console.secret(PIN.as_bytes());
    let args = [];
    let spent = with_session(&fake, &simulated(), |session| {
        session.argon2id.budget = V3_WORK * 10;
        let a = crate::testing::target("a", "/dev/sda", &args);
        assert!(session.try_tpm2_token(a, &v3()).is_ok());
        for device in ["/dev/sdb", "/dev/sdc"] {
            let volume = crate::testing::target("v", device, &args);
            for copy in &copies {
                assert!(session.try_tpm2_token(volume, copy).is_err());
            }
        }
        session.argon2id.spent.get()
    });
    // V3's derivation, and the cached PIN on b's first token: not on its
    // other 31, nor on c's, whose sealed object refused it already.
    assert_eq!(derivations(&fake), 2);
    assert_eq!(spent, V3_WORK * 2);
    let asked = fake.console.output_text();
    assert_eq!(asked.matches("TPM2 PIN (leave empty").count(), 65);
}

#[test]
fn memory_that_shrinks_after_a_wrong_pin_ends_the_token_before_the_next_prompt() {
    let fake = v3_machine();
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        if command.program() == "tpm2_unseal" {
            files.file("/proc/meminfo", meminfo(1 << 20, 1 << 10));
        }
        None
    });
    let result = try_v3(&fake, &simulated(), &["0000", PIN]);
    assert!(ended_with(&result, TokenFailure::TooCostly));
    // The second PIN was not asked for.
    assert_eq!(fake.console.remaining().len(), 1);
    assert_eq!(derivations(&fake), 1);
    let output = fake.console.output_text();
    assert_eq!(output.matches("TPM2 PIN (leave empty").count(), 1);
    assert!(output.contains("0 MiB is free"));
}

#[test]
fn the_keyholder_raises_its_oom_score_before_reserving_work_memory() {
    let keyholder = Settings {
        keyholder: true,
        ..simulated()
    };
    let fake = v3_machine();
    assert!(try_v3(&fake, &keyholder, &[PIN]).is_ok());
    assert_eq!(
        fake.files.text("/proc/self/oom_score_adj").as_deref(),
        Some("1000\n")
    );
    // A derivation refused before it starts reserves nothing, and leaves
    // the score alone.
    let fake = v3_machine();
    fake.files.file("/proc/meminfo", meminfo(1 << 10, 1 << 10));
    assert!(try_v3(&fake, &keyholder, &[PIN]).is_err());
    assert!(fake.files.contents("/proc/self/oom_score_adj").is_none());
}

#[test]
fn a_client_that_unlocks_itself_leaves_its_oom_score_alone() {
    // No keyholder could start, so start-up or the menu derives, and goes
    // on as the menu afterwards.
    let fake = v3_machine();
    assert!(try_v3(&fake, &simulated(), &[PIN]).is_ok());
    assert_eq!(derivations(&fake), 1);
    assert!(fake.files.contents("/proc/self/oom_score_adj").is_none());
}
