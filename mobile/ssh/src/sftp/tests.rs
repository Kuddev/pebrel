use super::*;
use bytes::Bytes;
use russh_sftp::protocol::{Attrs, Data, File, Handle, Name, Packet, Version};
use sha2::{Digest, Sha256};
use std::sync::{Mutex, atomic::AtomicUsize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Peer {
    files: HashMap<String, Vec<u8>>,
    held: Vec<Packet>,
    window: usize,
    short_reads: bool,
    peak: AtomicUsize,
    reads: usize,
    stats: usize,
}

impl Peer {
    fn attrs(&self, id: u32, path: &str) -> Packet {
        match self.files.get(path) {
            Some(bytes) => Packet::Attrs(Attrs {
                id,
                attrs: FileAttributes {
                    size: Some(bytes.len() as u64),
                    permissions: Some(0o100600),
                    mtime: Some(123),
                    ..FileAttributes::empty()
                },
            }),
            None => Packet::error(id, StatusCode::NoSuchFile),
        }
    }
    fn reply(&mut self, packet: Packet) -> Vec<Packet> {
        let pipelined = matches!(packet, Packet::Read(_) | Packet::Write(_));
        let reply = match packet {
            Packet::Init(_) => Packet::Version(Version::new()),
            Packet::RealPath(p) => {
                Packet::Name(Name { id: p.id, files: vec![File::dummy(p.path)] })
            },
            Packet::Stat(p) => self.attrs(p.id, &p.path),
            Packet::Lstat(p) => self.attrs(p.id, &p.path),
            Packet::Fstat(p) => {
                self.stats += 1;
                self.attrs(p.id, &p.handle)
            },
            Packet::Open(p) => {
                if p.pflags.contains(OpenFlags::EXCLUDE) && self.files.contains_key(&p.filename) {
                    Packet::error(p.id, StatusCode::Failure)
                } else if !self.files.contains_key(&p.filename)
                    && !p.pflags.contains(OpenFlags::CREATE)
                {
                    Packet::error(p.id, StatusCode::NoSuchFile)
                } else {
                    self.files.entry(p.filename.clone()).or_default();
                    Packet::Handle(Handle { id: p.id, handle: p.filename })
                }
            },
            Packet::Read(p) => {
                assert!(p.len as usize <= CHUNK);
                self.reads += 1;
                let bytes = &self.files[&p.handle];
                let start = p.offset as usize;
                let count =
                    if self.short_reads { (p.len as usize).min(701) } else { p.len as usize };
                if start >= bytes.len() {
                    Packet::error(p.id, StatusCode::Eof)
                } else {
                    Packet::Data(Data {
                        id: p.id,
                        data: bytes[start..(start + count).min(bytes.len())].to_vec(),
                    })
                }
            },
            Packet::Write(p) => {
                assert!(p.data.len() <= CHUNK);
                let bytes = self.files.get_mut(&p.handle).unwrap();
                let start = p.offset as usize;
                bytes.resize(bytes.len().max(start + p.data.len()), 0);
                bytes[start..start + p.data.len()].copy_from_slice(&p.data);
                Packet::error(p.id, StatusCode::Ok)
            },
            Packet::FSetStat(p) => {
                self.files.get_mut(&p.handle).unwrap().resize(p.attrs.size.unwrap() as usize, 0);
                Packet::error(p.id, StatusCode::Ok)
            },
            Packet::Close(p) => Packet::error(p.id, StatusCode::Ok),
            Packet::Remove(p) => Packet::error(
                p.id,
                if self.files.remove(&p.filename).is_some() {
                    StatusCode::Ok
                } else {
                    StatusCode::NoSuchFile
                },
            ),
            Packet::Rename(p) => {
                if self.files.contains_key(&p.newpath) {
                    Packet::error(p.id, StatusCode::Failure)
                } else {
                    let bytes = self.files.remove(&p.oldpath).unwrap();
                    self.files.insert(p.newpath, bytes);
                    Packet::error(p.id, StatusCode::Ok)
                }
            },
            _ => panic!("unexpected request"),
        };
        if pipelined && self.window > 1 {
            self.held.push(reply);
            self.peak.fetch_max(self.held.len(), Ordering::Relaxed);
            if self.held.len() < self.window {
                return Vec::new();
            }
            let mut replies = std::mem::take(&mut self.held);
            replies.reverse();
            replies
        } else {
            vec![reply]
        }
    }
}

async fn connect(peer: Arc<Mutex<Peer>>) -> Files {
    let (client, mut server) = tokio::io::duplex(BATCH * 2);
    tokio::spawn(async move {
        while let Ok(length) = server.read_u32().await {
            let mut bytes = vec![0; length as usize];
            if server.read_exact(&mut bytes).await.is_err() {
                break;
            }
            let request = Packet::try_from(&mut Bytes::from(bytes)).unwrap();
            let replies = peer.lock().unwrap().reply(request);
            for reply in replies {
                if server.write_all(&Bytes::try_from(reply).unwrap()).await.is_err() {
                    return;
                }
            }
        }
    });
    let raw = RawSftpSession::new_with_config(
        client,
        russh_sftp::client::Config { request_timeout_secs: 2, ..Default::default() },
    );
    raw.init().await.unwrap();
    Files { raw: Arc::new(raw), directory: None, transfers: HashMap::new(), nonce: 1 }
}

fn data() -> Vec<u8> {
    (0..BATCH).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn both_directions_fill_the_window_and_reassemble_reversed_replies() {
    let bytes = data();
    let peer = Arc::new(Mutex::new(Peer { window: 64, ..Peer::default() }));
    peer.lock().unwrap().files.insert("/source".into(), bytes.clone());
    let mut files = connect(peer.clone()).await;
    let opened = files.open_read("/source".into(), 0, None).await.unwrap();
    let result = files.read(opened["transfer"].as_u64().unwrap(), 0, BATCH).await.unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(result["data"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(peer.lock().unwrap().peak.load(Ordering::Relaxed), 64);
    assert_eq!(peer.lock().unwrap().stats, 2, "one OPEN verification and one batch verification");
    let opened = files.begin_upload("/target".into(), None, 0, false).await.unwrap();
    let id = opened["transfer"].as_u64().unwrap();
    files
        .write(id, 0, base64::engine::general_purpose::STANDARD.encode(&bytes), BATCH)
        .await
        .unwrap();
    files.commit(id).await.unwrap();
    assert_eq!(peer.lock().unwrap().files["/target"], bytes);
}

#[tokio::test]
async fn short_reads_are_filled_and_changed_files_cannot_resume() {
    let peer = Arc::new(Mutex::new(Peer { short_reads: true, ..Peer::default() }));
    peer.lock().unwrap().files.insert("/source".into(), vec![7; CHUNK + 77]);
    let mut files = connect(peer.clone()).await;
    let opened = files.open_read("/source".into(), 123, None).await.unwrap();
    let result = files.read(opened["transfer"].as_u64().unwrap(), 123, BATCH).await.unwrap();
    assert_eq!(result["next_offset"], CHUNK + 77);
    assert!(peer.lock().unwrap().reads > 2);
    peer.lock().unwrap().files.get_mut("/source").unwrap().push(9);
    assert_eq!(
        files
            .open_read(
                "/source".into(),
                123,
                Some(opened["file"]["revision"].as_str().unwrap().into())
            )
            .await
            .unwrap_err(),
        Failure("SFTP_CHANGED")
    );
}

#[tokio::test]
async fn upload_resume_rewinds_ambiguous_bytes_and_reconciles_lost_commit_ack() {
    let peer = Arc::new(Mutex::new(Peer::default()));
    let token = "0123456789abcdef0123456789abcdef".to_owned();
    let mut old = connect(peer.clone()).await;
    let opened = old.begin_upload("/target".into(), Some(token.clone()), 0, false).await.unwrap();
    old.write(opened["transfer"].as_u64().unwrap(), 0, "YWJjZGVm".into(), CHUNK).await.unwrap();
    drop(old); // The peer has six bytes, but only three were confirmed before disconnection.
    let mut files = connect(peer.clone()).await;
    let opened = files.begin_upload("/target".into(), Some(token.clone()), 3, true).await.unwrap();
    assert_eq!(peer.lock().unwrap().files[&format!("/.pebrel-upload-{token}")], b"abc");
    let id = opened["transfer"].as_u64().unwrap();
    files.write(id, 3, "ZGVm".into(), CHUNK).await.unwrap();
    files.commit(id).await.unwrap();
    let digest = format!("{:x}", Sha256::digest(b"abcdef"));
    assert_eq!(
        files.resolve_upload("/target".into(), token.clone(), 6, digest).await.unwrap()["committed"],
        true
    );
    assert_eq!(
        files.resolve_upload("/target".into(), token, 6, "0".repeat(64)).await.unwrap_err(),
        Failure("SFTP_CHANGED")
    );
    assert_eq!(peer.lock().unwrap().files["/target"], b"abcdef");
}

#[test]
fn batch_limits_and_resume_identity_reject_malformed_requests() {
    assert!(
        parse(&format!(
            r#"{{"op":"write_batch","transfer":1,"offset":0,"data":"{}"}}"#,
            "x".repeat(BATCH.div_ceil(3) * 4 + 1)
        ))
        .is_err()
    );
    assert!(
        parse(&format!(r#"{{"op":"read","transfer":1,"offset":0,"length":{}}}"#, BATCH + 1))
            .is_err()
    );
    assert!(parse(r#"{"op":"read","transfer":1,"offset":0,"length":0}"#).is_err());
}
