//! Semantic results for the settings-page network connectivity test.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTestRoute {
    Direct,
    DirectAddress,
    CustomCommand,
    ProxyServer(String),
    SshJump(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTestFailure {
    LoadSettings(String),
    InvalidSettings(String),
    Timeout { seconds: u64 },
    SendRequest(String),
    ReadResponse(String),
    InvalidHttpStatusLine(String),
    HttpStatus { status: u16 },
    ProxyServer { server: String, error: String },
    CustomCommand(String),
    JumpTask(String),
    JumpResolve { target: String, error: String },
    JumpConnect { target: String, error: String },
    JumpChannel { target: String, error: String },
    Direct(String),
    Start(String),
    UnexpectedEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTestOutcome {
    Success(ProxyTestRoute),
    Failed(ProxyTestFailure),
}

impl ProxyTestOutcome {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyTestResult {
    pub request_id: u64,
    pub outcome: ProxyTestOutcome,
    pub elapsed_ms: u64,
}

/// Validated HTTP(S) target shared by the settings UI and the network worker.
#[derive(Debug, Clone)]
pub struct NetworkTestTarget {
    uri: ureq::http::Uri,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebsiteRegion {
    Domestic,
    International,
    Unknown,
}

impl Default for NetworkTestTarget {
    fn default() -> Self {
        Self::parse("http://example.com/").expect("built-in network test URL")
    }
}

impl NetworkTestTarget {
    pub fn parse(value: &str) -> Result<Self, ()> {
        let value = value.trim();
        if value.is_empty() || value.contains('#') {
            return Err(());
        }
        let value =
            if value.contains("://") { value.to_owned() } else { format!("https://{value}") };
        let uri = value.parse::<ureq::http::Uri>().map_err(|_| ())?;
        if !matches!(uri.scheme_str(), Some("http" | "https")) {
            return Err(());
        }
        let authority = uri.authority().ok_or(())?;
        if authority.as_str().contains('@') || authority.host().is_empty() {
            return Err(());
        }
        let suffix = authority.as_str().strip_prefix(authority.host()).ok_or(())?;
        if !suffix.is_empty() {
            let port = suffix.strip_prefix(':').ok_or(())?.parse::<u16>().map_err(|_| ())?;
            if port == 0 {
                return Err(());
            }
        }
        Ok(Self { uri })
    }

    pub fn url(&self) -> String {
        self.uri.to_string()
    }

    pub fn host(&self) -> &str {
        self.uri.host().expect("validated host").trim_start_matches('[').trim_end_matches(']')
    }

    pub fn port(&self) -> u16 {
        self.uri.port_u16().unwrap_or(if self.is_https() { 443 } else { 80 })
    }

    pub fn is_https(&self) -> bool {
        self.uri.scheme_str() == Some("https")
    }

    pub fn request(&self) -> String {
        let path = self.uri.path_and_query().map_or("/", |path| path.as_str());
        let authority = self.uri.authority().expect("validated authority");
        format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nUser-Agent: Pebrel-Network-Test\r\n\r\n"
        )
    }

    pub fn region(&self) -> WebsiteRegion {
        let host = self.host().trim_end_matches('.').to_ascii_lowercase();
        let matches = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
        // ponytail: domain rules do not locate CDN servers; use a maintained geo database if required.
        let domestic = [
            "baidu.com",
            "qq.com",
            "bilibili.com",
            "taobao.com",
            "jd.com",
            "163.com",
            "aliyun.com",
            "gitee.com",
        ];
        let international = [
            "github.com",
            "githubusercontent.com",
            "google.com",
            "youtube.com",
            "wikipedia.org",
            "example.com",
            "cloudflare.com",
        ];
        if matches("cn") || domestic.iter().any(|domain| matches(domain)) {
            WebsiteRegion::Domestic
        } else if international.iter().any(|domain| matches(domain)) {
            WebsiteRegion::International
        } else {
            WebsiteRegion::Unknown
        }
    }
}

/// Only a complete, successful HTTP status line proves this request succeeded.
pub fn check_http_response(response: &str) -> Result<(), ProxyTestFailure> {
    let invalid = || ProxyTestFailure::InvalidHttpStatusLine(response.to_owned());
    let (line, _) = response.split_once("\r\n").ok_or_else(invalid)?;
    let mut parts = line.split_whitespace();
    if !matches!(parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(invalid());
    }
    let code = parts.next().filter(|code| code.len() == 3).ok_or_else(invalid)?;
    let status = code.parse::<u16>().map_err(|_| invalid())?;
    if !(200..300).contains(&status) {
        return Err(ProxyTestFailure::HttpStatus { status });
    }
    Ok(())
}

#[cfg(test)]
mod target_tests {
    use super::{NetworkTestTarget, ProxyTestFailure, WebsiteRegion, check_http_response};

    #[test]
    fn targets_preserve_scheme_path_port_and_classify_domain_boundaries() {
        let target = NetworkTestTarget::parse("https://GitHub.com:8443/status?q=1").unwrap();
        assert_eq!(target.port(), 8443);
        assert!(target.is_https());
        assert!(
            target.request().starts_with("GET /status?q=1 HTTP/1.1\r\nHost: GitHub.com:8443\r\n")
        );
        assert_eq!(target.region(), WebsiteRegion::International);
        assert_eq!(
            NetworkTestTarget::parse("www.baidu.com").unwrap().region(),
            WebsiteRegion::Domestic
        );
        assert_eq!(
            NetworkTestTarget::parse("https://example.cn").unwrap().region(),
            WebsiteRegion::Domestic
        );
        assert_eq!(
            NetworkTestTarget::parse("https://baidu.com.evil.org").unwrap().region(),
            WebsiteRegion::Unknown
        );
        let ipv6 = NetworkTestTarget::parse("http://[::1]:8080/").unwrap();
        assert_eq!((ipv6.host(), ipv6.port()), ("::1", 8080));
        for value in [
            "",
            "ftp://example.com",
            "https://user:pass@example.com",
            "http://example.com:0",
            "http://example.com:bad",
            "http://example.com:65536",
            "http://example.com/\r\nInjected: yes",
            "https://example.com/#fragment",
        ] {
            assert!(NetworkTestTarget::parse(value).is_err(), "{value}");
        }
    }

    #[test]
    fn network_test_requires_a_complete_successful_http_response() {
        for response in ["HTTP/1.1 200 OK\r\n", "HTTP/1.0 204 No Content\r\n"] {
            assert_eq!(check_http_response(response), Ok(()));
        }
        for status in [301, 403, 500] {
            assert_eq!(
                check_http_response(&format!("HTTP/1.1 {status} Response\r\n")),
                Err(ProxyTestFailure::HttpStatus { status }),
            );
        }
        for response in ["FAKE 200 OK\r\n", "HTTP/1.1 200", "HTTP/1.1 0200 OK\r\n"] {
            assert!(matches!(
                check_http_response(response),
                Err(ProxyTestFailure::InvalidHttpStatusLine(_)),
            ));
        }
    }
}
