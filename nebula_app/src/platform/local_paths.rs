//! Native host and drive observations used when resolving local file links.
//! URI parsing and the decision to open a link remain in the file-link layer.

/// Match the machine names supplied by the native terminal environment.
pub(crate) fn matches_hostname(host: &str) -> bool {
    #[cfg(windows)]
    if std::env::var("COMPUTERNAME").is_ok_and(|name| name.eq_ignore_ascii_case(host)) {
        return true;
    }
    #[cfg(unix)]
    {
        for variable in ["HOSTNAME", "HOST"] {
            if std::env::var(variable).is_ok_and(|name| name.eq_ignore_ascii_case(host)) {
                return true;
            }
        }
        if system_hostname().is_some_and(|name| name.eq_ignore_ascii_case(host)) {
            return true;
        }
    }
    false
}

/// Whether a Windows drive is mounted; Unix paths never imply a Windows drive.
pub(crate) fn drive_exists(letter: char) -> bool {
    #[cfg(windows)]
    {
        letter.is_ascii_alphabetic() && std::path::Path::new(&format!("{letter}:\\")).exists()
    }
    #[cfg(not(windows))]
    {
        let _ = letter;
        false
    }
}

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the writable buffer has the exact length passed to gethostname.
    // Decoding below also rejects an unterminated or invalid UTF-8 response.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return None;
    }
    hostname_from_buffer(&buffer).map(str::to_owned)
}

#[cfg(any(unix, test))]
fn hostname_from_buffer(buffer: &[u8]) -> Option<&str> {
    let end = buffer.iter().position(|&byte| byte == 0)?;
    let name = std::str::from_utf8(&buffer[..end]).ok()?;
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::hostname_from_buffer;

    #[test]
    fn hostname_decoding_stops_at_the_native_terminator() {
        assert_eq!(hostname_from_buffer(b"workstation\0unused"), Some("workstation"));
    }

    #[test]
    fn hostname_decoding_rejects_incomplete_or_invalid_native_data() {
        assert_eq!(hostname_from_buffer(b"unterminated"), None);
        assert_eq!(hostname_from_buffer(b"\xff\0"), None);
        assert_eq!(hostname_from_buffer(b"\0"), None);
        assert_eq!(hostname_from_buffer(b""), None);
    }
}
