//! Pane-owned file intake shares the streaming upload engine and publication rules.
use super::*;

pub(crate) struct FileIntake {
    context: TaskContext,
}

/// A completed upload is retained only after its owning pane accepts the paths.
/// Dropping a failed/stale result schedules cleanup on the existing SSH runtime.
pub(crate) struct UploadedPaths {
    pub(crate) paths: Vec<String>,
    cleanup: Option<UploadCleanup>,
}

struct UploadCleanup {
    session: Arc<SftpSession>,
    files: Vec<String>,
    directories: Vec<String>,
}

impl UploadedPaths {
    #[cfg(test)]
    pub(crate) fn test_paths(paths: Vec<String>) -> Self {
        Self { paths, cleanup: None }
    }

    pub(crate) fn retain(mut self) {
        self.cleanup = None;
    }
}

impl Drop for UploadedPaths {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            if let Ok(runtime) = crate::ssh_session::runtime() {
                runtime.spawn(async move {
                    if tokio::time::timeout(Duration::from_secs(30), cleanup.run()).await.is_err() {
                        log::warn!(
                            "file intake cleanup timed out; remote staging files may remain"
                        );
                    }
                });
            }
        }
    }
}

impl UploadCleanup {
    async fn run(self) {
        for path in self.files {
            let _ = self.session.remove_file(path).await;
        }
        for path in self.directories.into_iter().rev() {
            if let Err(error) = self.session.remove_dir(&path).await {
                log::debug!("file intake cleanup could not remove {path}: {error}");
            }
        }
    }
}

impl FileIntake {
    pub(crate) fn start(
        destination: String,
        connection: crate::ssh_session::PaneConnection,
        directory: Option<String>,
        paths: Vec<PathBuf>,
        wake: WakeFn,
    ) -> io::Result<(Self, tokio::sync::oneshot::Receiver<Result<UploadedPaths, String>>)> {
        let runtime = crate::ssh_session::runtime()?;
        let intake = Self::new(destination.clone(), directory.clone(), wake);
        let context = intake.context.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let mut work = Box::pin(async {
                context.check_cancelled()?;
                let session = Arc::new(connection.open_sftp().await?);
                upload(session, directory.as_deref(), paths, &context).await
            });
            let result = tokio::select! {
                biased;
                _ = connection.closed() => {
                    context.task_control.fetch_or(CANCEL_REQUESTED, Ordering::AcqRel);
                    work.await
                },
                result = &mut work => result,
            }
            .map_err(|error: SftpError| error.to_string());
            let _ = send.send(result);
        });
        Ok((intake, receive))
    }

    fn new(destination: String, directory: Option<String>, wake: WakeFn) -> Self {
        Self {
            context: TaskContext {
                state: Arc::new(Mutex::new(SftpSnapshot {
                    destination,
                    path: directory.unwrap_or_else(|| ".".to_owned()),
                    entries: Vec::new(),
                    phase: SftpPhase::Working,
                    error: None,
                    progress: Some(TransferProgress::new("", 0)),
                })),
                task_control: Arc::new(AtomicUsize::new(0)),
                generation: Arc::new(AtomicU64::new(1)),
                task_generation: 1,
                wake,
                last_wake: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1))),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn test_pending() -> Self {
        Self::new("fixture".into(), None, Arc::new(|| {}))
    }

    pub(crate) fn progress(&self) -> TransferProgress {
        lock(&self.context.state).progress.clone().unwrap_or_else(|| TransferProgress::new("", 0))
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.context.cancelled()
    }

    pub(crate) fn cancel(&self) {
        self.context.task_control.fetch_or(CANCEL_REQUESTED, Ordering::AcqRel);
    }
}

impl Drop for FileIntake {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn upload(
    session: Arc<SftpSession>,
    directory: Option<&str>,
    paths: Vec<PathBuf>,
    context: &TaskContext,
) -> SftpResult<UploadedPaths> {
    if paths.is_empty() || paths.len() > 32 {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "Select between 1 and 32 files").into()
        );
    }
    context.check_cancelled()?;
    let parent = session.canonicalize(directory.unwrap_or(".")).await?;
    if !parent.starts_with('/') || parent.chars().any(char::is_control) || parent.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid remote directory").into());
    }
    // A private per-drop directory avoids replacing project files, including a
    // file created by another writer after a KeepBoth conflict check.
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let directory = format!(
        "{}/.pebrel-drop-{:032x}",
        parent.trim_end_matches('/'),
        u128::from_le_bytes(random)
    );
    context.check_cancelled()?;
    session.create_dir(&directory).await?;
    let mut uploaded = UploadedPaths {
        paths: Vec::new(),
        cleanup: Some(UploadCleanup {
            session: session.clone(),
            files: Vec::new(),
            directories: vec![directory.clone()],
        }),
    };
    session
        .set_metadata(
            &directory,
            russh_sftp::protocol::FileAttributes {
                permissions: Some(0o700),
                ..russh_sftp::protocol::FileAttributes::empty()
            },
        )
        .await?;
    context.check_cancelled()?;
    let options =
        SftpTransferOptions { conflict: SftpConflictPolicy::KeepBoth, skip_unchanged: false };
    let roots = resolve_upload_roots(&session, paths, &directory, options).await?;
    uploaded.paths = roots.iter().map(|root| root.remote.clone()).collect();
    let plan = tokio::task::spawn_blocking(move || build_upload_plan(roots))
        .await
        .map_err(|error| io::Error::other(error.to_string()))??;
    let cleanup = uploaded.cleanup.as_mut().unwrap();
    cleanup.files = plan.files.iter().map(|file| file.remote.clone()).collect();
    cleanup.directories.extend(plan.directories.iter().cloned());
    context.set_total(plan.total);
    execute_upload_plan(&session, plan, options, context).await?;
    context.check_cancelled()?;
    Ok(uploaded)
}

#[cfg(test)]
#[path = "intake/tests.rs"]
mod tests;
