use super::*;
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, Status, StatusCode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
enum Node {
    Directory,
    File(Vec<u8>),
    Link(String),
}

impl Node {
    fn attrs(&self) -> FileAttributes {
        FileAttributes {
            permissions: Some(match self {
                Self::Directory => 0o40755,
                Self::File(_) => 0o100644,
                Self::Link(_) => 0o120777,
            }),
            size: Some(match self {
                Self::File(data) => data.len() as u64,
                _ => 0,
            }),
            ..FileAttributes::empty()
        }
    }
}

type Store = Arc<Mutex<BTreeMap<String, Node>>>;

struct Peer {
    store: Store,
    handles: HashMap<String, String>,
    read_dirs: HashSet<String>,
}

fn status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}

impl Peer {
    fn real(&self, path: &str) -> Result<String, StatusCode> {
        let store = self.store.lock().unwrap();
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {},
                ".." => {
                    parts.pop();
                },
                part => parts.push(part),
            }
        }
        let mut path = format!("/{}", parts.join("/"));
        for _ in 0..16 {
            match store.get(&path).ok_or(StatusCode::NoSuchFile)? {
                Node::Link(target) => path = target.clone(),
                _ => return Ok(path),
            }
        }
        Err(StatusCode::Failure)
    }
}

impl russh_sftp::server::Handler for Peer {
    type Error = StatusCode;
    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        Ok(Name { id, files: vec![File::dummy(self.real(&path)?)] })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let path = self.real(&path)?;
        self.lstat(id, path).await
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let store = self.store.lock().unwrap();
        Ok(Attrs { id, attrs: store.get(&path).ok_or(StatusCode::NoSuchFile)?.attrs() })
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _: FileAttributes,
    ) -> Result<Status, StatusCode> {
        let mut store = self.store.lock().unwrap();
        if store.contains_key(&path) {
            return Err(StatusCode::Failure);
        }
        store.insert(path, Node::Directory);
        Ok(status(id))
    }

    async fn open(
        &mut self,
        id: u32,
        path: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let mut store = self.store.lock().unwrap();
        if flags.contains(OpenFlags::CREATE) {
            store.entry(path.clone()).or_insert(Node::File(Vec::new()));
        }
        let node = store.get_mut(&path).ok_or(StatusCode::NoSuchFile)?;
        if !matches!(node, Node::File(_)) {
            return Err(StatusCode::Failure);
        }
        if flags.contains(OpenFlags::TRUNCATE) {
            *node = Node::File(Vec::new());
        }
        let handle = format!("file-{id}");
        self.handles.insert(handle.clone(), path);
        Ok(Handle { id, handle })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        self.lstat(id, self.handles.get(&handle).ok_or(StatusCode::NoSuchFile)?.clone()).await
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        length: u32,
    ) -> Result<Data, StatusCode> {
        let path = self.handles.get(&handle).ok_or(StatusCode::NoSuchFile)?;
        let store = self.store.lock().unwrap();
        let Some(Node::File(data)) = store.get(path) else { return Err(StatusCode::Failure) };
        let start = offset as usize;
        if start >= data.len() {
            return Err(StatusCode::Eof);
        }
        Ok(Data { id, data: data[start..data.len().min(start + length as usize)].to_vec() })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let path = self.handles.get(&handle).ok_or(StatusCode::NoSuchFile)?;
        let mut store = self.store.lock().unwrap();
        let Some(Node::File(target)) = store.get_mut(path) else { return Err(StatusCode::Failure) };
        let start = offset as usize;
        target.resize(target.len().max(start + data.len()), 0);
        target[start..start + data.len()].copy_from_slice(&data);
        Ok(status(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.handles.remove(&handle);
        self.read_dirs.remove(&handle);
        Ok(status(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        if !matches!(self.store.lock().unwrap().get(&path), Some(Node::Directory)) {
            return Err(StatusCode::Failure);
        }
        let handle = format!("dir-{id}");
        self.handles.insert(handle.clone(), path);
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        if !self.read_dirs.insert(handle.clone()) {
            return Err(StatusCode::Eof);
        }
        let prefix = format!("{}/", self.handles.get(&handle).ok_or(StatusCode::NoSuchFile)?);
        let store = self.store.lock().unwrap();
        let files = store
            .iter()
            .filter_map(|(path, node)| {
                let name = path.strip_prefix(&prefix)?;
                if name.is_empty() || name.contains('/') {
                    return None;
                }
                Some(File {
                    filename: name.to_owned(),
                    longname: name.to_owned(),
                    attrs: node.attrs(),
                })
            })
            .collect();
        Ok(Name { id, files })
    }

    async fn remove(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        self.store.lock().unwrap().remove(&path).ok_or(StatusCode::NoSuchFile)?;
        Ok(status(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        if self.store.lock().unwrap().keys().any(|key| key.starts_with(&format!("{path}/"))) {
            return Err(StatusCode::Failure);
        }
        self.remove(id, path).await
    }
}

async fn fixture() -> (Store, SftpSession) {
    let store = Arc::new(Mutex::new(BTreeMap::from([
        ("/".into(), Node::Directory),
        ("/tmp".into(), Node::Directory),
        ("/tmp/target".into(), Node::Directory),
        ("/outside".into(), Node::Directory),
        ("/outside/keep".into(), Node::File(b"keep".to_vec())),
    ])));
    let (client, server) = tokio::io::duplex(128 * 1024);
    tokio::spawn(russh_sftp::server::run(
        server,
        Peer { store: store.clone(), handles: HashMap::new(), read_dirs: HashSet::new() },
    ));
    let session = SftpSession::new(client).await.unwrap();
    (store, session)
}

fn check(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(5), future).await.unwrap();
    });
}

#[test]
fn ssh_sync_guard_normalizes_dot_and_parent_components() {
    for path in ["/", "/var", "/var/..", "/var/../", "/././", "/tmp/safe/../..", "x/../.."] {
        assert!(check_sync_target(path).is_err(), "{path}");
    }
    assert!(check_sync_target("/tmp/safe/").is_ok());
}

#[test]
fn ssh_sync_realpath_guard_refuses_aliases_before_writing() {
    check(async {
        let (store, session) = fixture().await;
        store.lock().unwrap().insert("/tmp/alias".into(), Node::Link("/".into()));
        let before = store.lock().unwrap().len();
        assert!(sync_base(&session, "/tmp/alias", true).await.is_err());
        assert!(sync_base(&session, "/tmp/alias/new", true).await.is_err());
        assert_eq!(store.lock().unwrap().len(), before);
        assert_eq!(sync_base(&session, "/tmp/new/deeper", true).await.unwrap(), "/tmp/new/deeper");
    });
}

#[test]
fn ssh_directory_roundtrip_keeps_empty_directories_and_sync_root() {
    check(async {
        let (store, session) = fixture().await;
        let local = tempfile::tempdir().unwrap();
        let source = local.path().join("source");
        std::fs::create_dir_all(source.join("nested/empty")).unwrap();
        std::fs::write(source.join("nested/payload.txt"), b"payload").unwrap();
        for delete in [false, true] {
            let base = sync_base(&session, "/tmp/target", delete).await.unwrap();
            let (files, bytes, expected) =
                upload_directory(&session, &source, &base).await.unwrap();
            assert_eq!((files, bytes), (1, 7));
            if delete {
                store
                    .lock()
                    .unwrap()
                    .insert("/tmp/target/stale".into(), Node::File(b"old".to_vec()));
                store
                    .lock()
                    .unwrap()
                    .insert("/tmp/target/link".into(), Node::Link("/outside".into()));
                assert_eq!(
                    remove_unexpected(&session, &base, String::new(), &expected).await.unwrap(),
                    2
                );
            }
            let store = store.lock().unwrap();
            assert!(matches!(store.get("/tmp/target/nested/empty"), Some(Node::Directory)));
            assert!(!store.contains_key("/tmp/target/source"));
            assert!(store.contains_key("/outside/keep"));
        }
        let target = local.path().join("download");
        assert_eq!(download_directory(&session, "/tmp/target", &target).await.unwrap(), (1, 7));
        assert_eq!(std::fs::read(target.join("nested/payload.txt")).unwrap(), b"payload");
        assert!(target.join("nested/empty").is_dir());
        let empty = local.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(upload_directory(&session, &empty, "/tmp/new-empty").await.unwrap().0, 0);
        assert!(store.lock().unwrap().contains_key("/tmp/new-empty"));
    });
}

#[test]
fn ssh_download_rejects_remote_names_that_escape_local_directory() {
    for name in ["..", ".", "/absolute", "../escape", "a\\escape", "C:drive", "nul\0name"] {
        assert!(download_child(Path::new("destination"), name).is_err());
    }
    assert_eq!(
        download_child(Path::new("destination"), "payload.txt").unwrap(),
        Path::new("destination").join("payload.txt")
    );
}
