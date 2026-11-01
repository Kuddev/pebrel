//! A GUI launch may retry discovery, but never an ambiguously delivered mutation.
use super::*;

enum Outcome {
    Unavailable,
    Confirmed,
    Unconfirmed,
}

/// `true` means this launch is consumed, including a reported unknown outcome.
/// It does not claim that the resident finished creating the requested window.
pub(super) fn open(method: &str, params: Value) -> bool {
    // Pin both operations to one resident; rediscovery between ATTACH and create
    // could otherwise send the mutation to a different instance.
    let Some(endpoint) = server::attach_endpoint() else { return false };
    match exchange(&endpoint, method, params, IO_TIMEOUT) {
        Outcome::Unavailable => false,
        Outcome::Confirmed => true,
        Outcome::Unconfirmed => {
            let language = crate::i18n::LanguagePreference::from(
                nebula_settings::RuntimeSettings::load().language,
            )
            .resolved();
            let message = language.text(crate::i18n::Message::StartupHandoverUnconfirmed);
            log::warn!("{message}");
            eprintln!("{message}");
            crate::platform::startup::report_error(&message, true);
            true
        },
    }
}

fn exchange(endpoint: &Endpoint, method: &str, params: Value, timeout: Duration) -> Outcome {
    let request = ApiRequest::new(endpoint.token.clone(), method, params);
    let Ok(mut bytes) = serde_json::to_vec(&request) else { return Outcome::Unconfirmed };
    bytes.push(b'\n');
    let Ok(mut stream) = TcpStream::connect_timeout(&endpoint_addr(endpoint), CONNECT_TIMEOUT)
    else {
        return Outcome::Unavailable;
    };
    // All failures before the first write are safe to retry. After writing may
    // have started, neither a timeout, EOF, rejection nor malformed reply proves
    // that tab.new/window.create had no effect. Never replay or open a fallback.
    if stream.set_read_timeout(Some(timeout)).is_err()
        || stream.set_write_timeout(Some(timeout)).is_err()
    {
        return Outcome::Unavailable;
    }
    if stream.write_all(&bytes).is_err() {
        return Outcome::Unconfirmed;
    }
    // JSONL is already framed. A half-close is unnecessary and can fail after
    // a fast resident has replied, before we read its buffered response.
    let mut line = Vec::new();
    if BufReader::new(stream)
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_until(b'\n', &mut line)
        .is_err()
        || line.len() > MAX_REQUEST_BYTES
        || line.last() != Some(&b'\n')
    {
        return Outcome::Unconfirmed;
    }
    match cli::decode_plugin_response(&request, &line) {
        Ok(response) if response.ok => Outcome::Confirmed,
        _ => Outcome::Unconfirmed,
    }
}

#[cfg(test)]
#[path = "handover_tests.rs"]
mod tests;
