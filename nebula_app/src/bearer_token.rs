//! Bearer-token generation and verification for process bridges.

use subtle::ConstantTimeEq as _;

/// Read 128 bits from the OS and encode the token for the existing wire format.
pub(crate) fn generate() -> std::io::Result<String> {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut token = String::with_capacity(32);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

/// Compares a presented token with the expected one through `subtle`, which does not
/// branch on byte contents, so response timing does not reveal a matching prefix.
/// Only the length can leak, and tokens have a fixed public length. An empty expected
/// token means the service has no credential and rejects every request.
pub(crate) fn matches(expected: &str, presented: Option<&str>) -> bool {
    let Some(presented) = presented else { return false };
    let (expected, presented) = (expected.as_bytes(), presented.as_bytes());
    if expected.is_empty() || expected.len() != presented.len() {
        return false;
    }
    expected.ct_eq(presented).into()
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn accepts_only_the_exact_non_empty_token() {
        let token = "a".repeat(32);
        let token = token.as_str();
        assert!(matches(token, Some(token)));
        assert!(!matches(token, None));
        assert!(!matches(token, Some("")));
        assert!(!matches(token, Some(&"a".repeat(31))));
        assert!(!matches(token, Some(&"a".repeat(33))));
        assert!(!matches(token, Some(&format!("b{}", "a".repeat(31)))));
        assert!(!matches(token, Some(&format!("{}b", "a".repeat(31)))));
        assert!(!matches("", Some("")));
        assert!(!matches("", None));
    }
}
