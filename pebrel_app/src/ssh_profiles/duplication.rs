//! Independent saved-host identities; endpoint and secret copying stay out of the UI.

use super::*;

impl SshProfiles {
    pub(crate) fn connection_destination<'a>(&'a self, identity: &'a str) -> &'a str {
        self.targets.get(identity).map(String::as_str).unwrap_or(identity)
    }

    pub(crate) fn set_connection_destination(
        &mut self,
        identity: &str,
        target: &str,
    ) -> Result<(), String> {
        validate_ssh_destination(target)?;
        if !self.contains(identity) || self.targets.contains_key(target) || identity == target {
            return Err("Invalid copied host target".into());
        }
        self.targets.insert(identity.to_owned(), target.to_owned());
        Ok(())
    }

    pub(super) fn validate_targets(&self) -> Result<(), String> {
        for (identity, target) in &self.targets {
            validate_ssh_destination(identity)?;
            validate_ssh_destination(target)?;
            if !self.contains(identity) || self.targets.contains_key(target) || identity == target {
                return Err("Invalid copied host target".into());
            }
        }
        Ok(())
    }

    /// 编辑副本时只改变它自己的连接目标，不能 upsert 到原主机的地址键上。
    pub(crate) fn edited_identity(
        &mut self,
        original: Option<&str>,
        target: &str,
    ) -> Result<String, String> {
        if let Some(identity) = original.filter(|identity| self.targets.contains_key(*identity)) {
            self.set_connection_destination(identity, target)?;
            Ok(identity.to_owned())
        } else {
            Ok(target.to_owned())
        }
    }

    pub(crate) fn duplicate_host(
        &mut self,
        source: &str,
        identity: &str,
    ) -> Result<String, String> {
        validate_ssh_destination(identity)?;
        if identity == source || self.contains(identity) {
            return Err("Host identity already exists".into());
        }
        let target = self.connection_destination(source).to_owned();
        validate_ssh_destination(&target)?;
        if identity == target {
            return Err("Host identity already exists".into());
        }
        let mut profile = self.for_destination(source);
        let prefix =
            profile.label.as_deref().filter(|label| !label.trim().is_empty()).unwrap_or(&target);
        let label = self.next_default_label(prefix.trim());
        profile.destination = identity.to_owned();
        profile.label = Some(label.clone());
        let organization = self.organization(source).clone();
        self.upsert(profile);
        self.set_connection_destination(identity, &target)?;
        self.set_organization(identity, organization)?;
        Ok(label)
    }
}

/// 写入副本自己的凭据后才提交 Profile；失败只撤回新凭据，原主机始终不动。
pub(crate) fn save_duplicate(
    mut profiles: SshProfiles,
    path: &Path,
    source: &str,
    identity: &str,
    mut load: impl FnMut(&str) -> io::Result<Option<Vec<u8>>>,
    mut store: impl FnMut(&str, &[u8]) -> io::Result<()>,
    mut delete: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<(SshProfiles, String)> {
    let connection = profiles.for_destination(source).connection;
    let label = profiles.duplicate_host(source, identity).map_err(io::Error::other)?;
    let mut keys = vec![(
        crate::ssh_credentials::credential_target(source),
        crate::ssh_credentials::credential_target(identity),
    )];
    if let Some(old) = connection.proxy_credential_target(source) {
        keys.push((old, connection.proxy_credential_target(identity).expect("same proxy options")));
    }
    let mut written = Vec::new();
    let result = (|| {
        for (old, new) in keys {
            if load(&new)?.is_some() {
                return Err(io::Error::other("Copied host credential identity already exists"));
            }
            if let Some(secret) = load(&old)? {
                let secret = zeroize::Zeroizing::new(secret);
                store(&new, &secret)?;
                written.push(new);
            }
        }
        profiles.save(path)
    })();
    if let Err(error) = result {
        let cleanup: Vec<_> = written.iter().filter_map(|key| delete(key).err()).collect();
        return if cleanup.is_empty() {
            Err(error)
        } else {
            Err(io::Error::other(format!("{error}; copied credential cleanup failed: {cleanup:?}")))
        };
    }
    Ok((profiles, label))
}
