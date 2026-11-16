//! The lease that mkinitcpio's `net` hook leaves in `/tmp/net-<device>.conf`
//! (input I16 of docs/dev/threat-model.md): the DHCP server (A6) chooses its
//! values. Reading its name servers may not panic or hang, and gives at most
//! three addresses, none unspecified and none twice, each the value of an
//! `IPV4DNS0` or `IPV4DNS1` line of the input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_startup::lease::name_servers;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let servers = name_servers(&text);
    assert!(servers.len() <= 3);
    for (index, server) in servers.iter().enumerate() {
        assert!(!server.is_unspecified());
        assert!(!servers[..index].contains(server));
        let bare = server.to_string();
        let quoted = format!("'{bare}'");
        assert!(text.lines().any(|line| {
            line.split_once('=').is_some_and(|(name, value)| {
                (name == "IPV4DNS0" || name == "IPV4DNS1") && (value == bare || value == quoted)
            })
        }));
    }
});
