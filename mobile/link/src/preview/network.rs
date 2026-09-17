use std::{
    io,
    net::{IpAddr, UdpSocket},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanAddress {
    pub address: IpAddr,
    pub name: String,
    pub preferred: bool,
}

pub fn addresses() -> io::Result<Vec<LanAddress>> {
    // UDP connect only asks the kernel for a route. It sends no datagram, performs
    // no DNS request and does not require the documentation address to respond.
    let preferred = UdpSocket::bind("0.0.0.0:0").ok().and_then(|socket| {
        socket.connect("192.0.2.1:9").ok()?;
        Some(socket.local_addr().ok()?.ip())
    });
    let mut values: Vec<_> = if_addrs::get_if_addrs()?
        .into_iter()
        .filter_map(|interface| {
            let address = interface.ip();
            usable(address).then_some(LanAddress {
                address,
                name: interface.name,
                preferred: preferred == Some(address),
            })
        })
        .collect();
    values.sort_by_key(|entry| {
        (!entry.preferred, entry.address.is_ipv6(), entry.name.clone(), entry.address)
    });
    values.dedup_by_key(|entry| entry.address);
    Ok(values)
}

pub fn usable(address: IpAddr) -> bool {
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return false;
    }
    match address {
        IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_broadcast(),
        IpAddr::V6(v6) => !v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_unreachable_and_wildcard_addresses_but_keeps_vpn_addresses() {
        for address in ["127.0.0.1", "0.0.0.0", "169.254.1.2", "::1", "::", "fe80::1", "224.0.0.1"]
        {
            assert!(!usable(address.parse().unwrap()));
        }
        for address in ["192.168.31.250", "100.100.1.2", "fd00::12"] {
            assert!(usable(address.parse().unwrap()));
        }
    }
}
