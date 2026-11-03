//! Host port allocation for new instances.

use std::collections::HashSet;
use std::net::{Ipv4Addr, TcpListener};

/// Whether nothing on this host is listening on `port` for `addr`.
pub fn is_free(addr: Ipv4Addr, port: u16) -> bool {
    TcpListener::bind((addr, port)).is_ok()
}

/// First port at or after `start` that no instance uses and the host has free.
pub fn allocate(start: u16, used: &HashSet<u16>, addr: Ipv4Addr) -> Option<u16> {
    allocate_with(start, used, |port| is_free(addr, port))
}

pub fn allocate_with(start: u16, used: &HashSet<u16>, free: impl Fn(u16) -> bool) -> Option<u16> {
    (start..=u16::MAX).find(|port| !used.contains(port) && free(*port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_used_and_busy_ports() {
        let used = HashSet::from([25565, 25566]);
        assert_eq!(allocate_with(25565, &used, |_| true), Some(25567));
        assert_eq!(allocate_with(25565, &used, |port| port != 25567), Some(25568));
    }

    #[test]
    fn detects_a_busy_port() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_free(Ipv4Addr::LOCALHOST, port));
    }
}
