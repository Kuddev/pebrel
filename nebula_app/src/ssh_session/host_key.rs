//! Changed-key updates preserve unrelated known_hosts bytes and require a fresh snapshot.
use std::io::{self, Write as _};
use std::path::Path;

use hmac::{Hmac, KeyInit as _, Mac as _};
use russh::keys::ssh_key::{self, known_hosts::{Entry, HostPatterns}};

pub(super) struct Change {
    snapshot: Vec<u8>,
    replacement: Vec<u8>,
    pub(super) fingerprints: Vec<String>,
}

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn matches(patterns: &HostPatterns, endpoint: &str) -> bool {
    match patterns {
        HostPatterns::HashedName { salt, hash } => {
            let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(salt) else { return false };
            mac.update(endpoint.as_bytes());
            mac.verify_slice(hash).is_ok()
        },
        HostPatterns::Patterns(patterns) => {
            let matches = |pattern: &str| {
                let expression = regex::escape(pattern).replace("\\*", ".*").replace("\\?", ".");
                regex::Regex::new(&format!("^{expression}$")).is_ok_and(|regex| regex.is_match(endpoint))
            };
            !patterns.iter().any(|pattern| pattern.strip_prefix('!').is_some_and(matches))
                && patterns.iter().any(|pattern| !pattern.starts_with('!') && matches(pattern))
        },
    }
}

/// Marked entries are never downgraded to an ordinary trust prompt. Pattern-based
/// replacement is deliberately refused: a literal update cannot revoke a wildcard.
pub(super) fn inspect(path: &Path, host: &str, port: u16, key: &ssh_key::PublicKey) -> io::Result<Option<Change>> {
    let snapshot = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let source = std::str::from_utf8(&snapshot).map_err(|_| denied("known_hosts is not UTF-8"))?;
    let endpoint = if port == 22 { host.to_owned() } else { format!("[{host}]:{port}") };
    let mut replacement = String::new();
    let mut fingerprints = Vec::new();
    for line in source.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let ending = &line[content.len()..];
        let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() || normalized.starts_with('#') {
            replacement.push_str(line);
            continue;
        }
        let entry: Entry = normalized.parse().map_err(|_| denied("Cannot safely parse known_hosts"))?;
        if !matches(entry.host_patterns(), &endpoint) {
            replacement.push_str(line);
            continue;
        }
        if entry.marker().is_some() {
            return Err(denied("SSH host has a revoked or certificate-authority record; update it outside Pebrel"));
        }
        if entry.public_key().algorithm() != key.algorithm() {
            replacement.push_str(line);
            continue;
        }
        if entry.public_key().key_data() == key.key_data() {
            replacement.push_str(line);
            continue;
        }
        let fingerprint = entry.public_key().fingerprint(ssh_key::HashAlg::Sha256).to_string();
        if !fingerprints.contains(&fingerprint) { fingerprints.push(fingerprint); }
        let hosts = content.split_whitespace().next().expect("parsed host field");
        let prefix_length = content.find(hosts).expect("original host field");
        let suffix = &content[prefix_length + hosts.len()..];
        let remaining = match entry.host_patterns() {
            HostPatterns::HashedName { .. } => Vec::new(),
            HostPatterns::Patterns(patterns) => {
                if patterns.iter().any(|pattern| pattern.contains(['*', '?']) || pattern.starts_with('!')) {
                    return Err(denied("Pattern-based SSH host records cannot be safely replaced"));
                }
                patterns.iter().filter(|pattern| *pattern != &endpoint).cloned().collect::<Vec<_>>()
            },
        };
        if !remaining.is_empty() {
            replacement.push_str(&content[..prefix_length]);
            replacement.push_str(&remaining.join(","));
            replacement.push_str(suffix);
            replacement.push_str(if ending.is_empty() { "\n" } else { ending });
        }
        let target = if matches!(entry.host_patterns(), HostPatterns::HashedName { .. }) { hosts } else { &endpoint };
        replacement.push_str(&content[..prefix_length]);
        replacement.push_str(target);
        replacement.push(' ');
        let mut saved_key = key.clone();
        saved_key.set_comment(entry.public_key().comment().clone());
        replacement.push_str(&saved_key.to_openssh().map_err(io::Error::other)?);
        replacement.push_str(ending);
    }
    if fingerprints.is_empty() { return Ok(None); }
    Ok(Some(Change { snapshot, replacement: replacement.into_bytes(), fingerprints }))
}

