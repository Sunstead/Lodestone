//! Host port allocation for new instances.

use std::collections::HashSet;
use std::net::{Ipv4Addr, TcpListener};
use std::ops::RangeInclusive;

/// Whether nothing on this host is listening on `port` for `addr`.
pub fn is_free(addr: Ipv4Addr, port: u16) -> bool {
    TcpListener::bind((addr, port)).is_ok()
}

/// First port in `range` that no instance uses and the host has free.
pub fn allocate(range: RangeInclusive<u16>, used: &HashSet<u16>, addr: Ipv4Addr) -> Option<u16> {
    allocate_with(range, used, |port| is_free(addr, port))
}

pub fn allocate_with(range: RangeInclusive<u16>, used: &HashSet<u16>, free: impl Fn(u16) -> bool) -> Option<u16> {
    range.into_iter().find(|port| !used.contains(port) && free(*port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_used_and_busy_ports() {
        let used = HashSet::from([25565, 25566]);
        assert_eq!(allocate_with(25565..=u16::MAX, &used, |_| true), Some(25567));
        assert_eq!(allocate_with(25565..=u16::MAX, &used, |port| port != 25567), Some(25568));
    }

    #[test]
    fn stays_inside_the_range() {
        let used = HashSet::from([25565, 25566]);
        assert_eq!(allocate_with(25565..=25567, &used, |_| true), Some(25567));
        assert_eq!(allocate_with(25565..=25566, &used, |_| true), None);
    }

    #[test]
    fn detects_a_busy_port() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_free(Ipv4Addr::LOCALHOST, port));
    }
}
