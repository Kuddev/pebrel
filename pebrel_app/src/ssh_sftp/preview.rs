//! Bounded, cancellable reads for remote text and image previews.
//! Progress is coalesced; a partial prefix is never a writable document snapshot.

use super::document::{DocumentOperation, RemoteLocation};
use super::*;
use tokio::io::AsyncReadExt as _;
use tokio::sync::watch;

pub(crate) const IMAGE_MAX_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const IMAGE_MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
const TEXT_PREFIX_BYTES: usize = 256 * 1024;
const IMAGE_PREFIX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum PreviewKind {
    Text,
    Image,
}

impl PreviewKind {
    fn limit(self) -> usize {
        match self {
            Self::Text => crate::text_document::MAX_BYTES,
            Self::Image => IMAGE_MAX_BYTES,
        }
    }

    fn prefix_limit(self) -> usize {
        match self {
            Self::Text => TEXT_PREFIX_BYTES,
            Self::Image => IMAGE_PREFIX_BYTES,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ReadProgress {
    pub received: u64,
    pub total: Option<u64>,
    pub prefix: Option<Arc<[u8]>>,
}

pub(crate) struct RemoteRead {
    pub(super) bytes: Vec<u8>,
    pub(super) target: String,
    pub(super) metadata: russh_sftp::protocol::FileAttributes,
    pub(super) session: Arc<SftpSession>,
}

impl RemoteRead {
    pub(crate) fn into_bytes(self) -> Arc<[u8]> {
        self.bytes.into()
    }
}

/// The existing SSH runtime owns network I/O; the caller owns cancellation.
pub(crate) async fn load(
    location: RemoteLocation,
    operation: DocumentOperation,
    kind: PreviewKind,
    progress: Option<watch::Sender<ReadProgress>>,
) -> io::Result<RemoteRead> {
    let (send, receive) = tokio::sync::oneshot::channel();
    crate::ssh_session::runtime()?.spawn(async move {
        let read = async {
            let session = Arc::new(
                crate::ssh_session::open_sftp(&location.destination)
                    .await
                    .map_err(io::Error::other)?,
            );
            read_from_session(&location, session, &operation, kind, progress).await
        };
        let timeout = match kind {
            PreviewKind::Text => Duration::from_secs(45),
            PreviewKind::Image => Duration::from_secs(120),
        };
        let result = tokio::select! {
            result = tokio::time::timeout(timeout, read) => result.unwrap_or_else(|_| {
                Err(io::Error::new(io::ErrorKind::TimedOut, "Remote file read timed out"))
            }),
            _ = operation.cancelled() => Err(cancelled()),
        };
        let _ = send.send(result);
    });
    receive.await.map_err(|_| io::Error::other("Remote file read ended"))?
}

pub(super) async fn read_from_session(
    location: &RemoteLocation,
    session: Arc<SftpSession>,
    operation: &DocumentOperation,
    kind: PreviewKind,
    progress: Option<watch::Sender<ReadProgress>>,
) -> io::Result<RemoteRead> {
    if operation.is_cancelled() {
        return Err(cancelled());
    }
    let target = session.canonicalize(location.path.clone()).await.map_err(io::Error::other)?;
    let metadata = session.metadata(target.clone()).await.map_err(io::Error::other)?;
    if !metadata.is_regular() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Expected a regular remote file"));
    }
    if matches!(kind, PreviewKind::Image)
        && metadata.size.is_some_and(|size| size > IMAGE_MAX_BYTES as u64)
    {
        return Err(image_too_large());
    }
    let limit = kind.limit();
    let mut file = session.open(target.clone()).await.map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut snapshot = ReadProgress { total: metadata.size, ..ReadProgress::default() };
    let mut next_prefix = buffer.len();
    let mut last_update = Instant::now();
    if let Some(progress) = &progress {
        progress.send_replace(snapshot.clone());
    }
    while bytes.len() <= limit {
        if operation.is_cancelled() {
            return Err(cancelled());
        }
        let read_size = buffer.len().min(limit + 1 - bytes.len());
        let count = file.read(&mut buffer[..read_size]).await?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        snapshot.received = bytes.len() as u64;
        if let Some(progress) = &progress {
            // Exponential checkpoints bound prefix copies and decode attempts.
            // After the prefix budget, updates contain counts and the same Arc.
            let new_prefix = bytes.len() >= next_prefix && next_prefix <= kind.prefix_limit();
            if new_prefix {
                let end = bytes.len().min(kind.prefix_limit());
                snapshot.prefix = Some(Arc::from(&bytes[..end]));
                next_prefix = end.saturating_mul(2);
            }
            if new_prefix || last_update.elapsed() >= Duration::from_millis(100) {
                progress.send_replace(snapshot.clone());
                last_update = Instant::now();
            }
        }
    }
    if bytes.len() > limit && matches!(kind, PreviewKind::Image) {
        return Err(image_too_large());
    }
    let after = session.metadata(target.clone()).await.map_err(io::Error::other)?;
    if metadata.size != after.size
        || metadata.mtime != after.mtime
        || metadata.permissions != after.permissions
        || session.canonicalize(location.path.clone()).await.map_err(io::Error::other)? != target
    {
        return Err(io::Error::other(crate::text_document::SaveError::Changed));
    }
    if bytes.len() <= limit && metadata.size.is_some_and(|size| size != bytes.len() as u64) {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Incomplete remote file read"));
    }
    if operation.is_cancelled() {
        return Err(cancelled());
    }
    if let Some(progress) = progress {
        progress.send_replace(snapshot);
    }
    Ok(RemoteRead { bytes, target, metadata, session })
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Remote file read cancelled")
}

fn image_too_large() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Image preview is limited to 64 MiB")
}
