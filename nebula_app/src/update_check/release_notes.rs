//! Release descriptions are updater metadata, separate from user settings.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReleaseNotes {
    pub version: String,
    pub body: String,
}

fn cache_directory() -> PathBuf {
    nebula_settings::settings_dir().join("updates")
}

fn cache_path(directory: &Path, version: &str) -> PathBuf {
    use sha2::{Digest as _, Sha256};
    // Remote version strings must not become filesystem paths.
    let key = format!("{:x}", Sha256::digest(version.as_bytes()));
    directory.join("release-notes").join(format!("{key}.json"))
}

fn cached(directory: &Path, version: &str) -> Option<ReleaseNotes> {
    read(&cache_path(directory, version), version)
        .or_else(|| read(&directory.join("release-notes.json"), version))
}

fn read(path: &Path, version: &str) -> Option<ReleaseNotes> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let notes: ReleaseNotes = serde_json::from_reader(file.take(2 * 1024 * 1024)).ok()?;
    (notes.version == version).then_some(notes)
}

pub(crate) fn remember(version: &str, body: &str) {
    let notes = ReleaseNotes { version: version.to_owned(), body: body.to_owned() };
    if let Err(error) = save(&cache_path(&cache_directory(), version), &notes) {
        log::warn!("Could not cache release notes: {error}");
    }
}

fn save(path: &Path, notes: &ReleaseNotes) -> Result<(), String> {
    let bytes = serde_json::to_vec(notes).map_err(|error| error.to_string())?;
    crate::atomic_file::write(path, &bytes).map_err(|error| error.to_string())
}

pub(crate) fn snapshot(version: &str, directory: &Path) -> Result<(), String> {
    snapshot_from(&cache_directory(), version, directory)
}

fn snapshot_from(cache: &Path, version: &str, directory: &Path) -> Result<(), String> {
    if let Some(notes) = cached(cache, version) {
        save(&directory.join("release-notes.json"), &notes)?;
    }
    Ok(())
}

/// Use the installed tag, never the latest release (which may already be newer).
pub(crate) fn current() -> Result<ReleaseNotes, String> {
    let version = env!("CARGO_PKG_VERSION");
    if let Some(directory) = crate::update_download::handoff::completed_update_directory()
        && let Some(notes) = read(&directory.join("release-notes.json"), version)
    {
        return Ok(notes);
    }
    if let Some(notes) = cached(&cache_directory(), version) {
        return Ok(notes);
    }
    let url = format!("https://api.github.com/repos/Kuddev/pebrel/releases/tags/v{version}");
    #[cfg(feature = "update-test-source")]
    if let Some(origin) = super::test_source::origin()? {
        return fetch(
            &super::test_source::agent(Duration::from_secs(10)),
            &format!("{origin}/release.json"),
            version,
        );
    }
    let agent = crate::update_proxy::agent(&url, Duration::from_secs(10));
    let notes = fetch(&agent, &url, version)?;
    remember(version, &notes.body);
    Ok(notes)
}

fn fetch(agent: &ureq::Agent, url: &str, version: &str) -> Result<ReleaseNotes, String> {
    let mut response = agent
        .get(url)
        .header("User-Agent", "pebrel")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    parse(&bytes, version)
}

fn parse(bytes: &[u8], version: &str) -> Result<ReleaseNotes, String> {
    let release: super::GitHubRelease =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if release.tag_name.trim().trim_start_matches(['v', 'V']) != version {
        return Err("Release notes do not match the installed version".into());
    }
    Ok(ReleaseNotes { version: version.to_owned(), body: release.body.unwrap_or_default() })
}

pub(crate) fn pending_installed() -> Option<ReleaseNotes> {
    crate::update_download::handoff::completed_update_directory()?;
    if super::load_prompt_state().release_notes_shown.as_deref() == Some(env!("CARGO_PKG_VERSION"))
    {
        return None;
    }
    match current() {
        Ok(notes) => Some(notes),
        Err(error) => {
            // Keep it pending so an offline first launch does not consume the notice.
            log::debug!("Could not load installed release notes: {error}");
            None
        },
    }
}

