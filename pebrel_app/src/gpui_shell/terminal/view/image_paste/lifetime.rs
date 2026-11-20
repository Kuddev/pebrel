use std::time::{Duration, Instant};

use crate::ssh_session::PaneConnection;

use super::MAX_STAGED_BYTES;

// The PTY supplies a path, not an acknowledgement that the reader opened it.
// Keep a grace period after confirmed completion for deferred reads.
const READ_GRACE: Duration = Duration::from_secs(120);
const CLEANUP_RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImageOwner {
    Draft,
    Shell,
    AgentDraft,
    Agent,
}

#[derive(Clone, Copy)]
pub(super) enum Retire {
    AgentTurn,
    AgentSession,
    Command,
}

enum File {
    Local(tempfile::TempPath),
    Remote(RemoteFile),
}

struct RemoteFile {
    connection: PaneConnection,
    path: Option<String>,
    delivered: bool,
}

impl Drop for RemoteFile {
    fn drop(&mut self) {
        // An abandoned upload was never given to a reader. Published paths
        // may outlive a dropped view; only retire those on a proven lifecycle edge.
        if !self.delivered
            && let Some(path) = self.path.take()
        {
            crate::ssh_sftp::remove_clipboard_images_on_pane(
                self.connection.clone(),
                vec![path],
                |_| {},
            );
        }
    }
}

pub(super) struct Attachment {
    file: File,
    bytes: usize,
    owner: ImageOwner,
    expires: Option<Instant>,
}

impl Attachment {
    pub(super) fn local(file: tempfile::TempPath, bytes: usize) -> Self {
        Self { file: File::Local(file), bytes, owner: ImageOwner::Draft, expires: None }
    }

    pub(super) fn remote(path: String, connection: PaneConnection, bytes: usize) -> Self {
        let file = File::Remote(RemoteFile { connection, path: Some(path), delivered: false });
        Self { file, bytes, owner: ImageOwner::Draft, expires: None }
    }

    pub(super) fn belongs_to(&self, connection: Option<&PaneConnection>) -> bool {
        match &self.file {
            File::Local(_) => true,
            File::Remote(file) => {
                connection.is_some_and(|active| file.connection.same_scope(active))
            },
        }
    }
}

#[derive(Default)]
pub(super) struct Attachments {
    files: Vec<Attachment>,
    cleaning_bytes: usize,
}

pub(super) struct CleanupResult {
    retry: Vec<Attachment>,
}

impl Attachments {
    pub(super) fn remaining(&self) -> usize {
        let retained: usize = self.files.iter().map(|file| file.bytes).sum();
        MAX_STAGED_BYTES.saturating_sub(retained + self.cleaning_bytes)
    }

    pub(super) fn push(&mut self, mut attachment: Attachment, owner: ImageOwner) {
        debug_assert!(attachment.bytes <= self.remaining());
        attachment.owner = owner;
        if let File::Remote(file) = &mut attachment.file {
            file.delivered = true;
        }
        self.files.push(attachment);
    }

    pub(super) fn assign(&mut self, from: ImageOwner, to: ImageOwner) {
        for attachment in &mut self.files {
            if attachment.owner == from {
                attachment.owner = to;
            }
        }
    }

    pub(super) fn retire(&mut self, source: Retire, now: Instant) {
        for attachment in &mut self.files {
            let finished = match source {
                Retire::AgentTurn => attachment.owner == ImageOwner::Agent,
                Retire::AgentSession => {
                    matches!(attachment.owner, ImageOwner::AgentDraft | ImageOwner::Agent)
                },
                Retire::Command => attachment.owner != ImageOwner::Draft,
            };
            if finished {
                attachment.expires.get_or_insert(now + READ_GRACE);
            }
        }
    }

    pub(super) fn next_expiry(&self) -> Option<Instant> {
        self.files.iter().filter_map(|file| file.expires).min()
    }

    pub(super) fn take_expired(&mut self, now: Instant) -> Vec<Attachment> {
        let expired: Vec<_> = self
            .files
            .extract_if(.., |file| file.expires.is_some_and(|expiry| expiry <= now))
            .collect();
        self.cleaning_bytes += expired.iter().map(|file| file.bytes).sum::<usize>();
        expired
    }

    pub(super) fn finish_cleanup(&mut self, mut result: CleanupResult, now: Instant) {
        self.cleaning_bytes = 0;
        for file in &mut result.retry {
            file.expires = Some(now + CLEANUP_RETRY);
        }
        self.files.append(&mut result.retry);
    }

    pub(super) fn forget_disconnected_files(&mut self) {
        self.files.retain(|attachment| {
            !matches!(&attachment.file, File::Remote(file) if file.connection.scope_closed())
        });
    }
}

