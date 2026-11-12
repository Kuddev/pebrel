//! One lazily opened SFTP subsystem per authenticated SSH connection.
//!
//! russh-sftp owns the protocol. This adapter owns bounded directory/transfer handles,
//! upload staging and cancellation; it never handles terminal bytes.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use russh::{ChannelMsg, client};
use russh_sftp::{
    client::{RawSftpSession, error::Error as SftpError},
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::session::{Failure, Result};

#[cfg(test)]
mod tests;
mod transfer;

pub(crate) const CHUNK: usize = 32 * 1024;
pub(crate) const BATCH: usize = CHUNK * 64;
const DIRECTORY_PAGE: usize = 256;
const MAX_HANDLES: usize = 4;

fn next_handle_id() -> Result<u64> {
    // 文件通道重开后仍不复用 ID，迟到的取消/读取只会指向已经失效的旧句柄。
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        id.checked_add(1).filter(|next| *next <= i64::MAX as u64)
    })
    .map_err(|_| Failure("SFTP_LIMIT"))
}

pub(crate) struct Call {
    pub request: Request,
    pub reply: oneshot::Sender<Result<Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    List {
        path: String,
        cursor: Option<u64>,
    },
    CloseList {
        cursor: u64,
    },
    Stat {
        path: String,
    },
    OpenRead {
        path: String,
        #[serde(default)]
        offset: u64,
        revision: Option<String>,
    },
    Read {
        transfer: u64,
        offset: u64,
        length: Option<usize>,
    },
    BeginUpload {
        path: String,
        token: Option<String>,
        #[serde(default)]
        offset: u64,
        #[serde(default)]
        resume: bool,
    },
    Write {
        transfer: u64,
        offset: u64,
        data: String,
    },
    WriteBatch {
        transfer: u64,
        offset: u64,
        data: String,
    },
    ResolveUpload {
        path: String,
        token: String,
        size: u64,
        digest: String,
    },
    AbortUpload {
        path: String,
        token: String,
    },
    Commit {
        transfer: u64,
    },
    Close {
        transfer: u64,
    },
    Mkdir {
        path: String,
    },
    Rename {
        path: String,
        destination: String,
        revision: String,
    },
    Remove {
        path: String,
        revision: String,
    },
}

pub(crate) fn parse(text: &str) -> Result<Request> {
    if text.len() > BATCH.div_ceil(3) * 4 + 8192 {
        return Err(Failure("INVALID_INPUT"));
    }
    let request: Request = serde_json::from_str(text).map_err(|_| Failure("INVALID_INPUT"))?;
    let path = match &request {
        Request::List { path, .. }
        | Request::Stat { path }
        | Request::OpenRead { path, .. }
        | Request::BeginUpload { path, .. }
        | Request::ResolveUpload { path, .. }
        | Request::AbortUpload { path, .. }
        | Request::Mkdir { path }
        | Request::Remove { path, .. } => Some(path),
        Request::Rename { path, destination, .. } => {
            validate_path(destination)?;
            Some(path)
        },
        Request::Write { data, .. } if data.len() > CHUNK.div_ceil(3) * 4 => {
            return Err(Failure("INVALID_INPUT"));
        },
        Request::WriteBatch { data, .. } if data.len() > BATCH.div_ceil(3) * 4 => {
            return Err(Failure("INVALID_INPUT"));
        },
        Request::Read { length: Some(length), .. } if *length == 0 || *length > BATCH => {
            return Err(Failure("INVALID_INPUT"));
        },
        _ => None,
    };
    if let Some(path) = path {
        validate_path(path)?;
    }
    Ok(request)
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        Err(Failure("INVALID_INPUT"))
    } else {
        Ok(())
    }
}

impl From<SftpError> for Failure {
    fn from(error: SftpError) -> Self {
        Self(match error {
            SftpError::Status(status) => match status.status_code {
                StatusCode::NoSuchFile => "SFTP_NOT_FOUND",
                StatusCode::PermissionDenied => "SFTP_PERMISSION",
                StatusCode::OpUnsupported => "SFTP_UNSUPPORTED",
                StatusCode::NoConnection | StatusCode::ConnectionLost => "SFTP_CLOSED",
                _ => "SFTP_OPERATION",
            },
            SftpError::Timeout => "SFTP_TIMEOUT",
            SftpError::Limited(_) => "SFTP_LIMIT",
            _ => "SFTP_CLOSED",
        })
    }
}

