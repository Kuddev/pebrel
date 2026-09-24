//! Exact package names shared by release discovery and download validation.
use super::{GitHubReleaseAsset, UpdateAsset, checksum_from_release_body, normalize_sha256};
use crate::platform::Platform;

pub(crate) fn macos_names(version: &str, architecture: &str) -> Vec<String> {
    let architecture = match architecture {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return Vec::new(),
    };
    vec![
        format!("Pebrel-v{version}-macos-{architecture}.dmg"),
        format!("Pebrel-v{version}-macos-{architecture}-preview.dmg"),
    ]
}

pub(crate) fn native_names(version: &str) -> Vec<String> {
    match Platform::current() {
        Platform::MacOS => macos_names(version, std::env::consts::ARCH),
        Platform::Windows if std::env::consts::ARCH == "x86_64" => {
            super::windows_x64_installer_names(version).to_vec()
        },
        _ => Vec::new(),
    }
}

pub(super) fn select(
    version: &str,
    body: &str,
    assets: &[GitHubReleaseAsset],
    names: &[String],
) -> Option<UpdateAsset> {
    let asset = names.iter().find_map(|name| assets.iter().find(|asset| asset.name == *name))?;
    Some(UpdateAsset {
        version: version.into(),
        name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
        size: (asset.size > 0).then_some(asset.size),
        sha256: asset
            .digest
            .as_deref()
            .and_then(normalize_sha256)
            .or_else(|| checksum_from_release_body(body, &asset.name)),
    })
}

pub(super) fn from_checksums(version: &str, text: &str) -> Option<UpdateAsset> {
    native_names(version).iter().find_map(|name| {
        let hashes: Vec<_> = text
            .lines()
            .filter_map(|line| {
                let (hash, file) = line.split_once(char::is_whitespace)?;
                (file.trim().trim_start_matches('*') == name)
                    .then(|| normalize_sha256(&format!("sha256:{hash}")))
                    .flatten()
            })
            .collect();
        if hashes.len() != 1 {
            return None;
        }
        Some(UpdateAsset {
            version: version.into(),
            name: name.clone(),
            download_url: format!(
                "https://github.com/Kuddev/pebrel/releases/download/v{version}/{name}"
            ),
            size: None,
            sha256: hashes.into_iter().next(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn macos_architectures_and_preference_are_exact() {
        assert!(macos_names("1.9.0", "unknown").is_empty());
        for (arch, expected) in [("aarch64", "arm64"), ("x86_64", "x64")] {
            let names = macos_names("1.9.0", arch);
            assert_eq!(names[0], format!("Pebrel-v1.9.0-macos-{expected}.dmg"));
            assert_eq!(names[1], format!("Pebrel-v1.9.0-macos-{expected}-preview.dmg"));
            let assets = names
                .iter()
                .rev()
                .map(|name| GitHubReleaseAsset {
                    name: name.clone(),
                    browser_download_url: "https://example.invalid".into(),
                    size: 42,
                    digest: Some(format!("sha256:{}", "a".repeat(64))),
                })
                .collect::<Vec<_>>();
            assert_eq!(select("1.9.0", "", &assets, &names).unwrap().name, names[0]);
        }
    }
    #[test]
    fn checksum_manifest_requires_one_exact_valid_entry() {
        let Some(name) = native_names("1.9.0").into_iter().next() else { return };
        let entry = format!("{}  {name}\n", "a".repeat(64));
        assert!(from_checksums("1.9.0", &entry).is_some());
        assert!(from_checksums("1.9.0", &(entry.clone() + &entry)).is_none());
        assert!(from_checksums("1.9.0", &format!("bad  {name}")).is_none());
        assert!(from_checksums("1.9.0", &entry.replace(&name, &(name.clone() + ".bak"))).is_none());
        assert!(from_checksums("1.9.1", &entry).is_none());
    }
}
