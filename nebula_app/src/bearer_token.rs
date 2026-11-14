//! Bearer-token checks for the local loopback services.

/// Compares a presented token with the expected one without stopping at the first
/// differing byte, so response timing does not reveal a matching prefix. Only the
/// length can leak, and tokens have a fixed public length. An empty expected token
/// means the service has no credential and rejects every request.
pub(crate) fn matches(expected: &str, presented: Option<&str>) -> bool {
    let Some(presented) = presented else { return false };
    let (expected, presented) = (expected.as_bytes(), presented.as_bytes());
    if expected.is_empty() || expected.len() != presented.len() {
        return false;
    }
    let difference = expected.iter().zip(presented).fold(0u8, |acc, (a, b)| acc | (a ^ b));
    std::hint::black_box(difference) == 0
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
