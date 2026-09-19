//! 更新检查和下载共用的 HTTP 客户端与代理解析。
//!
//! 1. Windows 系统代理：读注册表并自行解析，不交给 ureq 的
//!    `win-system-proxy`——它把 `socks=` 条目也拼成 `http://`，会对 SOCKS
//!    端口说 HTTP；
//! 2. 代理环境变量（`ALL_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY`，含 `NO_PROXY`）：
//!    复用 ureq 的 `Proxy::try_from_env()`。
//!
//! 应用暂无更新器专用的手动代理设置，也不借用终端或 SSH 的代理偏好。
//! 注册表读取只发生在后台请求开始时，不改变其他请求。
//! 排除规则交给 ureq 的同一实现，重定向到下载 CDN 时也会重新判断。

use std::time::Duration;

use ureq::{Proxy, ProxyProtocol};

#[cfg(test)]
#[path = "update_proxy/test_support.rs"]
pub(crate) mod test_support;

/// 解析更新下载应使用的代理；没有可用代理时返回 `None`（直连）。
pub(crate) fn resolve(target_url: &str) -> Option<Proxy> {
    resolve_with_system(raw_system_proxy(target_url), Proxy::try_from_env)
}

fn resolve_with_system(
    system: Option<(String, Vec<String>)>,
    environment: impl FnOnce() -> Option<Proxy>,
) -> Option<Proxy> {
    system.and_then(|(url, no_proxy)| proxy_from_url(&url, &no_proxy).ok()).or_else(environment)
}

/// 每项更新操作拥有独立的有界客户端，复用现有后台执行器，不新增常驻服务。
pub(crate) fn agent(target_url: &str, timeout: Duration) -> ureq::Agent {
    ureq::config::Config::builder()
        .proxy(resolve(target_url))
        .timeout_global(Some(timeout))
        // 连接限时 30 秒，正文保留调用方的总时限。
        // ureq 3.3 的 recv_response 时限也会延续到正文，不能把大文件截在 30 秒。
        .timeout_connect(Some(Duration::from_secs(30)))
        .https_only(true)
        .build()
        .new_agent()
}

#[cfg(windows)]
fn raw_system_proxy(target_url: &str) -> Option<(String, Vec<String>)> {
    const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(INTERNET_SETTINGS)
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled != 1 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    let overrides: String = key.get_value("ProxyOverride").unwrap_or_default();
    let no_proxy: Vec<String> = overrides
        .split(';')
        .map(str::trim)
        // `<local>` 表示"不带点的主机名"，更新目标都是公网域名，无法表达。
        .filter(|entry| !entry.is_empty() && *entry != "<local>")
        .map(str::to_owned)
        .collect();
    let url = parse_windows_proxy_server(&server, target_scheme(target_url))?;
    Some((url, no_proxy))
}

#[cfg(not(windows))]
fn raw_system_proxy(_target_url: &str) -> Option<(String, Vec<String>)> {
    None
}

/// 把 Windows `ProxyServer` 注册表值解析成代理 URL。刻意放在平台模块外，让
/// 解析逻辑在任何主机上都编译并被测试。
/// - `127.0.0.1:7890` -> `http://127.0.0.1:7890`
/// - `http=127.0.0.1:7890;https=127.0.0.1:7891` 按目标协议取，缺协议回退 http
/// - `socks=127.0.0.1:1080` -> `socks5://127.0.0.1:1080`
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_windows_proxy_server(server: &str, target_scheme: &str) -> Option<String> {
    if !server.contains('=') {
        let server = server.trim();
        return (!server.is_empty()).then(|| {
            if server.contains("://") { server.to_owned() } else { format!("http://{server}") }
        });
    }

    let mut by_protocol = std::collections::HashMap::new();
    for part in server.split(';') {
        // Windows 会写入 `ftp=` 等额外条目，末尾也常有 `;`。
        // 单个无法解析的片段不能丢弃整个代理设置。
        let Some((protocol, address)) = part.split_once('=') else {
            continue;
        };
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        by_protocol.insert(protocol.trim().to_ascii_lowercase(), address.to_owned());
    }

    if let Some(address) = by_protocol.get(target_scheme).or_else(|| by_protocol.get("http")) {
        return Some(if address.contains("://") {
            address.clone()
        } else {
            format!("http://{address}")
        });
    }
    by_protocol.get("socks").map(|address| {
        if address.contains("://") { address.clone() } else { format!("socks5://{address}") }
    })
}

