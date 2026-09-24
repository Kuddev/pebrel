//! GitHub's public latest-release redirect is independent of the REST API quota.
//! The redirect proves a version; the official SHA256SUMS asset can then supply
//! verified package metadata. If absent, retain manual download only.

use std::time::Duration;

use ureq::ResponseExt as _;

use super::LatestRelease;
use crate::i18n::{LanguagePreference, Message};

const LATEST_PAGE: &str = "https://github.com/Kuddev/pebrel/releases/latest";
const TAG_PREFIX: &str = "https://github.com/Kuddev/pebrel/releases/tag/";

pub(super) fn fetch_latest(status: u16) -> Result<LatestRelease, String> {
    log::info!("update-check: API returned HTTP {status}; checking official latest-release page");
    // Resolve proxy settings for github.com independently of api.github.com.
    let agent = crate::update_proxy::agent(LATEST_PAGE, Duration::from_secs(10));
    let language =
        LanguagePreference::from(nebula_settings::RuntimeSettings::load().language).resolved();
    let uri = redirected_uri(&agent, LATEST_PAGE).map_err(|error| {
        language.format(
            Message::UpdateCheckFallbackFailed,
            &[("status", &status.to_string()), ("error", &error.to_string())],
        )
    })?;
    let mut release = release_from_uri(&uri)
        .ok_or_else(|| language.text(Message::UpdateCheckUnrecognizedRelease).to_owned())?;
    // The official manifest restores verified download metadata during API limits.
    // If it is unavailable, version discovery still works with manual download.
    let url = format!(
        "https://github.com/Kuddev/pebrel/releases/download/v{}/SHA256SUMS",
        release.version
    );
    let agent = crate::update_proxy::agent(&url, Duration::from_secs(10));
    if let Ok(mut response) = agent.get(&url).header("User-Agent", "pebrel").call()
        && let Ok(text) = response.body_mut().with_config().limit(64 * 1024).read_to_string()
    {
        release.asset = super::assets::from_checksums(&release.version, &text);
    }
    Ok(release)
}

fn redirected_uri(agent: &ureq::Agent, url: &str) -> Result<String, ureq::Error> {
    let response = agent
        .get(url)
        .header("User-Agent", "pebrel")
        .header("Accept", "text/html")
        .config()
        .max_redirects(5)
        .build()
        .call()?;
    // The final response URI is supplied by the HTTP client, not by untrusted
    // markup. No HTML body needs to be downloaded or parsed.
    Ok(response.get_uri().to_string())
}

fn release_from_uri(uri: &str) -> Option<LatestRelease> {
    let tag = uri.strip_prefix(TAG_PREFIX)?;
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    // Official stable releases use major.minor.patch. Reject login pages,
    // arbitrary tags, extra path/query/fragment components and preview tags.
    let components: Vec<_> = version.split('.').collect();
    if components.len() != 3
        || components.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.parse::<u64>().is_err()
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return None;
    }
    Some(LatestRelease { version: version.to_owned(), asset: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_proxy::test_support::{Server, response};

    #[test]
    fn rate_limited_api_falls_back_and_discovers_an_upgrade_without_install_authority() {
        for status in ["403 Forbidden", "429 Too Many Requests"] {
            let server = Server::start(vec![response(status, "", "API rate limit exceeded")]);
            let result = super::super::fetch_release_with_fallback(
                &server.agent(&[]),
                "http://api.update.invalid/latest",
                |code| {
                    assert!(matches!(code, 403 | 429));
                    Ok(release_from_uri(&format!("{TAG_PREFIX}v1.9.0")).unwrap())
                },
            )
            .unwrap();
            assert!(super::super::is_newer(&result.version, "1.8.2"));
            assert!(!super::super::is_newer(&result.version, "1.9.0"));
            assert!(result.asset.is_none(), "a version redirect grants no install authority");
            assert_eq!(server.finish().len(), 1);
        }
    }

    #[test]
    fn successful_api_and_other_errors_never_call_the_fallback() {
        for (status, body, succeeds) in [
            ("200 OK", r#"{"tag_name":"v1.9.0","assets":[]}"#, true),
            ("200 OK", "not JSON", false),
            ("404 Not Found", "missing release", false),
            ("500 Internal Server Error", "server error", false),
        ] {
            let server = Server::start(vec![response(status, "", body)]);
            let result = super::super::fetch_release_with_fallback(
                &server.agent(&[]),
                "http://api.update.invalid/latest",
                |_| panic!("fallback is only for access/rate limiting"),
            );
            assert_eq!(result.is_ok(), succeeds);
            server.finish();
        }
    }

    #[test]
    fn failed_fallback_stays_an_error_instead_of_claiming_up_to_date() {
        let server = Server::start(vec![response("403 Forbidden", "", "rate limited")]);
        let result = super::super::fetch_release_with_fallback(
            &server.agent(&[]),
            "http://api.update.invalid/latest",
            |_| Err("release page unavailable".into()),
        );
        assert_eq!(result.unwrap_err(), "release page unavailable");
        server.finish();
    }

    #[test]
    fn redirect_transport_uses_the_final_location_without_parsing_html() {
        // The local CONNECT fixture uses HTTP; production agent requires HTTPS.
        let destination = "http://github.com/Kuddev/pebrel/releases/tag/v1.9.0";
        let server = Server::start(vec![
            response("302 Found", &format!("Location: {destination}\r\n"), ""),
            response("200 OK", "Content-Type: text/html\r\n", "<html>no metadata</html>"),
        ]);
        let uri =
            redirected_uri(&server.agent(&[]), "http://github.com/Kuddev/pebrel/releases/latest")
                .unwrap();
        assert_eq!(uri, destination);
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].1.contains("/releases/latest"));
        assert!(requests[1].1.contains("/releases/tag/v1.9.0"));
    }

    #[test]
    fn only_exact_official_stable_tag_locations_are_accepted() {
        for tag in ["v1.9.0", "V1.9.0", "1.9.0"] {
            assert_eq!(release_from_uri(&format!("{TAG_PREFIX}{tag}")).unwrap().version, "1.9.0");
        }
        for uri in [
            LATEST_PAGE,
            "https://github.com/login",
            "https://github.com/Other/pebrel/releases/tag/v99.0.0",
            "http://github.com/Kuddev/pebrel/releases/tag/v1.9.0",
            "https://github.com.evil.invalid/Kuddev/pebrel/releases/tag/v1.9.0",
            "https://github.com/Kuddev/pebrel/releases/tag/v1.9.0?foo=bar",
            "https://github.com/Kuddev/pebrel/releases/tag/v1.9.0#fragment",
            "https://github.com/Kuddev/pebrel/releases/tag/v1.9.0/extra",
            "https://github.com/Kuddev/pebrel/releases/tag/v1.9.0-rc1",
            "https://github.com/Kuddev/pebrel/releases/tag/relay-abcd",
            "https://github.com/Kuddev/pebrel/releases/tag/v1..0",
            "https://github.com/Kuddev/pebrel/releases/tag/v01.9.0",
            "https://github.com/Kuddev/pebrel/releases/tag/v18446744073709551616.0.0",
        ] {
            assert!(release_from_uri(uri).is_none(), "{uri}");
        }
    }

    #[test]
    #[ignore = "contacts public GitHub; run explicitly for native network acceptance"]
    fn live_update_check_survives_api_rate_limiting() {
        let release = super::super::check_now().expect("live public update check");
        println!(
            "current={} latest={} update_available={}",
            release.current, release.latest, release.update_available
        );
        let fallback = fetch_latest(403).expect("live official release fallback");
        assert_eq!(release.latest, fallback.version);
    }
}