pub(crate) fn claim(version: &str) -> Result<bool, String> {
    super::update_prompt_state(|state| {
        if state.release_notes_shown.as_deref() == Some(version) {
            return false;
        }
        state.release_notes_shown = Some(version.to_owned());
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_preserves_rich_text_and_rejects_another_version() {
        let body = "# Changes\n\n**Bold** and [link](https://github.com/Kuddev/pebrel)\n\n```sh\npwd\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n";
        let json =
            serde_json::to_vec(&serde_json::json!({"tag_name":"v2.0.0", "body":body})).unwrap();
        assert_eq!(parse(&json, "2.0.0").unwrap().body, body);
        assert!(parse(&json, "2.1.0").is_err());
        assert!(parse(b"invalid JSON", "2.0.0").is_err());
        assert!(parse(br#"{"tag_name":"v2.0.0","body":null}"#, "2.0.0").unwrap().body.is_empty());
    }

    #[test]
    fn release_description_uses_the_update_proxy_and_reports_http_failures() {
        use crate::update_proxy::test_support::{Server, response};
        for (status, success) in [("200 OK", true), ("404 Not Found", false)] {
            let server = Server::start(vec![response(
                status,
                "Content-Type: application/json\r\n",
                r#"{"tag_name":"v2.0.0","body":"**Changes**"}"#,
            )]);
            let result =
                fetch(&server.agent(&[]), "http://api.update.invalid/tags/v2.0.0", "2.0.0");
            assert_eq!(result.is_ok(), success);
            if success {
                assert_eq!(result.unwrap().body, "**Changes**");
            }
            let requests = server.finish();
            assert!(requests[0].0.starts_with("CONNECT api.update.invalid:80 "));
            assert!(requests[0].1.starts_with("GET /tags/v2.0.0 "));
        }
    }

    #[test]
    fn snapshot_round_trips_the_exact_release_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("release-notes.json");
        let notes =
            ReleaseNotes { version: "2.0.0".into(), body: "## 更新\n- **内容**".into() };
        save(&path, &notes).unwrap();
        assert_eq!(read(&path, "2.0.0").unwrap().body, notes.body);
        assert!(read(&path, "2.1.0").is_none());
    }

    #[test]
    fn reading_installed_notes_does_not_evict_the_pending_offline_snapshot() {
        let cache = tempfile::tempdir().unwrap();
        let handoff = tempfile::tempdir().unwrap();
        let pending = ReleaseNotes { version: "2.1.0".into(), body: "New update notes".into() };
        let installed = ReleaseNotes { version: "2.0.0".into(), body: "Installed notes".into() };
        // The update check caches the pending release before Settings reads the installed one.
        save(&cache_path(cache.path(), &pending.version), &pending).unwrap();
        save(&cache_path(cache.path(), &installed.version), &installed).unwrap();
        assert_eq!(cached(cache.path(), &installed.version).unwrap().body, installed.body);
        snapshot_from(cache.path(), &pending.version, handoff.path()).unwrap();
        let restored = read(&handoff.path().join("release-notes.json"), &pending.version).unwrap();
        assert_eq!(restored.body, pending.body);
        assert!(read(&handoff.path().join("release-notes.json"), &installed.version).is_none());
    }

    #[test]
    fn legacy_cache_remains_readable_and_version_keys_cannot_escape_the_cache() {
        let cache = tempfile::tempdir().unwrap();
        let legacy = ReleaseNotes { version: "2.0.0".into(), body: "Existing notes".into() };
        save(&cache.path().join("release-notes.json"), &legacy).unwrap();
        assert_eq!(cached(cache.path(), &legacy.version).unwrap().body, legacy.body);
        assert!(cached(cache.path(), "2.1.0").is_none());
        let unusual = ReleaseNotes { version: "../../outside".into(), body: "Isolated".into() };
        let path = cache_path(cache.path(), &unusual.version);
        assert_eq!(path.parent().unwrap(), cache.path().join("release-notes"));
        save(&path, &unusual).unwrap();
        assert_eq!(cached(cache.path(), &unusual.version).unwrap().body, unusual.body);
        assert_eq!(cached(cache.path(), &legacy.version).unwrap().body, legacy.body);
    }
}
