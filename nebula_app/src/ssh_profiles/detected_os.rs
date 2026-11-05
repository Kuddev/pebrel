//! Remote systems learned after connecting, kept apart from user-edited profiles.
//!
//! A host whose icon is automatic shows the system its server reported. The map
//! lives in its own file so a background connection never rewrites
//! `ssh_profiles.json` while the host editor holds an older copy of it.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const FILE: &str = "ssh_detected_os.json";
const MAXIMUM_HOSTS: usize = 512;

fn file_beside(profiles_path: &Path) -> PathBuf {
    profiles_path.with_file_name(FILE)
}

/// A missing or unreadable map is only a lost hint; icons fall back to the default.
pub(super) fn load(profiles_path: &Path) -> BTreeMap<String, String> {
    std::fs::read(file_beside(profiles_path))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Records `icon` for `destination`; returns whether the stored value changed.
pub(crate) fn remember(profiles_path: &Path, destination: &str, icon: &str) -> io::Result<bool> {
    let path = file_beside(profiles_path);
    let _lease = crate::atomic_file::try_lifetime_lock(&path)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::WouldBlock, "detected systems are being saved")
    })?;
    let mut map = load(profiles_path);
    if map.get(destination).map(String::as_str) == Some(icon) {
        return Ok(false);
    }
    if !map.contains_key(destination) && map.len() >= MAXIMUM_HOSTS {
        map.pop_first();
    }
    map.insert(destination.to_owned(), icon.to_owned());
    let data = serde_json::to_vec_pretty(&map).map_err(io::Error::other)?;
    crate::atomic_file::write(&path, &data)?;
    Ok(true)
}

/// Maps `uname -s` and `/etc/os-release` to an icon id from the host icon catalog.
pub(crate) fn icon_for(uname: &str, os_release: &str) -> Option<&'static str> {
    match uname.trim().to_ascii_lowercase().as_str() {
        "darwin" => return Some("macos"),
        "freebsd" => return Some("freebsd"),
        system
            if system.starts_with("mingw")
                || system.starts_with("msys")
                || system.starts_with("cygwin") =>
        {
            return Some("windows");
        },
        "linux" => {},
        _ => return None,
    }
    let field = |key: &str| {
        os_release.lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix('=')?;
            Some(value.trim().trim_matches(['"', '\'']).to_ascii_lowercase())
        })
    };
    let id = field("ID").unwrap_or_default();
    let like = field("ID_LIKE").unwrap_or_default();
    // The distribution itself first, then the families it declares, in its own order.
    for candidate in std::iter::once(id.as_str()).chain(like.split_whitespace()) {
        let icon = match candidate {
            "ubuntu" => "ubuntu",
            "debian" => "debian",
            "raspbian" => "rpi",
            "centos" => "centos",
            "rhel" | "redhat" => "rhel",
            "fedora" => "fedora",
            "rocky" => "rocky",
            "alpine" => "alpine",
            "arch" | "archarm" => "arch",
            "nixos" => "nixos",
            "kali" => "kali",
            value if value == "suse" || value.starts_with("opensuse") || value == "sles" => "suse",
            _ => continue,
        };
        return Some(icon);
    }
    Some("linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_and_their_declared_families_pick_catalog_icons() {
        assert_eq!(
            icon_for("Linux", "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"),
            Some("ubuntu")
        );
        assert_eq!(icon_for("Linux", "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), Some("ubuntu"));
        assert_eq!(
            icon_for("Linux", "ID=\"almalinux\"\nID_LIKE=\"rhel centos fedora\"\n"),
            Some("rhel")
        );
        assert_eq!(
            icon_for("Linux", "ID=opensuse-leap\nID_LIKE=\"suse opensuse\"\n"),
            Some("suse")
        );
        assert_eq!(icon_for("Linux", "ID=raspbian\nID_LIKE=debian\n"), Some("rpi"));
        assert_eq!(icon_for("Linux", "ID=gentoo\n"), Some("linux"));
        assert_eq!(icon_for("Linux", ""), Some("linux"));
        assert_eq!(icon_for("Darwin\n", ""), Some("macos"));
        assert_eq!(icon_for("MINGW64_NT-10.0", ""), Some("windows"));
        assert_eq!(icon_for("SunOS", ""), None);
    }

    #[test]
    fn remembering_reports_changes_and_survives_reload() {
        let directory = tempfile::tempdir().unwrap();
        let profiles = directory.path().join("ssh_profiles.json");
        assert!(remember(&profiles, "dev@box", "debian").unwrap());
        assert!(!remember(&profiles, "dev@box", "debian").unwrap());
        assert!(remember(&profiles, "dev@box", "ubuntu").unwrap());
        assert_eq!(load(&profiles).get("dev@box").map(String::as_str), Some("ubuntu"));
    }
}