pub(crate) async fn serve<H: client::Handler>(
    client: &client::Handle<H>,
    calls: &mut mpsc::Receiver<Call>,
) -> Result<i32> {
    let mut files: Option<Files> = None;
    let mut cleanup = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            call = calls.recv() => {
                let Call { request, reply } = call.ok_or(Failure("CLOSED"))?;
                if reply.is_closed() { continue; }
                let verifying = matches!(request, Request::ResolveUpload { .. });
                let operation = async {
                    if files.is_none() { files = Some(Files::open(client).await?); }
                    files.as_mut().expect("SFTP initialized").execute(request).await
                };
                // A lost RENAME reply may require hashing a large final file. Each
                // request still times out, but total verification is not a transfer size limit.
                let result = if verifying { operation.await } else {
                    tokio::time::timeout(Duration::from_secs(60), operation).await.unwrap_or(Err(Failure("SFTP_TIMEOUT")))
                };
                // Reopen only the file subsystem; resumable transactions reconcile ambiguous writes by offset.
                if matches!(result, Err(Failure("SFTP_TIMEOUT" | "SFTP_CLOSED"))) { files = None; }
                let _ = reply.send(result);
            },
            _ = cleanup.tick(), if files.is_some() => {
                if let Some(files) = &mut files { files.expire().await; }
            },
        }
    }
}

struct Directory {
    id: u64,
    path: String,
    handle: String,
    pending: VecDeque<russh_sftp::protocol::File>,
    touched: Instant,
}

struct Transfer {
    handle: Option<String>,
    path: String,
    temp: Option<String>,
    offset: u64,
    attrs: FileAttributes,
    touched: Instant,
}

struct Files {
    raw: Arc<RawSftpSession>,
    directory: Option<Directory>,
    transfers: HashMap<u64, Transfer>,
    nonce: u128,
}

