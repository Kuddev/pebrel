//! Exact package names shared by release discovery and download validation.
use super::{
    GitHubReleaseAsset, UpdateAsset, checksum_from_release_body, normalize_sha256,
    stable_version_from_tag,
};
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

pub(crate) fn windows_names(version: &str, architecture: &str) -> Vec<String> {
    match architecture {
        "x86_64" => super::windows_x64_installer_names(version).to_vec(),
        "aarch64" => vec![format!("Pebrel-v{version}-windows-arm64-setup.exe")],
        _ => Vec::new(),
    }
}

pub(crate) fn native_names(version: &str) -> Vec<String> {
    match Platform::current() {
        Platform::MacOS => macos_names(version, std::env::consts::ARCH),
        Platform::Windows => windows_names(version, std::env::consts::ARCH),
        _ => Vec::new(),
    }
}

pub(super) fn current_windows_arch() -> Option<&'static str> {
    (Platform::current() == Platform::Windows).then_some(std::env::consts::ARCH)
}

fn windows_zip(tag: &str, architecture: &str) -> Option<(String, String)> {
    let version = stable_version_from_tag(tag)?;
    let architecture = match architecture {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let name = format!("Pebrel-v{version}-windows-{architecture}.zip");
    let url = format!("https://github.com/Kuddev/pebrel/releases/download/{tag}/{name}");
    Some((name, url))
}

pub(super) fn portable_from_assets(
    tag: &str,
    architecture: &str,
    assets: &[GitHubReleaseAsset],
) -> Option<String> {
    let (name, url) = windows_zip(tag, architecture)?;
    assets
        .iter()
        .any(|asset| asset.name == name && asset.browser_download_url == url && asset.size > 0)
        .then_some(url)
}

pub(super) fn portable_from_checksums(
    version: &str,
    architecture: &str,
    text: &str,
) -> Option<String> {
    let (name, url) = windows_zip(&format!("v{version}"), architecture)?;
    manifest_hash(text, &name).map(|_| url)
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
        let sha256 = manifest_hash(text, name)?;
        Some(UpdateAsset {
            version: version.into(),
            name: name.clone(),
            download_url: format!(
                "https://github.com/Kuddev/pebrel/releases/download/v{version}/{name}"
            ),
            size: None,
            sha256: Some(sha256),
        })
    })
}

fn manifest_hash(text: &str, name: &str) -> Option<String> {
    let mut hashes = text.lines().filter_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then_some(hash)
    });
    let hash = hashes.next()?;
    if hashes.next().is_some() {
        return None;
    }
    normalize_sha256(&format!("sha256:{hash}"))
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
    fn windows_architectures_are_exact() {
        let version = "1.9.2";
        let arm = windows_names(version, "aarch64");
        let x64 = windows_names(version, "x86_64");
        assert_eq!(arm, ["Pebrel-v1.9.2-windows-arm64-setup.exe"]);
        assert_eq!(x64, super::super::windows_x64_installer_names(version));
        assert!(windows_names(version, "unknown").is_empty());
        let assets = [x64[0].clone(), arm[0].clone(), "Pebrel-v1.9.2-windows-arm64.zip".into()]
            .into_iter()
            .map(|name| GitHubReleaseAsset {
                name,
                browser_download_url: "https://example.invalid".into(),
                size: 42,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
            })
            .collect::<Vec<_>>();
        assert_eq!(select(version, "", &assets, &arm).unwrap().name, arm[0]);
        assert_eq!(select(version, "", &assets, &x64).unwrap().name, x64[0]);
        assert!(select(version, "", &assets[..1], &arm).is_none());
        assert!(select(version, "", &assets[2..], &arm).is_none());
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

    #[test]
    fn rate_limited_portable_check_requires_the_matching_zip_in_the_manifest() {
        let zip = "Pebrel-v2.5.3-windows-arm64.zip";
        let entry = format!("{}  {zip}\n", "a".repeat(64));
        assert_eq!(
            portable_from_checksums("2.5.3", "aarch64", &entry).as_deref(),
            Some(
                "https://github.com/Kuddev/pebrel/releases/download/v2.5.3/Pebrel-v2.5.3-windows-arm64.zip"
            ),
        );
        assert!(portable_from_checksums("2.5.3", "x86_64", &entry).is_none());
        assert!(
            portable_from_checksums("2.5.3", "aarch64", &entry.replace(".zip", "-setup.exe"))
                .is_none()
        );
        assert!(
            portable_from_checksums("2.5.3", "aarch64", &format!("{entry}bad  {zip}")).is_none()
        );
        assert!(windows_zip("v2.5.3#fragment", "aarch64").is_none());
    }
}
