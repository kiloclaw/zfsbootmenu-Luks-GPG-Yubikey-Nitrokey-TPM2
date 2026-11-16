//! The name servers of a DHCP lease that mkinitcpio's `net` hook leaves in
//! `/tmp/net-<device>.conf` (input I16 of `docs/dev/threat-model.md`).
//!
//! `ipconfig` writes one `NAME='value'` line per field, and `0.0.0.0` for a
//! name server the server did not give; an older `ipconfig` may write the
//! value without quotes. Only the `IPV4DNS0` and `IPV4DNS1` lines are read,
//! and only a value that is a dotted IPv4 address, quoted or not, counts: the
//! file is never sourced, and no other field is interpreted. A field whose
//! quoted value holds a newline, as a host name from the server may, can
//! start a line of its own; the server that writes it names the name servers
//! anyway.

use std::net::Ipv4Addr;

/// The name servers a resolver reads at most (glibc's `MAXNS`).
const MOST: usize = 3;

/// The fields of the lease that name a name server.
const FIELDS: [&str; 2] = ["IPV4DNS0", "IPV4DNS1"];

/// The name servers of the lease `text`, in the order of its lines: the
/// addresses of its `IPV4DNS0` and `IPV4DNS1` lines, `'a.b.c.d'` or
/// `a.b.c.d`, without `0.0.0.0` and repeats, at most three. A value that is
/// no address is skipped.
pub fn name_servers(text: &str) -> Vec<Ipv4Addr> {
    let mut servers = Vec::new();
    for line in text.lines() {
        if servers.len() == MOST {
            break;
        }
        let Some(address) = name_server(line) else {
            continue;
        };
        if !address.is_unspecified() && !servers.contains(&address) {
            servers.push(address);
        }
    }
    servers
}

/// The address of one line of the lease, when it is a name server's field
/// whose whole value is an address.
fn name_server(line: &str) -> Option<Ipv4Addr> {
    let (name, value) = line.split_once('=')?;
    if !FIELDS.contains(&name) {
        return None;
    }
    let bare = value
        .strip_prefix('\'')
        .and_then(|quoted| quoted.strip_suffix('\''))
        .unwrap_or(value);
    bare.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::QEMU_LEASE;

    fn addresses(list: &[&str]) -> Vec<Ipv4Addr> {
        list.iter()
            .map(|address| address.parse().unwrap())
            .collect()
    }

    #[test]
    fn the_lease_of_mkinitcpio_s_net_names_its_one_name_server() {
        assert_eq!(name_servers(QEMU_LEASE), addresses(&["10.0.2.3"]));
    }

    #[test]
    fn quoted_and_bare_values_are_both_read() {
        let lease = "IPV4DNS0='192.0.2.1'\nIPV4DNS1=192.0.2.2\n";

        assert_eq!(name_servers(lease), addresses(&["192.0.2.1", "192.0.2.2"]));
    }

    #[test]
    fn name_servers_that_are_not_addresses_are_skipped() {
        let lease = "\
IPV4DNS0='1.2.3.4; reboot'
IPV4DNS1='1.2.3.4'\\''x'
IPV4DNS0=1.2.3.4.5
IPV4DNS1='01.2.3.4'
IPV4DNS0=
IPV4DNS1=''
IPV4DNS0='
IPV4DNS1=' 1.2.3.4'
IPV4DNS0=\"1.2.3.4\"
HOSTNAME='$(reboot)'
HOSTNAME='1.2.3.4'
 IPV4DNS0='1.2.3.4'
export IPV4DNS0='1.2.3.4'
IPV4DNS2='1.2.3.4'
";

        assert_eq!(name_servers(lease), Vec::<Ipv4Addr>::new());
    }

    #[test]
    fn unset_and_repeated_name_servers_are_left_out() {
        let lease = "IPV4DNS0=0.0.0.0\nIPV4DNS1='192.0.2.1'\nIPV4DNS0='192.0.2.1'\n";

        assert_eq!(name_servers(lease), addresses(&["192.0.2.1"]));
    }

    #[test]
    fn at_most_three_name_servers_are_read_in_the_order_of_the_lines() {
        let lease = "\
IPV4DNS1='192.0.2.4'
IPV4DNS0='192.0.2.3'
IPV4DNS1='192.0.2.2'
IPV4DNS0='192.0.2.1'
";

        assert_eq!(
            name_servers(lease),
            addresses(&["192.0.2.4", "192.0.2.3", "192.0.2.2"])
        );
    }

    #[test]
    fn a_lease_without_name_servers_names_none() {
        assert!(name_servers("").is_empty());
        assert!(name_servers("DEVICE='eth0'\n").is_empty());
    }
}
