//! One lazily opened SFTP subsystem per authenticated SSH connection.
//!
//! russh-sftp owns the protocol. This adapter owns bounded directory/transfer handles,
//! upload staging and cancellation; it never handles terminal bytes.

use std::{
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicU64, Ordering},
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

pub(crate) const CHUNK: usize = 32 * 1024;
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
    List { path: String, cursor: Option<u64> },
    CloseList { cursor: u64 },
    Stat { path: String },
    OpenRead { path: String },
    Read { transfer: u64, offset: u64 },
    BeginUpload { path: String },
    Write { transfer: u64, offset: u64, data: String },
    Commit { transfer: u64 },
    Close { transfer: u64 },
    Mkdir { path: String },
    Rename { path: String, destination: String, revision: String },
    Remove { path: String, revision: String },
}

pub(crate) fn parse(text: &str) -> Result<Request> {
    if text.len() > 64 * 1024 {
        return Err(Failure("INVALID_INPUT"));
    }
    let request: Request = serde_json::from_str(text).map_err(|_| Failure("INVALID_INPUT"))?;
    let path = match &request {
        Request::List { path, .. }
        | Request::Stat { path }
        | Request::OpenRead { path }
        | Request::BeginUpload { path }
        | Request::Mkdir { path }
        | Request::Remove { path, .. } => Some(path),
        Request::Rename { path, destination, .. } => {
            validate_path(destination)?;
            Some(path)
        },
        Request::Write { data, .. } if data.len() > CHUNK.div_ceil(3) * 4 => {
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
                let result = tokio::time::timeout(Duration::from_secs(20), async {
                    if files.is_none() { files = Some(Files::open(client).await?); }
                    files.as_mut().expect("SFTP initialized").execute(request).await
                }).await.unwrap_or(Err(Failure("SFTP_TIMEOUT")));
                // 协议超时只回收文件通道，不破坏仍可用的 shell；未确认写入不自动重放。
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
    raw: RawSftpSession,
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
                request_timeout_secs: 10,
                max_concurrent_writes: 1,
                ..Default::default()
            },
        );
        if raw.init().await?.version != 3 {
            return Err(Failure("SFTP_UNSUPPORTED"));
        }
        Ok(Self {
            raw,
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
            Request::OpenRead { path } => self.open_read(path).await,
            Request::Read { transfer, offset } => self.read(transfer, offset).await,
            Request::BeginUpload { path } => self.begin_upload(path).await,
            Request::Write { transfer, offset, data } => self.write(transfer, offset, data).await,
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

    async fn open_read(&mut self, path: String) -> Result<Value> {
        if self.transfers.len() >= MAX_HANDLES {
            return Err(Failure("SFTP_LIMIT"));
        }
        let path = self.canonical(&path).await?;
        let attrs = self.raw.stat(&path).await?.attrs;
        if !attrs.file_type().is_file() || attrs.size.is_none() {
            return Err(Failure("SFTP_FILE_TYPE"));
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
                    offset: 0,
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

    async fn read(&mut self, id: u64, offset: u64) -> Result<Value> {
        let transfer = self.transfers.get_mut(&id).ok_or(Failure("SFTP_STALE"))?;
        if transfer.temp.is_some() || transfer.offset != offset {
            return Err(Failure("INVALID_INPUT"));
        }
        transfer.touched = Instant::now();
        let handle = transfer.handle.as_ref().ok_or(Failure("SFTP_STALE"))?;
        let data = match self.raw.read(handle, offset, CHUNK as u32).await {
            Ok(data) => data.data,
            Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let current = self.raw.fstat(handle).await?.attrs;
        let next = offset + data.len() as u64;
        if revision(&current) != revision(&transfer.attrs)
            || next > transfer.attrs.len()
            || (data.is_empty() && next != transfer.attrs.len())
        {
            return Err(Failure("SFTP_CHANGED"));
        }
        transfer.offset = next;
        Ok(json!({"offset":offset,"next_offset":next,"eof":next == transfer.attrs.len(),
            "data":base64::engine::general_purpose::STANDARD.encode(data)}))
    }

    async fn begin_upload(&mut self, path: String) -> Result<Value> {
        if self.transfers.len() >= MAX_HANDLES {
            return Err(Failure("SFTP_LIMIT"));
        }
        let (parent, name) = path.rsplit_once('/').ok_or(Failure("INVALID_INPUT"))?;
        validate_name(name)?;
        let parent = self.canonical(if parent.is_empty() { "/" } else { parent }).await?;
        let path = join(&parent, name);
        self.ensure_absent(&path).await?;
        let id = next_handle_id()?;
        let temp = join(&parent, &format!(".pebrel-upload-{:x}-{id}", self.nonce));
        let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() };
        let handle = self
            .raw
            .open(&temp, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE, attrs.clone())
            .await?
            .handle;
        self.transfers.insert(
            id,
            Transfer {
                handle: Some(handle),
                path,
                temp: Some(temp),
                offset: 0,
                attrs,
                touched: Instant::now(),
            },
        );
        Ok(json!({"transfer":id}))
    }

    async fn write(&mut self, id: u64, offset: u64, data: String) -> Result<Value> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Failure("INVALID_INPUT"))?;
        if bytes.is_empty() || bytes.len() > CHUNK {
            return Err(Failure("INVALID_INPUT"));
        }
        let transfer = self.transfers.get_mut(&id).ok_or(Failure("SFTP_STALE"))?;
        if transfer.temp.is_none() || transfer.offset != offset {
            return Err(Failure("INVALID_INPUT"));
        }
        transfer.touched = Instant::now();
        let handle = transfer.handle.as_ref().ok_or(Failure("SFTP_STALE"))?;
        let next = offset.checked_add(bytes.len() as u64).ok_or(Failure("INVALID_INPUT"))?;
        self.raw.write(handle, offset, bytes).await?;
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
