//! Explicit native acceptance transport. Official asset names, URLs and digest
//! validation still run; only HTTP transport uses a local test server. A matching
//! current version can be reinstalled to rehearse without changing release tags.

#[cfg(not(debug_assertions))]
compile_error!("update-test-source is for debug acceptance only; never enable it in a release");

/// A test needs both an isolated settings override and an explicit loopback
/// origin. Normal debug builds and all release builds have no source override.
pub(crate) fn origin() -> Result<Option<String>, String> {
    let Some(origin) = std::env::var_os("PEBREL_TEST_UPDATE_ORIGIN") else {
        return Ok(None);
    };
    if std::env::var_os("PEBREL_CONFIG_DIR").is_none_or(|path| path.is_empty()) {
        return Err("An updater rehearsal requires an isolated PEBREL_CONFIG_DIR".into());
    }
    let origin = origin.to_str().ok_or("The test update origin is not UTF-8")?;
    validate_origin(origin)?;
    Ok(Some(origin.to_owned()))
}

fn validate_origin(origin: &str) -> Result<(), String> {
    let valid = origin
        .strip_prefix("http://127.0.0.1:")
        .filter(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(|port| port != 0);
    if valid {
        Ok(())
    } else {
        Err("The test update origin must be http://127.0.0.1:<port>".into())
    }
}

pub(crate) fn agent(timeout: std::time::Duration) -> ureq::Agent {
    ureq::config::Config::builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(Some(timeout))
        .build()
        .new_agent()
}

#[cfg(test)]
mod tests {
    use super::validate_origin;

    #[test]
    fn transport_is_limited_to_an_explicit_loopback_port() {
        assert!(validate_origin("http://127.0.0.1:45678").is_ok());
        for invalid in [
            "https://example.com",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:80@evil.example",
            "http://127.0.0.1:80/path",
            "http://127.0.0.1.evil.example:80",
            "http://localhost:80",
        ] {
            assert!(validate_origin(invalid).is_err(), "{invalid}");
        }
    }
}