pub(super) async fn remove(expired: Vec<Attachment>) -> CleanupResult {
    let (mut retry, remote) = remove_local_files(expired, |path| std::fs::remove_file(path));
    let mut batches: Vec<(PaneConnection, Vec<Attachment>)> = Vec::new();
    for attachment in remote {
        let File::Remote(file) = &attachment.file else { unreachable!() };
        if let Some((_, files)) =
            batches.iter_mut().find(|(connection, _)| connection.same_scope(&file.connection))
        {
            files.push(attachment);
        } else {
            batches.push((file.connection.clone(), vec![attachment]));
        }
    }
    for (connection, files) in batches {
        if connection.scope_closed() {
            continue;
        }
        let paths = files
            .iter()
            .filter_map(|attachment| match &attachment.file {
                File::Remote(file) => file.path.clone(),
                File::Local(_) => None,
            })
            .collect();
        let (finished, result) = futures::channel::oneshot::channel();
        crate::ssh_sftp::remove_clipboard_images_on_pane(connection, paths, move |failed| {
            let _ = finished.send(failed);
        });
        let failed = result.await.ok();
        retry.extend(files.into_iter().filter(|attachment| {
            let File::Remote(file) = &attachment.file else { unreachable!() };
            !file.connection.scope_closed()
                && failed.as_ref().is_none_or(|failed| {
                    file.path.as_ref().is_some_and(|path| failed.contains(path))
                })
        }));
    }
    CleanupResult { retry }
}

fn remove_local_files(
    expired: Vec<Attachment>,
    remove_local: impl Fn(&std::path::Path) -> std::io::Result<()>,
) -> (Vec<Attachment>, Vec<Attachment>) {
    let mut retry = Vec::new();
    let mut remote = Vec::new();
    for mut attachment in expired {
        match &mut attachment.file {
            File::Local(path) => {
                if let Err(error) = remove_local(path.as_ref())
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    log::debug!("Could not remove a temporary clipboard image: {error}");
                    retry.push(attachment);
                } else {
                    path.disable_cleanup(true);
                }
            },
            File::Remote(_) => remote.push(attachment),
        }
    }
    (retry, remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(bytes: usize) -> (Attachment, std::path::PathBuf) {
        let file = crate::clipboard::stage_image_png(b"image").unwrap();
        let path = file.to_path_buf();
        (Attachment::local(file, bytes), path)
    }

    #[test]
    fn thirty_three_pastes_do_not_exhaust_the_byte_budget() {
        let mut files = Attachments::default();
        for _ in 0..33 {
            let (file, path) = local(1024);
            files.push(file, ImageOwner::Draft);
            assert!(path.is_file());
        }
        assert_eq!(files.remaining(), MAX_STAGED_BYTES - 33 * 1024);
    }

    #[test]
    fn verified_completion_and_grace_reclaim_bytes_without_touching_a_new_draft() {
        let start = Instant::now();
        let mut files = Attachments::default();
        let (first, first_path) = local(MAX_STAGED_BYTES - 1024);
        files.push(first, ImageOwner::AgentDraft);
        files.assign(ImageOwner::AgentDraft, ImageOwner::Agent);
        assert!(files.take_expired(start + READ_GRACE * 2).is_empty());
        let (draft, draft_path) = local(1024);
        files.push(draft, ImageOwner::AgentDraft);
        assert_eq!(files.remaining(), 0);

        files.retire(Retire::AgentTurn, start);
        assert!(files.take_expired(start + READ_GRACE - Duration::from_millis(1)).is_empty());
        assert!(first_path.exists());
        let expired = files.take_expired(start + READ_GRACE);
        assert_eq!(files.remaining(), 0);
        files.finish_cleanup(futures::executor::block_on(remove(expired)), start + READ_GRACE);
        assert!(!first_path.exists());
        assert!(draft_path.exists());
        assert_eq!(files.remaining(), MAX_STAGED_BYTES - 1024);
    }

    #[test]
    fn agent_turn_completion_does_not_retire_the_shell_commands_files() {
        let start = Instant::now();
        let mut files = Attachments::default();
        let (shell, shell_path) = local(1024);
        files.push(shell, ImageOwner::Shell);
        files.retire(Retire::AgentTurn, start);
        assert!(files.take_expired(start + READ_GRACE).is_empty());
        files.retire(Retire::Command, start);
        let expired = files.take_expired(start + READ_GRACE);
        files.finish_cleanup(futures::executor::block_on(remove(expired)), start + READ_GRACE);
        assert!(!shell_path.exists());
    }

    #[test]
    fn a_file_locked_by_a_reader_counts_until_deletion_succeeds() {
        let start = Instant::now();
        let mut files = Attachments::default();
        let (file, path) = local(MAX_STAGED_BYTES);
        files.push(file, ImageOwner::Shell);
        files.retire(Retire::Command, start);
        let expiry = start + READ_GRACE;
        let expired = files.take_expired(expiry);
        let (retry, remote) =
            remove_local_files(expired, |_| Err(std::io::ErrorKind::PermissionDenied.into()));
        assert!(remote.is_empty());
        files.finish_cleanup(CleanupResult { retry }, expiry);
        assert!(path.exists());
        assert_eq!(files.remaining(), 0);
        assert!(files.take_expired(expiry).is_empty());

        let retry_at = expiry + CLEANUP_RETRY;
        let expired = files.take_expired(retry_at);
        files.finish_cleanup(futures::executor::block_on(remove(expired)), retry_at);
        assert!(!path.exists());
        assert_eq!(files.remaining(), MAX_STAGED_BYTES);
    }
}