impl Files {
    async fn open<H: client::Handler>(client: &client::Handle<H>) -> Result<Self> {
        let mut channel = client.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => break,
                Some(ChannelMsg::WindowAdjusted { .. }) => {},
                _ => return Err(Failure("SFTP_UNSUPPORTED")),
            }
        }
        let raw = RawSftpSession::new_with_config(
            channel.into_stream(),
            russh_sftp::client::Config {
                request_timeout_secs: 30,
                max_concurrent_writes: 64,
                ..Default::default()
            },
        );
        if raw.init().await?.version != 3 {
            return Err(Failure("SFTP_UNSUPPORTED"));
        }
        Ok(Self {
            raw: Arc::new(raw),
            directory: None,
            transfers: HashMap::new(),
            nonce: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(),
        })
    }

    async fn canonical(&self, path: &str) -> Result<String> {
        let resolved = self
            .raw
            .realpath(path)
            .await?
            .files
            .into_iter()
            .next()
            .map(|file| file.filename)
            .ok_or(Failure("SFTP_NOT_FOUND"))?;
        validate_path(&resolved)?;
        Ok(resolved)
    }

    async fn execute(&mut self, request: Request) -> Result<Value> {
        match request {
            Request::List { path, cursor } => self.list(path, cursor).await,
            Request::CloseList { cursor } => {
                if self.directory.as_ref().is_some_and(|d| d.id == cursor) {
                    self.close_directory().await;
                }
                Ok(json!({"ok":true}))
            },
            Request::Stat { path } => {
                let resolved = self.canonical(&path).await?;
                let attrs = self.raw.stat(&resolved).await?.attrs;
                Ok(entry(&resolved, resolved.rsplit('/').next().unwrap_or(&resolved), &attrs))
            },
            Request::OpenRead { path, offset, revision } => {
                self.open_read(path, offset, revision).await
            },
            Request::Read { transfer, offset, length } => {
                self.read(transfer, offset, length.unwrap_or(CHUNK)).await
            },
            Request::BeginUpload { path, token, offset, resume } => {
                self.begin_upload(path, token, offset, resume).await
            },
            Request::Write { transfer, offset, data } => {
                self.write(transfer, offset, data, CHUNK).await
            },
            Request::WriteBatch { transfer, offset, data } => {
                self.write(transfer, offset, data, BATCH).await
            },
            Request::ResolveUpload { path, token, size, digest } => {
                self.resolve_upload(path, token, size, digest).await
            },
            Request::AbortUpload { path, token } => {
                let (_, temp) = self.upload_paths(&path, &token).await?;
                match self.raw.remove(temp).await {
                    Ok(_) => {},
                    Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {},
                    Err(error) => return Err(error.into()),
                }
                Ok(json!({"ok":true}))
            },
            Request::Commit { transfer } => self.commit(transfer).await,
            Request::Close { transfer } => {
                self.close_transfer(transfer).await?;
                Ok(json!({"ok":true}))
            },
            Request::Mkdir { path } => {
                self.raw.mkdir(path, FileAttributes::empty()).await?;
                Ok(json!({"ok":true}))
            },
            Request::Rename { path, destination, revision: expected } => {
                self.check_revision(&path, &expected).await?;
                self.ensure_absent(&destination).await?;
                self.raw.rename(path, destination).await?;
                Ok(json!({"ok":true}))
            },
            Request::Remove { path, revision: expected } => {
                let attrs = self.check_revision(&path, &expected).await?;
                // LSTAT 决定操作类型：只删除链接本身或空目录，绝不递归遍历。
                if attrs.file_type().is_dir() {
                    self.raw.rmdir(path).await?;
                } else {
                    self.raw.remove(path).await?;
                }
                Ok(json!({"ok":true}))
            },
        }
    }

    async fn check_revision(&self, path: &str, expected: &str) -> Result<FileAttributes> {
        let attrs = self.raw.lstat(path).await?.attrs;
        if revision(&attrs) != expected {
            return Err(Failure("SFTP_CHANGED"));
        }
        Ok(attrs)
    }

    async fn ensure_absent(&self, path: &str) -> Result<()> {
        match self.raw.lstat(path).await {
            Ok(_) => Err(Failure("SFTP_EXISTS")),
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                Ok(())
            },
            Err(error) => Err(error.into()),
        }
    }

    async fn list(&mut self, path: String, cursor: Option<u64>) -> Result<Value> {
        if cursor.is_none() {
            self.close_directory().await;
            let path = self.canonical(&path).await?;
            let handle = self.raw.opendir(&path).await?.handle;
            let id = next_handle_id()?;
            self.directory = Some(Directory {
                id,
                path,
                handle,
                pending: VecDeque::new(),
                touched: Instant::now(),
            });
        }
        let directory = self.directory.as_mut().ok_or(Failure("SFTP_STALE"))?;
        if cursor.is_some_and(|id| id != directory.id || path != directory.path) {
            return Err(Failure("SFTP_STALE"));
        }
        directory.touched = Instant::now();
        let mut rows = Vec::new();
        let mut done = false;
        let mut skipped = 0;
        while rows.len() < DIRECTORY_PAGE {
            if let Some(file) = directory.pending.pop_front() {
                if matches!(file.filename.as_str(), "." | "..") {
                    continue;
                }
                if validate_name(&file.filename).is_err() {
                    skipped += 1;
                    continue;
                }
                let child = join(&directory.path, &file.filename);
                if validate_path(&child).is_err() {
                    skipped += 1;
                    continue;
                }
                rows.push(entry(&child, &file.filename, &file.attrs));
            } else {
                match self.raw.readdir(&directory.handle).await {
                    Ok(page) if !page.files.is_empty() => directory.pending = page.files.into(),
                    Ok(_) => return Err(Failure("SFTP_OPERATION")),
                    Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => {
                        done = true;
                        break;
                    },
                    Err(error) => return Err(error.into()),
                }
            }
        }
        let result = json!({"path":directory.path,"entries":rows,"cursor":if done { None } else { Some(directory.id) },"skipped":skipped});
        if done {
            self.close_directory().await;
        }
        Ok(result)
    }

    async fn open_read(
        &mut self,
        path: String,
        offset: u64,
        expected: Option<String>,
    ) -> Result<Value> {
        if self.transfers.len() >= MAX_HANDLES {
            return Err(Failure("SFTP_LIMIT"));
        }
        let path = self.canonical(&path).await?;
        let attrs = self.raw.stat(&path).await?.attrs;
        if !attrs.file_type().is_file() || attrs.size.is_none() {
            return Err(Failure("SFTP_FILE_TYPE"));
        }
        if offset > attrs.len() || expected.is_some_and(|expected| expected != revision(&attrs)) {
            return Err(Failure("SFTP_CHANGED"));
        }
        let handle = self.raw.open(&path, OpenFlags::READ, FileAttributes::empty()).await?.handle;
        let actual = self.raw.fstat(&handle).await;
        if actual.as_ref().is_ok_and(|actual| revision(&actual.attrs) == revision(&attrs)) {
            let id = next_handle_id()?;
            self.transfers.insert(
                id,
                Transfer {
                    handle: Some(handle),
                    path: path.clone(),
                    temp: None,
                    offset,
                    attrs: attrs.clone(),
                    touched: Instant::now(),
                },
            );
            Ok(
                json!({"transfer":id,"file":entry(&path,path.rsplit('/').next().unwrap_or(&path),&attrs)}),
            )
        } else {
            let _ = self.raw.close(handle).await;
            Err(Failure("SFTP_CHANGED"))
        }
    }

    async fn read(&mut self, id: u64, offset: u64, length: usize) -> Result<Value> {
        let transfer = self.transfers.get_mut(&id).ok_or(Failure("SFTP_STALE"))?;
        if transfer.temp.is_some() || transfer.offset != offset || length == 0 || length > BATCH {
            return Err(Failure("INVALID_INPUT"));
        }
        transfer.touched = Instant::now();
        let handle = transfer.handle.as_ref().ok_or(Failure("SFTP_STALE"))?;
        let length = (transfer.attrs.len() - offset).min(length as u64) as usize;
        let data = transfer::read(&self.raw, handle, offset, length).await?;
        let next = offset + data.len() as u64;
        // Check at batch boundaries, not after every 32 KiB round trip.
        let current = self.raw.fstat(handle).await?.attrs;
        if revision(&current) != revision(&transfer.attrs) {
            return Err(Failure("SFTP_CHANGED"));
        }
        transfer.offset = next;
        Ok(json!({"offset":offset,"next_offset":next,"eof":next == transfer.attrs.len(),
            "data":base64::engine::general_purpose::STANDARD.encode(data)}))
    }

    async fn upload_paths(&self, path: &str, token: &str) -> Result<(String, String)> {
        if token.len() != 32 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Failure("INVALID_INPUT"));
        }
        let (parent, name) = path.rsplit_once('/').ok_or(Failure("INVALID_INPUT"))?;
        validate_name(name)?;
        let parent = self.canonical(if parent.is_empty() { "/" } else { parent }).await?;
        Ok((join(&parent, name), join(&parent, &format!(".pebrel-upload-{token}"))))
    }

    async fn begin_upload(
        &mut self,
        path: String,
        token: Option<String>,
        offset: u64,
        resume: bool,
    ) -> Result<Value> {
        if self.transfers.len() >= MAX_HANDLES {
            return Err(Failure("SFTP_LIMIT"));
        }
        if !resume && offset != 0 {
            return Err(Failure("INVALID_INPUT"));
        }
        let id = next_handle_id()?;
        let token = token.unwrap_or_else(|| format!("{:032x}", self.nonce ^ u128::from(id)));
        let (path, temp) = self.upload_paths(&path, &token).await?;
        self.ensure_absent(&path).await?;
        let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() };
        let mut flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
        if resume {
            match self.raw.lstat(&temp).await {
                Ok(current) => {
                    if !current.attrs.is_regular()
                        || current.attrs.size.is_none_or(|size| size < offset)
                    {
                        return Err(Failure("SFTP_CHANGED"));
                    }
                    flags = OpenFlags::WRITE;
                },
                Err(SftpError::Status(s))
                    if s.status_code == StatusCode::NoSuchFile && offset == 0 => {},
                Err(error) => return Err(error.into()),
            }
        }
        let handle = self.raw.open(&temp, flags, attrs.clone()).await?.handle;
        if resume {
            let reconciled = async {
                let current = self.raw.fstat(&handle).await?.attrs;
                if !current.is_regular() || current.size.is_none_or(|size| size < offset) {
                    return Err(Failure("SFTP_CHANGED"));
                }
                // Rewind unacknowledged, possibly out-of-order writes before replaying the retained batch.
                self.raw
                    .fsetstat(
                        &handle,
                        FileAttributes { size: Some(offset), ..FileAttributes::empty() },
                    )
                    .await?;
                Ok(())
            }
            .await;
            if let Err(error) = reconciled {
                let _ = self.raw.close(handle).await;
                return Err(error);
            }
        }
        self.transfers.insert(
            id,
            Transfer {
                handle: Some(handle),
                path,
                temp: Some(temp),
                offset,
                attrs,
                touched: Instant::now(),
            },
        );
        Ok(json!({"transfer":id,"token":token}))
    }

    async fn resolve_upload(
        &self,
        path: String,
        token: String,
        size: u64,
        digest: String,
    ) -> Result<Value> {
        let (path, temp) = self.upload_paths(&path, &token).await?;
        if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Failure("INVALID_INPUT"));
        }
        match self.raw.lstat(temp).await {
            Ok(_) => return Ok(json!({"committed":false})),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {},
            Err(error) => return Err(error.into()),
        }
        let attrs = self.raw.lstat(&path).await?.attrs;
        if !attrs.is_regular() || attrs.size != Some(size) {
            return Err(Failure("SFTP_CHANGED"));
        }
        let handle = self.raw.open(&path, OpenFlags::READ, FileAttributes::empty()).await?.handle;
        let result = transfer::verify(&self.raw, &handle, size, &digest).await;
        let _ = self.raw.close(handle).await;
        result?;
        if revision(&self.raw.lstat(&path).await?.attrs) != revision(&attrs) {
            return Err(Failure("SFTP_CHANGED"));
        }
        Ok(json!({"committed":true,"path":path}))
    }

    async fn write(&mut self, id: u64, offset: u64, data: String, maximum: usize) -> Result<Value> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Failure("INVALID_INPUT"))?;
        if bytes.is_empty() || bytes.len() > maximum {
            return Err(Failure("INVALID_INPUT"));
        }
        let transfer = self.transfers.get_mut(&id).ok_or(Failure("SFTP_STALE"))?;
        if transfer.temp.is_none() || transfer.offset != offset {
            return Err(Failure("INVALID_INPUT"));
        }
        transfer.touched = Instant::now();
        let handle = transfer.handle.as_ref().ok_or(Failure("SFTP_STALE"))?;
        let next = offset.checked_add(bytes.len() as u64).ok_or(Failure("INVALID_INPUT"))?;
        transfer::write(&self.raw, handle, offset, bytes).await?;
        transfer.offset = next;
        Ok(json!({"next_offset":next}))
    }

    async fn commit(&mut self, id: u64) -> Result<Value> {
        let transfer = self.transfers.get_mut(&id).ok_or(Failure("SFTP_STALE"))?;
        let temp = transfer.temp.as_ref().ok_or(Failure("INVALID_INPUT"))?.clone();
        let path = transfer.path.clone();
        if let Some(handle) = transfer.handle.as_ref() {
            let size = self.raw.fstat(handle).await?.attrs.size;
            self.raw.close(handle).await?;
            transfer.handle = None;
            if size != Some(transfer.offset) {
                return Err(Failure("SFTP_CHANGED"));
            }
        }
        self.ensure_absent(&path).await?;
        // SFTP v3 RENAME 要求目标不存在；不使用会覆盖目标的 posix-rename 扩展。
        self.raw.rename(temp, &path).await?;
        self.transfers.remove(&id);
        Ok(json!({"ok":true,"path":path}))
    }

    async fn close_directory(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = self.raw.close(directory.handle).await;
        }
    }

    async fn close_transfer(&mut self, id: u64) -> Result<()> {
        let mut result = Ok(());
        if let Some(transfer) = self.transfers.remove(&id) {
            if let Some(handle) = transfer.handle {
                result = self.raw.close(handle).await.map(|_| ()).map_err(Failure::from);
            }
            if let Some(temp) = transfer.temp {
                let cleanup = self.raw.remove(temp).await.map(|_| ()).map_err(Failure::from);
                result = result.and(cleanup);
            }
        }
        result
    }

    async fn expire(&mut self) {
        if self.directory.as_ref().is_some_and(|d| d.touched.elapsed().as_secs() > 60) {
            self.close_directory().await;
        }
        let expired = self
            .transfers
            .iter()
            .filter_map(|(id, transfer)| (transfer.touched.elapsed().as_secs() > 60).then_some(*id))
            .collect::<Vec<_>>();
        for id in expired {
            let _ = self.close_transfer(id).await;
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    validate_path(name)?;
    if matches!(name, "." | "..") || name.contains('/') {
        Err(Failure("INVALID_INPUT"))
    } else {
        Ok(())
    }
}

fn join(parent: &str, name: &str) -> String {
    format!("{}/{name}", parent.trim_end_matches('/'))
}

fn revision(attrs: &FileAttributes) -> String {
    format!(
        "{:?}:{:?}:{:?}:{:?}:{:?}",
        attrs.size, attrs.mtime, attrs.permissions, attrs.uid, attrs.gid
    )
}

fn entry(path: &str, name: &str, attrs: &FileAttributes) -> Value {
    let kind = match attrs.file_type() {
        russh_sftp::protocol::FileType::Dir => "directory",
        russh_sftp::protocol::FileType::File => "file",
        russh_sftp::protocol::FileType::Symlink => "symlink",
        _ => "other",
    };
    json!({"path":path,"name":name,"kind":kind,"size":attrs.size,"modified":attrs.mtime,
        "permissions":attrs.permissions,"revision":revision(attrs)})
}
