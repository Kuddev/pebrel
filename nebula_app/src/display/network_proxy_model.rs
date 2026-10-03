//! Shared model for the manual SSH proxy controls.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManualProxyProtocol {
    #[default]
    Socks5,
    /// Remote DNS. curl and git treat this differently from [`Self::Socks5`].
    Socks5h,
    Http,
}

pub(crate) const MANUAL_PROXY_PROTOCOL_OPTIONS: [ManualProxyProtocol; 3] =
    [ManualProxyProtocol::Socks5, ManualProxyProtocol::Socks5h, ManualProxyProtocol::Http];

pub(crate) fn manual_proxy_protocol_label(protocol: ManualProxyProtocol) -> &'static str {
    match protocol {
        ManualProxyProtocol::Socks5 => "SOCKS5",
        ManualProxyProtocol::Socks5h => "SOCKS5H",
        ManualProxyProtocol::Http => "HTTP",
    }
}

/// State of the network test for the settings currently persisted on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum ProxyTestStatus {
    #[default]
    Idle,
    Running,
    Complete {
        outcome: crate::proxy_test::ProxyTestOutcome,
        elapsed_ms: u64,
    },
}

/// Split a persisted proxy URL into the protocol control and editable address.
pub(crate) fn manual_proxy_parts(value: &str) -> (ManualProxyProtocol, &str) {
    let value = value.trim();
    for (prefix, protocol) in [
        ("socks5h://", ManualProxyProtocol::Socks5h),
        ("socks5://", ManualProxyProtocol::Socks5),
        ("socks://", ManualProxyProtocol::Socks5),
        ("http://", ManualProxyProtocol::Http),
    ] {
        if let Some(address) = crate::ssh_proxy::strip_prefix_ignore_case(value, prefix) {
            return (protocol, address);
        }
    }
    (ManualProxyProtocol::Socks5, value)
}

pub(crate) fn manual_proxy_value(protocol: ManualProxyProtocol, address: &str) -> String {
    let address = address.trim();
    if address.is_empty() {
        return String::new();
    }
    match protocol {
        ManualProxyProtocol::Socks5 => format!("socks5://{address}"),
        ManualProxyProtocol::Socks5h => format!("socks5h://{address}"),
        ManualProxyProtocol::Http => format!("http://{address}"),
    }
}

fn is_scheme_only(typed: &str) -> bool {
    const PREFIXES: &[&str] = &["socks5h://", "socks5://", "socks://", "http://"];
    PREFIXES.iter().any(|prefix| typed.eq_ignore_ascii_case(prefix))
}

/// A recognized scheme in the address box wins over the protocol dropdown.
/// Pasting `http://127.0.0.1:7890` while SOCKS5 stays selected still persists HTTP.
///
/// `None` means the field is only a scheme (`http://`, `socks5://`, `socks5h://`,
/// or `socks://`). That text stays in the box and is not saved as an empty URL.
/// An empty field still returns an empty URL so the saved address can be cleared.
pub(crate) fn compose_manual_proxy_url(
    dropdown: ManualProxyProtocol,
    typed: &str,
) -> Option<(ManualProxyProtocol, String)> {
    let typed = typed.trim();
    if is_scheme_only(typed) {
        return None;
    }
    let (parsed, host) = manual_proxy_parts(typed);
    let protocol = if host.len() == typed.len() { dropdown } else { parsed };
    Some((protocol, manual_proxy_value(protocol, host)))
}

#[cfg(test)]
mod tests {
    use super::{ManualProxyProtocol, manual_proxy_parts, manual_proxy_value};

    #[test]
    fn protocol_and_address_round_trip_without_duplicate_prefixes() {
        assert_eq!(
            manual_proxy_parts("SOCKS5://127.0.0.1:1080"),
            (ManualProxyProtocol::Socks5, "127.0.0.1:1080")
        );
        assert_eq!(
            manual_proxy_parts("HTTP://proxy.lan:8080"),
            (ManualProxyProtocol::Http, "proxy.lan:8080")
        );
        assert_eq!(
            manual_proxy_parts("127.0.0.1:7890"),
            (ManualProxyProtocol::Socks5, "127.0.0.1:7890")
        );
        assert_eq!(
            manual_proxy_parts("socks5h://127.0.0.1:1080"),
            (ManualProxyProtocol::Socks5h, "127.0.0.1:1080")
        );
        assert_eq!(
            manual_proxy_parts("socks://127.0.0.1:1080"),
            (ManualProxyProtocol::Socks5, "127.0.0.1:1080")
        );
        assert_eq!(
            manual_proxy_value(ManualProxyProtocol::Socks5, "127.0.0.1:1080"),
            "socks5://127.0.0.1:1080"
        );
        assert_eq!(
            manual_proxy_value(ManualProxyProtocol::Socks5h, "127.0.0.1:1080"),
            "socks5h://127.0.0.1:1080"
        );
        assert_eq!(manual_proxy_value(ManualProxyProtocol::Http, ""), "");
    }

    #[test]
    fn typed_scheme_overrides_the_protocol_dropdown() {
        use super::compose_manual_proxy_url;
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Socks5, "http://127.0.0.1:7890")
                .expect("full URL");
        assert_eq!(protocol, ManualProxyProtocol::Http);
        assert_eq!(url, "http://127.0.0.1:7890");
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Http, "127.0.0.1:7890").expect("host");
        assert_eq!(protocol, ManualProxyProtocol::Http);
        assert_eq!(url, "http://127.0.0.1:7890");
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Http, "socks5://127.0.0.1:7890")
                .expect("full URL");
        assert_eq!(protocol, ManualProxyProtocol::Socks5);
        assert_eq!(url, "socks5://127.0.0.1:7890");
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Socks5, "socks5h://127.0.0.1:1080")
                .expect("full URL");
        assert_eq!(protocol, ManualProxyProtocol::Socks5h);
        assert_eq!(url, "socks5h://127.0.0.1:1080");
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Socks5h, "127.0.0.1:1080").expect("host");
        assert_eq!(protocol, ManualProxyProtocol::Socks5h);
        assert_eq!(url, "socks5h://127.0.0.1:1080");
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Http, "socks://127.0.0.1:1080")
                .expect("alias");
        assert_eq!(protocol, ManualProxyProtocol::Socks5);
        assert_eq!(url, "socks5://127.0.0.1:1080");
    }

    #[test]
    fn scheme_without_a_host_is_not_saved_as_an_empty_url() {
        use super::compose_manual_proxy_url;
        for typed in ["http://", "HTTP://", " socks5:// ", "socks5h://", "SOCKS://"] {
            assert_eq!(
                compose_manual_proxy_url(ManualProxyProtocol::Socks5, typed),
                None,
                "{typed}"
            );
        }
        let (protocol, url) =
            compose_manual_proxy_url(ManualProxyProtocol::Http, "  ").expect("empty field");
        assert_eq!(protocol, ManualProxyProtocol::Http);
        assert_eq!(url, "");
    }

    #[test]
    fn parts_accept_non_ascii_without_slicing_inside_utf8() {
        for value in ["127.0.0.1：", "：127.0.0.1", "127.0：0.1", "："] {
            assert_eq!(manual_proxy_parts(value), (ManualProxyProtocol::Socks5, value));
        }
    }
}
