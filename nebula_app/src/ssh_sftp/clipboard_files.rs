//! One paste owns its new remote files until the originating pane accepts them.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::AsyncWriteExt as _;

use super::transfer::{self, TransferObserver};

const CLEANUP_BUDGET: Duration = Duration::from_secs(15);

#[derive(Clone, Default)]
pub(crate) struct Operation {
    cancelled: Arc<AtomicBool>,
    wake: Arc<tokio::sync::Notify>,
}

impl Operation {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            self.wake.notified().await;
        }
    }

    fn check(&self) -> io::Result<()> {
        if self.is_cancelled() {
            Err(io::Error::new(io::ErrorKind::Interrupted, "File paste cancelled"))
        } else {
            Ok(())
        }
    }
}

impl TransferObserver for Operation {
    fn advance(&self, _bytes: u64) {}
    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

pub(crate) struct UploadedFiles {
    session: Arc<SftpSession>,
    paths: Vec<String>,
    runtime: tokio::runtime::Handle,
}

impl UploadedFiles {
    pub(crate) fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Called only after the pane has inserted every path without sending Enter.
    pub(crate) fn keep(mut self) {
        self.paths.clear();
    }

    async fn rollback(&mut self) {
        cleanup(&self.session, &self.paths).await;
        self.paths.clear();
    }
}

impl Drop for UploadedFiles {
    fn drop(&mut self) {
        if self.paths.is_empty() {
            return;
        }
        let session = self.session.clone();
        let paths = std::mem::take(&mut self.paths);
        // A completed upload can be discarded on the GPUI thread after focus or
        // session changes. Keep its authenticated channel alive for cleanup.
        self.runtime.spawn(async move { cleanup(&session, &paths).await });
    }
}

async fn cleanup(session: &SftpSession, paths: &[String]) {
    let remove = async {
        for path in paths {
            if let Err(error) = session.remove_file(path.clone()).await {
                log::warn!("file paste cleanup failed for {path}: {error}");
            }
        }
    };
    if tokio::time::timeout(CLEANUP_BUDGET, remove).await.is_err() {
        log::warn!("file paste cleanup timed out; remote files may remain: {paths:?}");
    }
}

/// Runs on the pane's original SSH connection; never resolves an alias again.
pub(crate) async fn upload(
    source: crate::ssh_session::SftpSource,
    directory: Option<String>,
    paths: Vec<PathBuf>,
    operation: Operation,
) -> Result<UploadedFiles, String> {
    let session = tokio::select! {
        result = source.open() => result.map_err(|error| error.to_string())?,
        _ = operation.cancelled() => return Err("File paste cancelled".to_owned()),
    };
    let work = upload_on(Arc::new(session), directory, paths, &operation);
    tokio::pin!(work);
    tokio::select! {
        biased;
        _ = source.wait_closed() => {
            // Await the same future so an in-flight exclusive OPEN can establish
            // ownership before cancellation rolls back the acknowledged files.
            operation.cancel();
            work.await
        },
        result = &mut work => result,
    }
}

async fn upload_on(
    session: Arc<SftpSession>,
    directory: Option<String>,
    paths: Vec<PathBuf>,
    operation: &Operation,
) -> Result<UploadedFiles, String> {
    let mut uploaded =
        UploadedFiles { session, paths: Vec::new(), runtime: tokio::runtime::Handle::current() };
    // Do not abandon an in-flight OPEN: its acknowledgement establishes whether
    // this operation owns the path. Cancellation is observed immediately after it.
    // SFTP requests retain the shared finite request timeout. Streaming is
    // cancellable, without a size-dependent whole-batch timeout cutting an OPEN.
    let result = upload_batch(&mut uploaded, directory, paths, operation)
        .await
        .map_err(|error| error.to_string());
    match result {
        Ok(()) if !operation.is_cancelled() => Ok(uploaded),
        result => {
            uploaded.rollback().await;
            Err(result.err().unwrap_or_else(|| "File paste cancelled".to_owned()))
        },
    }
}

async fn upload_batch(
    uploaded: &mut UploadedFiles,
    directory: Option<String>,
    paths: Vec<PathBuf>,
    operation: &Operation,
) -> super::SftpResult<()> {
    operation.check()?;
    if paths.is_empty() || paths.len() > crate::clipboard::files::MAX_FILES {
        return Err("Copy between 1 and 32 files at a time".into());
    }
    // Validate the whole batch before publishing any file. Keep open descriptors
    // so path replacement cannot cause a later file to upload different contents.
    let mut files = Vec::with_capacity(paths.len());
    let mut names = std::collections::HashSet::new();
    for path in paths {
        operation.check()?;
        let name = path.file_name().and_then(|name| name.to_str()).ok_or("Invalid filename")?;
        if !path.is_absolute()
            || name.chars().any(char::is_control)
            || !names.insert(name.to_owned())
        {
            return Err(
                "Files need distinct names and absolute paths without control characters".into()
            );
        }
        if !tokio::fs::metadata(&path).await?.is_file() {
            return Err(
                "Only regular files can be pasted; upload directories from Remote Files".into()
            );
        }
        let file = tokio::fs::File::open(&path).await?;
        let metadata = file.metadata().await?;
        if !metadata.is_file() {
            return Err("The copied item is no longer a regular file".into());
        }
        files.push((name.to_owned(), file, metadata));
    }
    let directory = directory.unwrap_or_else(|| ".".to_owned());
    let directory = uploaded.session.canonicalize(directory).await?;
    if !directory.starts_with('/') || directory.chars().any(char::is_control) {
        return Err("The SSH server did not return an absolute upload directory".into());
    }
    for (name, mut source, before) in files {
        operation.check()?;
        let path = format!("{}/{name}", directory.trim_end_matches('/'));
        let mut target = uploaded
            .session
            .open_with_flags_and_attributes(
                path.clone(),
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() },
            )
            .await
            .map_err(|error| {
                format!("Cannot create {path}; existing files are never replaced: {error}")
            })?;
        uploaded.paths.push(path);
        let transfer = tokio::select! {
            result = transfer::upload_to_writer(&mut source, &mut target, operation) => result,
            _ = operation.cancelled() => Err(io::Error::new(io::ErrorKind::Interrupted, "File paste cancelled").into()),
        };
        // Drain/close before rollback, including an interrupted queued WRITE.
        if transfer.is_err() {
            let _ = tokio::time::timeout(CLEANUP_BUDGET, target.shutdown()).await;
        }
        transfer?;
        let after = source.metadata().await?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err("A copied file changed during upload; copy it again and retry".into());
        }
    }
    operation.check()?;
    Ok(())
}

#[cfg(test)]
#[path = "clipboard_files/tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) async fn upload_for_test(
    session: Arc<SftpSession>,
    paths: Vec<PathBuf>,
) -> Result<UploadedFiles, String> {
    upload_on(session, Some("/project".into()), paths, &Operation::default()).await
}
