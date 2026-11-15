//! Version-specific release pages; cached packages never link to a newer release.

const RELEASES: &str = "https://github.com/Kuddev/pebrel/releases/";

pub(crate) fn from_tag(tag: &str) -> Option<String> {
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    if version.is_empty()
        || !version.as_bytes()[0].is_ascii_digit()
        || tag.contains("..")
        || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return None;
    }
    Some(format!("{RELEASES}tag/{tag}"))
}

pub(crate) fn from_download(url: &str, version: &str) -> Option<String> {
    let path = url.strip_prefix(RELEASES)?.strip_prefix("download/")?;
    let (tag, _) = path.split_once('/')?;
    if tag.strip_prefix(['v', 'V']).unwrap_or(tag) != version {
        return None;
    }
    from_tag(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_pages_preserve_the_actual_tag() {
        assert_eq!(
            from_tag("V2.5.0").as_deref(),
            Some("https://github.com/Kuddev/pebrel/releases/tag/V2.5.0")
        );
        assert_eq!(
            from_tag("2.5.0").as_deref(),
            Some("https://github.com/Kuddev/pebrel/releases/tag/2.5.0")
        );
        assert_eq!(from_tag("../../latest"), None);
        assert_eq!(from_tag(""), None);
    }

    #[test]
    fn cached_package_link_is_pinned_and_must_match_its_version() {
        let url = "https://github.com/Kuddev/pebrel/releases/download/v2.5.0/Pebrel-setup.exe";
        assert_eq!(
            from_download(url, "2.5.0").as_deref(),
            Some("https://github.com/Kuddev/pebrel/releases/tag/v2.5.0")
        );
        assert_eq!(from_download(url, "2.6.0"), None);
        assert_eq!(
            from_download("https://elsewhere.example/releases/download/v2.5.0/file", "2.5.0"),
            None
        );
        assert_eq!(from_download("", "2.5.0"), None);
    }
}
