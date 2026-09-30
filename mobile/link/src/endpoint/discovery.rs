//! mDNS 只公布地址和证书公钥指纹，邀请码与设备凭据从不进入广播。

use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::{collections::HashMap, net::IpAddr};

pub(super) struct Advertisement(ServiceDaemon);

impl Advertisement {
    pub fn register(
        address: IpAddr,
        port: u16,
        host_id: &str,
        name: &str,
        pin: &str,
    ) -> Option<Self> {
        let suffix: String = host_id.chars().filter(char::is_ascii_alphanumeric).take(12).collect();
        let properties = HashMap::from([
            ("version".to_owned(), "2".to_owned()),
            ("name".to_owned(), name.to_owned()),
            ("pin".to_owned(), pin.to_owned()),
        ]);
        let service = ServiceInfo::new(
            "_pebrel-pair._tcp.local.",
            &format!("Pebrel-{suffix}"),
            &format!("pebrel-{}.local.", suffix.to_ascii_lowercase()),
            address,
            port,
            properties,
        )
        .ok()?;
        let daemon = ServiceDaemon::new().ok()?;
        if daemon.register(service).is_err() {
            let _ = daemon.shutdown();
            return None;
        }
        Some(Self(daemon))
    }
}

impl Drop for Advertisement {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}
