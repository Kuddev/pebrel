//! Exercise transfer integrity and cancellation through the real SFTP codec.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use russh_sftp::protocol::{Data, FileAttributes, Handle, Status, StatusCode};

use super::*;

#[derive(Default)]
struct Progress {
    bytes: AtomicU64,
    cancelled: AtomicBool,
}

impl TransferObserver for Progress {
    fn advance(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::SeqCst);
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Files {
    data: Vec<u8>,
    short_reads: bool,
    reject_writes: bool,
    cancel_after_read: Option<Arc<Progress>>,
}

struct Peer {
    files: Arc<std::sync::Mutex<Files>>,
    handles: BTreeSet<String>,
}

fn success(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}

impl russh_sftp::server::Handler for Peer {
    type Error = StatusCode;
    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        _: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        if flags.contains(OpenFlags::TRUNCATE) {
            self.files.lock().unwrap().data.clear();
        }
        let handle = id.to_string();
        self.handles.insert(handle.clone());
        Ok(Handle { id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        length: u32,
    ) -> Result<Data, StatusCode> {
        if !self.handles.contains(&handle) {
            return Err(StatusCode::Failure);
        }
        assert!(length as usize <= TRANSFER_CHUNK, "local buffering must not grow packets");
        let mut files = self.files.lock().unwrap();
        if let Some(progress) = files.cancel_after_read.take() {
            progress.cancelled.store(true, Ordering::SeqCst);
        }
        let offset = offset as usize;
        if offset >= files.data.len() {
            return Err(StatusCode::Eof);
        }
        let length = if files.short_reads { (length as usize).min(1013) } else { length as usize };
        let end = (offset + length).min(files.data.len());
        Ok(Data { id, data: files.data[offset..end].to_vec() })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        if !self.handles.contains(&handle) {
            return Err(StatusCode::Failure);
        }
        assert!(data.len() <= TRANSFER_CHUNK);
        let mut files = self.files.lock().unwrap();
        if files.reject_writes {
            return Err(StatusCode::PermissionDenied);
        }
        let start = offset as usize;
        let end = start + data.len();
        if end > files.data.len() {
            files.data.resize(end, 0);
        }
        files.data[start..end].copy_from_slice(&data);
        Ok(success(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.handles.remove(&handle);
        Ok(success(id))
    }
}

async fn peer() -> (Arc<std::sync::Mutex<Files>>, SftpSession) {
    let files = Arc::new(std::sync::Mutex::new(Files::default()));
    let (client, server) = tokio::io::duplex(128 * 1024);
    tokio::spawn(russh_sftp::server::run(
        server,
        Peer { files: files.clone(), handles: BTreeSet::new() },
    ));
    let session = SftpSession::new_with_config(client, crate::ssh_sftp::limits::session_config())
        .await
        .unwrap();
    (files, session)
}

fn run(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), future).await.unwrap()
        });
}

#[test]
fn buffered_transfers_preserve_empty_short_and_segmented_files() {
    run(async {
        let (files, session) = peer().await;
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        for size in [0, 7, TRANSFER_CHUNK + 17, 2 * 1024 * 1024 + 123] {
            let data: Vec<u8> = (0..size).map(|i| ((i * 37 + i / 113) % 251) as u8).collect();
            std::fs::write(&source, &data).unwrap();
            let uploaded = Progress::default();
            upload_stream(&session, &source, "remote", &uploaded).await.unwrap();
            assert_eq!(files.lock().unwrap().data, data);
            assert_eq!(uploaded.bytes.load(Ordering::SeqCst), size as u64);
            files.lock().unwrap().short_reads = true;
            let downloaded = Progress::default();
            let result =
                download_segmented(&session, "remote", size as u64, &target, 4, &downloaded)
                    .await
                    .unwrap();
            assert_eq!((result.written, result.contiguous), (size as u64, size as u64));
            assert_eq!(downloaded.bytes.load(Ordering::SeqCst), size as u64);
            assert_eq!(std::fs::read(&target).unwrap(), data);
        }
        session.close().await.unwrap();
    });
}

#[test]
fn early_eof_never_reports_a_complete_download() {
    run(async {
        let (files, session) = peer().await;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        files.lock().unwrap().data = vec![42; TRANSFER_CHUNK + 17];
        for window in [1, 4, 64] {
            let downloaded = Progress::default();
            let result = download_segmented(
                &session,
                "remote",
                2 * LOCAL_IO_BUFFER as u64,
                &target,
                window,
                &downloaded,
            )
            .await;
            let error = result.err().expect("a short source must not be published as success");
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
        session.close().await.unwrap();
    });
}

#[test]
fn cancellation_does_not_count_data_waiting_in_the_local_buffer() {
    run(async {
        let (files, session) = peer().await;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let progress = Arc::new(Progress::default());
        {
            let mut files = files.lock().unwrap();
            files.data = vec![42; 2 * LOCAL_IO_BUFFER];
            files.cancel_after_read = Some(progress.clone());
        }
        let result = download_segmented(
            &session,
            "remote",
            2 * LOCAL_IO_BUFFER as u64,
            &target,
            1,
            progress.as_ref(),
        )
        .await;
        let error = result.err().expect("cancelled transfer must fail");
        assert_eq!(error.downcast_ref::<io::Error>().unwrap().kind(), io::ErrorKind::Interrupted);
        assert_eq!(progress.bytes.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::metadata(&target).unwrap().len(), 0);
        session.close().await.unwrap();
    });
}

#[test]
fn buffered_upload_still_propagates_server_write_errors() {
    run(async {
        let (files, session) = peer().await;
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::write(&source, [42; TRANSFER_CHUNK]).unwrap();
        files.lock().unwrap().reject_writes = true;
        assert!(upload_stream(&session, &source, "remote", &Progress::default()).await.is_err());
        assert!(files.lock().unwrap().data.is_empty());
        session.close().await.unwrap();
    });
}
