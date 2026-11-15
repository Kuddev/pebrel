use super::{
    SftpBrowseSession, SftpClientError, SftpEntry, SftpEntryKind, SftpSession, remove_remote_path,
};

pub(crate) enum SftpDeleteError {
    Connection(String),
    Changed(String),
    Remote { name: String, error: SftpClientError },
}

impl SftpBrowseSession {
    pub(crate) async fn delete_entries(
        &self,
        directory: &str,
        entries: Vec<SftpEntry>,
    ) -> Result<(), SftpDeleteError> {
        let mut slot = self.session.lock().await;
        let session = match slot.take() {
            Some(session) => session,
            None => self.open_session().await.map_err(SftpDeleteError::Connection)?,
        };
        let result = delete_entries(&session, directory, entries).await;
        // An interrupted or failed mutation is never retried on this channel.
        if result.is_ok() {
            *slot = Some(session);
        }
        result
    }
}

async fn delete_entries(
    sftp: &SftpSession,
    directory: &str,
    entries: Vec<SftpEntry>,
) -> Result<(), SftpDeleteError> {
    // Accept only literal children of the confirmed directory. In particular,
    // a backslash in a POSIX filename is data, not a separator.
    if let Some(entry) = entries.iter().find(|entry| {
        let literal_path = format!("{}/{}", directory.trim_end_matches('/'), entry.name);
        entry.is_parent
            || matches!(entry.name.as_str(), "" | "." | "..")
            || entry.name.contains(['/', '\0'])
            || entry.path != literal_path
    }) {
        return Err(SftpDeleteError::Changed(entry.name.clone()));
    }

    for entry in entries {
        let metadata = sftp
            .symlink_metadata(&entry.path)
            .await
            .map_err(|error| SftpDeleteError::Remote { name: entry.name.clone(), error })?;
        if entry.kind != SftpEntryKind::from_file_type(metadata.file_type())
            || metadata.size.is_some_and(|size| size != entry.size)
            || metadata.mtime.is_some_and(|mtime| u64::from(mtime) != entry.modified)
        {
            return Err(SftpDeleteError::Changed(entry.name));
        }
        // REMOVE unlinks a symlink itself; RMDIR rejects non-empty directories.
        remove_remote_path(sftp, entry.path, entry.kind)
            .await
            .map_err(|error| SftpDeleteError::Remote { name: entry.name, error })?;
    }
    Ok(())
}