/// 由代理 URL 与 no-proxy 列表构造 `ureq::Proxy`。不走 `Proxy::new`，因为它
/// 没有附加 no-proxy 列表的入口；带认证信息时按 ureq 的约定原样传递。
fn proxy_from_url(url: &str, no_proxy: &[String]) -> Result<Proxy, ureq::Error> {
    let uri: ureq::http::Uri = url.parse().map_err(|_| ureq::Error::InvalidProxyUrl)?;
    let authority = uri.authority().ok_or(ureq::Error::InvalidProxyUrl)?;

    let protocol = ProxyProtocol::try_from(uri.scheme_str().unwrap_or("http"))?;
    let mut builder = Proxy::builder(protocol).host(authority.host());
    // 不设置端口时让 ureq 使用协议默认端口。
    if let Some(port) = authority.port_u16() {
        builder = builder.port(port);
    }

    let (username, password) = match authority.as_str().split_once('@') {
        Some((userinfo, _)) => {
            let mut parts = userinfo.splitn(2, ':');
            (parts.next().unwrap_or(""), parts.next())
        },
        None => ("", None),
    };
    if !username.is_empty() {
        builder = builder.username(username);
        if let Some(password) = password {
            builder = builder.password(password);
        }
    }

    for entry in no_proxy {
        builder = builder.no_proxy(entry.as_str());
    }

    builder.build()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn target_scheme(target_url: &str) -> &str {
    target_url.split_once("://").map_or("http", |(scheme, _)| scheme)
}

#[cfg(test)]
mod tests {
    use super::{parse_windows_proxy_server, proxy_from_url, resolve_with_system, target_scheme};
    use ureq::{Proxy, ProxyProtocol};

    fn windows_proxy(server: &str, target: &str) -> Option<String> {
        parse_windows_proxy_server(server, target)
    }

    #[test]
    fn bare_address_defaults_to_http() {
        assert_eq!(
            windows_proxy("127.0.0.1:7890", "https"),
            Some("http://127.0.0.1:7890".to_owned())
        );
    }

    #[test]
    fn per_protocol_entries_pick_the_target_scheme() {
        assert_eq!(
            windows_proxy("http=127.0.0.1:7890;https=127.0.0.1:7891", "https"),
            Some("http://127.0.0.1:7891".to_owned())
        );
        // 未知协议回退到 http 条目。
        assert_eq!(
            windows_proxy("http=127.0.0.1:7890;https=127.0.0.1:7891", "ftp"),
            Some("http://127.0.0.1:7890".to_owned())
        );
    }

    #[test]
    fn socks_only_entries_use_socks5() {
        assert_eq!(
            windows_proxy("socks=127.0.0.1:1080", "https"),
            Some("socks5://127.0.0.1:1080".to_owned())
        );
    }

    #[test]
    fn stray_parts_do_not_discard_the_setting() {
        assert_eq!(
            windows_proxy("http=127.0.0.1:7890;", "http"),
            Some("http://127.0.0.1:7890".to_owned())
        );
        assert_eq!(
            windows_proxy("ftp=1.2.3.4:21;http=127.0.0.1:7890;<local>", "http"),
            Some("http://127.0.0.1:7890".to_owned())
        );
    }

    #[test]
    fn empty_or_valueless_settings_resolve_to_nothing() {
        assert_eq!(windows_proxy("", "http"), None);
        assert_eq!(windows_proxy("   ", "http"), None);
        assert_eq!(windows_proxy("http=", "http"), None);
        assert_eq!(windows_proxy("ftp=1.2.3.4:21", "http"), None);
    }

    #[test]
    fn target_scheme_reads_the_scheme_or_assumes_http() {
        assert_eq!(target_scheme("https://github.com/x"), "https");
        assert_eq!(target_scheme("http://example.com"), "http");
        assert_eq!(target_scheme("github.com"), "http");
    }

    #[test]
    fn proxy_urls_keep_scheme_port_and_credentials() {
        let proxy = proxy_from_url("http://127.0.0.1:7890", &[]).expect("parses");
        assert_eq!(proxy.host(), "127.0.0.1");
        assert_eq!(proxy.port(), 7890);

        let socks = proxy_from_url("socks5://127.0.0.1:1080", &[]).expect("parses");
        assert_eq!(socks.port(), 1080);
        assert_eq!(socks.protocol(), ProxyProtocol::Socks5);

        // 缺省端口交给 ureq 按协议补齐。
        assert_eq!(proxy_from_url("http://proxy.local", &[]).expect("parses").port(), 80);
        let auth = proxy_from_url("http://user:secret@127.0.0.1:7890", &[]).expect("parses");
        assert_eq!(auth.username(), Some("user"));
        assert_eq!(auth.password(), Some("secret"));
    }

    #[test]
    fn no_proxy_entries_use_the_actual_transport_matcher() {
        let proxy = proxy_from_url(
            "http://127.0.0.1:7890",
            &["github.com".into(), "*.corp.local".into(), "*zhihu.com".into(), "192.168.*".into()],
        )
        .unwrap();
        for host in ["github.com", "GHUB.CORP.LOCAL", "www.zhihu.com", "zhihu.com", "192.168.1.1"] {
            assert!(proxy.is_no_proxy(&format!("https://{host}/").parse().unwrap()), "{host}");
        }
        for host in
            ["api.github.com", "notgithub.com", "corp.local", "objects.githubusercontent.com"]
        {
            assert!(!proxy.is_no_proxy(&format!("https://{host}/").parse().unwrap()), "{host}");
        }
    }

    #[test]
    fn star_and_local_entries_are_handled() {
        let proxy = proxy_from_url("http://proxy.local", &["*".into()]).unwrap();
        assert!(proxy.is_no_proxy(&"https://anything.example".parse().unwrap()));
        let proxy = proxy_from_url("http://proxy.local", &["example".into()]).unwrap();
        assert!(proxy.is_no_proxy(&"https://example".parse().unwrap()));
        assert!(!proxy.is_no_proxy(&"https://github.com".parse().unwrap()));
    }

    #[test]
    fn exclusions_apply_independently_to_api_asset_and_redirect_hosts() {
        let proxy = proxy_from_url(
            "http://proxy.local",
            &["github.com".into(), "*.githubusercontent.com".into()],
        )
        .unwrap();
        assert!(!proxy.is_no_proxy(
            &"https://api.github.com/repos/Kuddev/pebrel/releases/latest".parse().unwrap()
        ));
        assert!(proxy.is_no_proxy(
            &"https://github.com:443/Kuddev/pebrel/releases/download/v1.7.0/x.exe".parse().unwrap()
        ));
        assert!(
            proxy.is_no_proxy(
                &"https://release-assets.githubusercontent.com/x.exe".parse().unwrap()
            )
        );
    }

    #[test]
    fn protocol_entries_preserve_explicit_schemes_and_authentication() {
        for (server, expected) in [
            ("https=http://user:secret@proxy.local:7890", "http://user:secret@proxy.local:7890"),
            ("HTTPS=socks5h://[::1]:1080;http=proxy.local", "socks5h://[::1]:1080"),
            ("socks=socks5h://proxy.local:1080;", "socks5h://proxy.local:1080"),
        ] {
            let url = windows_proxy(server, "https").unwrap();
            assert_eq!(url, expected);
            assert!(proxy_from_url(&url, &[]).is_ok());
        }
    }

    #[test]
    fn system_proxy_and_its_exclusions_win_over_environment() {
        let proxy = resolve_with_system(
            Some(("http://system.local:7890".into(), vec!["github.com".into()])),
            || panic!("valid system settings must not consult environment"),
        )
        .unwrap();
        assert_eq!(proxy.host(), "system.local");
        assert!(proxy.is_no_proxy(&"https://github.com".parse().unwrap()));
    }

    #[test]
    fn missing_or_invalid_system_proxy_falls_back_to_environment_then_direct() {
        for system in [None, Some(("ftp://proxy.local".into(), vec![]))] {
            let proxy =
                resolve_with_system(system.clone(), || Proxy::new("http://env.local:8080").ok())
                    .unwrap();
            assert_eq!(proxy.host(), "env.local");
            assert!(resolve_with_system(system, || None).is_none());
        }
    }

    #[test]
    fn socks_proxy_uses_a_real_socks_handshake_and_remote_dns() {
        use super::test_support::{Server, response};
        let proxy = Server::start(vec![response("200 OK", "", "via SOCKS5")]);
        let body = proxy
            .socks_agent()
            .get("http://cdn.update.invalid/asset")
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        assert_eq!(body, "via SOCKS5");
        assert_eq!(proxy.finish()[0].0, "SOCKS5 cdn.update.invalid:80");
    }

    #[test]
    fn download_client_preserves_the_body_budget_and_requires_https() {
        let budget = std::time::Duration::from_secs(15 * 60);
        let client = super::agent("https://github.com", budget);
        let timeouts = client.config().timeouts();
        assert_eq!(timeouts.global, Some(budget));
        // A recv_response deadline also caps a streamed body in ureq 3.3.
        assert_eq!(timeouts.recv_response, None);
        assert_eq!(timeouts.recv_body, None);
        assert!(client.config().https_only());
    }

    #[test]
    fn environment_proxy_precedence_and_no_proxy_are_isolated_from_the_test_process() {
        const CASE: &str = "PEBREL_UPDATE_PROXY_TEST_CASE";
        if let Ok(case) = std::env::var(CASE) {
            let proxy = resolve_with_system(None, Proxy::try_from_env);
            if case == "direct" {
                assert!(proxy.is_none());
            } else {
                let proxy = proxy.unwrap();
                assert_eq!(proxy.host(), format!("{case}.invalid"));
                assert!(proxy.is_no_proxy(&"https://github.com".parse().unwrap()));
                assert!(!proxy.is_no_proxy(&"https://api.github.com".parse().unwrap()));
            }
            return;
        }
        for (case, vars) in [
            (
                "all",
                vec![
                    ("ALL_PROXY", "socks5h://all.invalid:1080"),
                    ("HTTPS_PROXY", "http://https.invalid:8080"),
                    ("HTTP_PROXY", "http://http.invalid:8080"),
                ],
            ),
            (
                "https",
                vec![
                    ("ALL_PROXY", "ftp://invalid.invalid"),
                    ("HTTPS_PROXY", "http://https.invalid:8080"),
                ],
            ),
            ("lower", vec![("https_proxy", "http://lower.invalid:8080")]),
            ("http", vec![("HTTP_PROXY", "http://http.invalid:8080")]),
            ("direct", vec![]),
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "update_proxy::tests::environment_proxy_precedence_and_no_proxy_are_isolated_from_the_test_process"]);
            for name in [
                "ALL_PROXY",
                "all_proxy",
                "HTTPS_PROXY",
                "https_proxy",
                "HTTP_PROXY",
                "http_proxy",
                "NO_PROXY",
                "no_proxy",
            ] {
                command.env_remove(name);
            }
            let output =
                command.env(CASE, case).env("NO_PROXY", "github.com").envs(vars).output().unwrap();
            assert!(
                output.status.success(),
                "{case}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
