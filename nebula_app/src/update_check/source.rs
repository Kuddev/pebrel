use super::{RELEASES_PAGE, UpdateAsset};

const GITHUB: &str = "https://github.com/";
const OFFICIAL: &str = "Kuddev/pebrel";
const LEGACY: &str = "Kuddev/nebula";

pub(super) struct ReleaseSource {
    repo: String,
    tag: Option<String>,
}

impl ReleaseSource {
    fn official() -> Self {
        Self { repo: OFFICIAL.into(), tag: None }
    }

    fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim().trim_end_matches('/');
        if value.is_empty() {
            return Ok(Self::official());
        }
        let path = value
            .strip_prefix(GITHUB)
            .ok_or_else(|| "自定义更新地址必须使用 https://github.com".to_owned())?;
        if path.contains('?') || path.contains('#') {
            return Err("自定义更新地址不能包含查询参数或片段".into());
        }
        let (repo_path, tag) = if let Some(repo) = path.strip_suffix("/releases/latest") {
            (repo, None)
        } else if let Some(repo) = path.strip_suffix("/releases") {
            (repo, None)
        } else if let Some((repo, tag)) = path.split_once("/releases/tag/") {
            (repo, Some(tag))
        } else {
            return Err("请填写 GitHub Releases 或 releases/tag/<tag> 地址".into());
        };
        let Some((owner, repo)) = repo_path.split_once('/') else {
            return Err("GitHub Release 地址无效".into());
        };
        if repo.contains('/')
            || !component(owner)
            || !component(repo)
            || tag.is_some_and(|tag| !tag_name(tag))
        {
            return Err("GitHub Release 地址无效".into());
        }
        Ok(Self { repo: format!("{owner}/{repo}"), tag: tag.map(str::to_owned) })
    }

    pub(super) fn is_default(&self) -> bool {
        self.repo == OFFICIAL && self.tag.is_none()
    }

    pub(super) fn api_url(&self) -> String {
        match &self.tag {
            Some(tag) => format!("https://api.github.com/repos/{}/releases/tags/{tag}", self.repo),
            None => format!("https://api.github.com/repos/{}/releases/latest", self.repo),
        }
    }

    fn page(&self) -> String {
        match &self.tag {
            Some(tag) => format!("{GITHUB}{}/releases/tag/{tag}", self.repo),
            None if self.is_default() => RELEASES_PAGE.into(),
            None => format!("{GITHUB}{}/releases", self.repo),
        }
    }

    fn accepts(&self, asset: &UpdateAsset) -> bool {
        if self.is_default() {
            return [OFFICIAL, LEGACY].iter().any(|repo| {
                asset.download_url
                    == format!("{GITHUB}{repo}/releases/download/v{}/{}", asset.version, asset.name)
            });
        }
        let prefix = format!("{GITHUB}{}/releases/download/", self.repo);
        let Some(path) = asset.download_url.strip_prefix(&prefix) else {
            return false;
        };
        let mut parts = path.split('/');
        let (Some(tag), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        name == asset.name && tag_name(tag) && self.tag.as_deref().is_none_or(|value| value == tag)
    }
}

fn component(value: &str) -> bool {
    !matches!(value, "" | "." | "..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn tag_name(value: &str) -> bool {
    !matches!(value, "" | "." | "..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+')
        })
}

pub(super) fn configured() -> Result<ReleaseSource, String> {
    ReleaseSource::parse(&nebula_settings::RuntimeSettings::load().update_release_url)
}

pub(crate) fn normalize_setting(value: &str) -> Result<String, String> {
    let source = ReleaseSource::parse(value)?;
    Ok(if source.is_default() { String::new() } else { source.page() })
}

pub(crate) fn release_page() -> String {
    configured().map(|source| source.page()).unwrap_or_else(|_| RELEASES_PAGE.into())
}

pub(crate) fn validate_asset_url(asset: &UpdateAsset) -> Result<(), String> {
    configured()?
        .accepts(asset)
        .then_some(())
        .ok_or_else(|| "release 安装包 URL 不属于当前更新源".into())
}

#[cfg(test)]
pub(crate) fn validate_official_asset_url(asset: &UpdateAsset) -> Result<(), String> {
    ReleaseSource::official()
        .accepts(asset)
        .then_some(())
        .ok_or_else(|| "release 安装包 URL 不属于 Pebrel 官方仓库".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_source_normalizes_and_pins_explicit_tags() {
        assert_eq!(
            normalize_setting("https://github.com/acme/pebrel/releases/latest").unwrap(),
            "https://github.com/acme/pebrel/releases"
        );
        assert!(normalize_setting("https://example.com/acme/pebrel/releases").is_err());

        let source =
            ReleaseSource::parse("https://github.com/acme/pebrel/releases/tag/v2.0.0-beta.1")
                .unwrap();
        let asset = |tag: &str| UpdateAsset {
            version: "2.0.0-beta.1".into(),
            name: "Pebrel-v2.0.0-beta.1-windows-x64-setup.exe".into(),
            download_url: format!(
                "https://github.com/acme/pebrel/releases/download/{tag}/Pebrel-v2.0.0-beta.1-windows-x64-setup.exe"
            ),
            size: Some(42),
            sha256: Some("a".repeat(64)),
        };
        assert!(source.accepts(&asset("v2.0.0-beta.1")));
        assert!(!source.accepts(&asset("v2.0.0-beta.2")));
    }
}
