//! The legacy host-list format is comma separated. Escape serial addresses only:
//! ordinary SSH aliases (including literal percent sequences) retain their bytes.

pub(crate) fn encode(hosts: &[String]) -> String {
    hosts
        .iter()
        .map(|host| {
            if host.starts_with(crate::serial_session::SCHEME) {
                host.replace('%', "%25").replace(',', "%2C")
            } else {
                host.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn decode(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| {
            if host.starts_with(crate::serial_session::SCHEME) {
                host.replace("%2C", ",").replace("%25", "%")
            } else {
                host.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_options_and_percent_sequences_survive_host_list_roundtrip() {
        let hosts = vec![
            "dev@host:2222".into(),
            "serial:/dev/tty%2C,9600,7E2".into(),
            "ssh%2C-alias".into(),
        ];
        let stored = encode(&hosts);
        assert_eq!(stored.split(',').count(), hosts.len());
        assert_eq!(decode(&stored), hosts);
        assert_eq!(decode(" a@b , ,c@d:2222 "), vec!["a@b", "c@d:2222"]);
    }
}
