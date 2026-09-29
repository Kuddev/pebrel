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
    sort_addresses(&mut values);
    Ok(values)
}

fn sort_addresses(values: &mut Vec<LanAddress>) {
    // 默认路由通常指向 Wi-Fi；手机换网后，二维码中的 Tailnet 地址仍可直连。
    values.sort_by_key(|entry| {
        (
            !is_tailnet(entry.address),
            !entry.preferred,
            entry.address.is_ipv6(),
            entry.name.clone(),
            entry.address,
        )
    });
    values.dedup_by_key(|entry| entry.address);
}

fn is_tailnet(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [first, second, ..] = address.octets();
            first == 100 && (64..=127).contains(&second)
        },
        IpAddr::V6(address) => address.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
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

    fn entry(address: &str, preferred: bool) -> LanAddress {
        LanAddress { address: address.parse().unwrap(), name: address.into(), preferred }
    }

    #[test]
    fn tailnet_addresses_sort_before_the_default_lan_route() {
        let mut addresses = vec![
            entry("192.168.1.2", true),
            entry("fd7a:115c:a1e0::2", false),
            entry("100.64.1.2", false),
            entry("10.0.0.2", false),
        ];
        sort_addresses(&mut addresses);
        assert_eq!(addresses[0].address, "100.64.1.2".parse::<IpAddr>().unwrap());
        assert_eq!(addresses[1].address, "fd7a:115c:a1e0::2".parse::<IpAddr>().unwrap());
        assert_eq!(addresses[2].address, "192.168.1.2".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn ordinary_networks_keep_the_default_route_preference() {
        let mut addresses = vec![entry("10.0.0.2", false), entry("192.168.1.2", true)];
        sort_addresses(&mut addresses);
        assert!(addresses[0].preferred);
    }

    #[test]
    fn tailnet_detection_uses_the_exact_address_ranges() {
        for address in ["100.64.0.0", "100.127.255.255", "fd7a:115c:a1e0::1"] {
            assert!(is_tailnet(address.parse().unwrap()), "{address}");
        }
        for address in ["100.63.255.255", "100.128.0.0", "100.1.2.3", "fd7a:115c:a1e1::1"] {
            assert!(!is_tailnet(address.parse().unwrap()), "{address}");
        }
    }

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