impl Change {
    pub(super) fn save(self, path: &Path) -> io::Result<()> {
        let _lock = crate::atomic_file::try_lock(path)?.ok_or_else(|| denied("known_hosts is busy"))?;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(denied("Only a regular known_hosts file can be updated"));
        }
        if std::fs::read(path)? != self.snapshot {
            return Err(denied("known_hosts changed during confirmation; reconnect to verify again"));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| denied("No known_hosts directory"))?)?;
        temporary.as_file().set_permissions(metadata.permissions())?;
        temporary.write_all(&self.replacement)?;
        temporary.as_file().sync_all()?;
        // Independent OpenSSH writers do not share Pebrel's lock. Compare again
        // immediately before replace; this is not a cross-process compare-and-swap.
        if std::fs::read(path)? != self.snapshot {
            return Err(denied("known_hosts changed during confirmation; reconnect to verify again"));
        }
        crate::atomic_file::replace(temporary.path(), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use ssh_key::{Algorithm, PrivateKey};

    #[test]
    fn update_preserves_other_hosts_ports_algorithms_hashes_comments_and_endings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let old = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let new = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve: ssh_key::EcdsaCurve::NistP256 }).unwrap();
        let mut commented = old.public_key().clone();
        commented.set_comment("operator comment");
        let old_line = commented.to_openssh().unwrap();
        let endpoint = "[fixture.example]:2200";
        let salt = b"fixed regression salt";
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(salt).unwrap();
        mac.update(endpoint.as_bytes());
        let encoder = base64::engine::general_purpose::STANDARD;
        let hash = format!("|1|{}|{}", encoder.encode(salt), encoder.encode(mac.finalize().into_bytes()));
        let unrelated = format!("# retained comment\r\nfixture.example {}\r\n{endpoint} {}\r\n", old.public_key().to_openssh().unwrap(), other.public_key().to_openssh().unwrap());
        let source = format!("{unrelated}{endpoint},other.example {old_line}\r\n{hash} {old_line}");
        std::fs::write(&path, &source).unwrap();
        let change = inspect(&path, "fixture.example", 2200, new.public_key()).unwrap().unwrap();
        assert_eq!(change.fingerprints, [old.public_key().fingerprint(ssh_key::HashAlg::Sha256).to_string()]);
        change.save(&path).unwrap();
        let actual = std::fs::read_to_string(&path).unwrap();
        assert!(actual.starts_with(&unrelated));
        assert!(actual.contains(&format!("other.example {old_line}\r\n")));
        assert!(actual.contains("operator comment"));
        assert!(actual.lines().any(|line| line.starts_with(&hash)));
        assert!(!actual.ends_with('\n'));
        assert!(russh::keys::known_hosts::check_known_hosts_path("fixture.example", 2200, new.public_key(), &path).unwrap());
        assert!(russh::keys::known_hosts::check_known_hosts_path("other.example", 22, old.public_key(), &path).unwrap());
        assert!(russh::keys::known_hosts::check_known_hosts_path("fixture.example", 22, old.public_key(), &path).unwrap());
    }

    #[test]
    fn stale_snapshot_marked_and_wildcard_records_cannot_be_updated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let old = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let new = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let source = format!("fixture.example {}\n", old.public_key().to_openssh().unwrap());
        std::fs::write(&path, &source).unwrap();
        let change = inspect(&path, "fixture.example", 22, new.public_key()).unwrap().unwrap();
        let concurrent = format!("{source}# another writer\n");
        std::fs::write(&path, &concurrent).unwrap();
        assert!(change.save(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), concurrent);
        for hosts in ["@revoked fixture.example", "@cert-authority fixture.example", "*.example", "*.example,!excluded.example"] {
            let source = format!("{hosts} {}\n", old.public_key().to_openssh().unwrap());
            std::fs::write(&path, &source).unwrap();
            assert!(inspect(&path, "fixture.example", 22, new.public_key()).is_err(), "{hosts}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        }
    }
}
