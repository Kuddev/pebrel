//! In-memory I/O boundary for backup tests. No test calls the native keyring or
//! reads a real backup secret from the process environment.

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env::VarError;
use std::io;

#[derive(Default)]
struct State {
    environment: HashMap<String, Result<String, VarError>>,
    secrets: HashMap<String, Vec<u8>>,
    reads: Vec<String>,
    writes: Vec<(String, String, Vec<u8>)>,
    load_error: Option<&'static str>,
    store_error: Option<&'static str>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn state<T>(operation: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| operation(&mut state.borrow_mut()))
}

struct Reset(State);

impl Drop for Reset {
    fn drop(&mut self) {
        STATE.with(|state| state.replace(std::mem::take(&mut self.0)));
    }
}

fn fixture() -> Reset {
    Reset(STATE.with(|state| state.replace(State::default())))
}

pub(super) fn environment(name: &str) -> Result<String, VarError> {
    state(|state| state.environment.get(name).cloned().unwrap_or(Err(VarError::NotPresent)))
}

pub(super) fn store_with_username(target: &str, username: &str, secret: &[u8]) -> io::Result<()> {
    state(|state| {
        state.writes.push((target.into(), username.into(), secret.into()));
        if let Some(error) = state.store_error {
            return Err(io::Error::other(error));
        }
        state.secrets.insert(target.into(), secret.into());
        Ok(())
    })
}

pub(super) fn load(target: &str) -> io::Result<Option<Vec<u8>>> {
    state(|state| {
        state.reads.push(target.into());
        if let Some(error) = state.load_error {
            return Err(io::Error::other(error));
        }
        Ok(state.secrets.get(target).cloned())
    })
}

const WEBDAV_ENV: &str = "PEBREL_BACKUP_WEBDAV_PASSWORD";
const S3_ENV: &str = "PEBREL_BACKUP_S3_SECRET";

#[test]
fn public_save_and_load_keep_each_protocols_target_username_and_utf8_secret() {
    let _fixture = fixture();
    store_webdav_password("dav-user", " \t口令 dav \n").unwrap();
    store_s3_secret("access-key", " \ts3 secret \n").unwrap();
    assert_eq!(webdav_password().as_deref(), Some("口令 dav"));
    assert_eq!(s3_secret().as_deref(), Some("s3 secret"));
    assert!(has_webdav_password());
    assert!(has_s3_secret());
    assert!(protocol_secret_set(BackupProtocol::WebDav));
    assert!(protocol_secret_set(BackupProtocol::S3));
    state(|state| {
        assert_eq!(
            state.writes,
            [
                (
                    "Pebrel Backup WebDAV Password".into(),
                    "dav-user".into(),
                    "口令 dav".as_bytes().into()
                ),
                ("Pebrel Backup S3 Secret Key".into(), "access-key".into(), b"s3 secret".to_vec()),
            ]
        );
        assert_eq!(&state.reads[..2], [WEBDAV_PASSWORD_TARGET, S3_SECRET_TARGET]);
    });
    // Saving the regular configuration must never persist either secret.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("backup.txt");
    BackupRemoteConfig {
        protocol: BackupProtocol::WebDav,
        webdav_username: "dav-user".into(),
        s3_access_key: "access-key".into(),
        ..Default::default()
    }
    .save_at(&path)
    .unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains("口令 dav"));
    assert!(!text.contains("s3 secret"));
}

#[test]
fn nonempty_environment_wins_without_accessing_the_secure_store() {
    let _fixture = fixture();
    state(|state| {
        state.environment.insert(WEBDAV_ENV.into(), Ok(" \tdav override\n ".into()));
        state.environment.insert(S3_ENV.into(), Ok(" \ts3 override\n ".into()));
        state.load_error = Some("must not access keyring");
    });
    assert_eq!(webdav_password().as_deref(), Some("dav override"));
    assert_eq!(s3_secret().as_deref(), Some("s3 override"));
    state(|state| {
        assert!(state.reads.is_empty());
        assert!(state.writes.is_empty());
    });
}

#[test]
fn empty_missing_or_invalid_environment_falls_back_without_trimming_stored_bytes() {
    let _fixture = fixture();
    state(|state| {
        state.secrets.insert(WEBDAV_PASSWORD_TARGET.into(), " 口令 \t".as_bytes().into());
        state.secrets.insert(S3_SECRET_TARGET.into(), b" stored s3 ".to_vec());
    });
    for value in [
        Err(VarError::NotPresent),
        Ok(String::new()),
        Ok(" \n\t".into()),
        Err(VarError::NotUnicode(std::ffi::OsString::from("synthetic invalid environment"))),
    ] {
        state(|state| {
            state.environment.insert(WEBDAV_ENV.into(), value.clone());
            state.environment.insert(S3_ENV.into(), value);
            state.reads.clear();
        });
        assert_eq!(webdav_password().as_deref(), Some(" 口令 \t"));
        assert_eq!(s3_secret().as_deref(), Some(" stored s3 "));
        state(|state| assert_eq!(state.reads, [WEBDAV_PASSWORD_TARGET, S3_SECRET_TARGET]));
    }
}

#[test]
fn missing_invalid_utf8_and_failed_loads_remain_unavailable() {
    let _fixture = fixture();
    for protocol in [BackupProtocol::WebDav, BackupProtocol::S3] {
        assert!(!protocol_secret_set(protocol));
    }
    state(|state| {
        state.secrets.insert(WEBDAV_PASSWORD_TARGET.into(), vec![0xff]);
        state.secrets.insert(S3_SECRET_TARGET.into(), vec![0xff]);
    });
    assert_eq!(webdav_password(), None);
    assert_eq!(s3_secret(), None);
    state(|state| {
        state.secrets.insert(WEBDAV_PASSWORD_TARGET.into(), b"saved dav".to_vec());
        state.secrets.insert(S3_SECRET_TARGET.into(), b"saved s3".to_vec());
        state.load_error = Some("keyring denied");
    });
    assert!(!has_webdav_password());
    assert!(!has_s3_secret());
    // Preserve the old decoding contract: valid stored bytes are not trimmed,
    // including a deliberately stored empty value.
    state(|state| {
        state.load_error = None;
        state.secrets.insert(WEBDAV_PASSWORD_TARGET.into(), Vec::new());
        state.secrets.insert(S3_SECRET_TARGET.into(), Vec::new());
    });
    assert_eq!(webdav_password(), Some(String::new()));
    assert_eq!(s3_secret(), Some(String::new()));
}

#[test]
fn failed_public_saves_propagate_the_error_and_do_not_claim_credentials_exist() {
    let _fixture = fixture();
    state(|state| state.store_error = Some("synthetic keyring denied"));
    for result in [
        store_webdav_password("dav-user", "dav secret"),
        store_s3_secret("access-key", "s3 secret"),
    ] {
        assert_eq!(result.unwrap_err(), "保存到凭据管理器失败：synthetic keyring denied");
    }
    assert!(!has_webdav_password());
    assert!(!has_s3_secret());
    state(|state| {
        assert!(state.secrets.is_empty());
        assert_eq!(state.writes.len(), 2);
    });
}

fn loopback_child(test: &str) -> bool {
    const CHILD: &str = "PEBREL_BACKUP_CREDENTIAL_LOOPBACK_TEST";
    if std::env::var(CHILD).as_deref() == Ok(test) {
        return true;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args(["--exact", test, "--nocapture"]).env(CHILD, test);
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        WEBDAV_ENV,
        S3_ENV,
    ] {
        child.env_remove(name);
    }
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    false
}

#[test]
fn webdav_wizard_check_save_and_backup_use_the_saved_password_over_loopback() {
    use crate::update_proxy::test_support::{Server, response};
    use base64::Engine as _;

    if !loopback_child(
        "backup_remote::credential_tests::webdav_wizard_check_save_and_backup_use_the_saved_password_over_loopback",
    ) {
        return;
    }
    let _fixture = fixture();
    let name = "pebrel-backup-20261010-010203.nbk";
    let packet = "synthetic encrypted archive";
    let xml = format!(
        "<d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>/dav/{name}</d:href><d:propstat><d:prop><d:getcontentlength>{}</d:getcontentlength></d:prop></d:propstat></d:response></d:multistatus>",
        packet.len()
    );
    let server = Server::start(vec![
        response("207 Multi-Status", "Content-Type: application/xml\r\n", &xml),
        response("207 Multi-Status", "Content-Type: application/xml\r\n", &xml),
        response("201 Created", "", ""),
        response("207 Multi-Status", "Content-Type: application/xml\r\n", &xml),
        response("200 OK", "", packet),
    ]);
    let config = BackupRemoteConfig {
        protocol: BackupProtocol::WebDav,
        webdav_url: format!("http://{}/dav", server.address),
        webdav_username: "dav-user".into(),
        allow_http: true,
        ..Default::default()
    };
    // Same domain calls as the wizard: test the draft without storing it, then
    // save before the inputs are cleared and the first backup starts.
    assert_eq!(snapshot_details(&config, Some("dav-password")).unwrap()[0].name, name);
    state(|state| {
        assert!(state.reads.is_empty());
        assert!(state.writes.is_empty());
    });
    store_webdav_password(&config.webdav_username, " dav-password ").unwrap();
    assert!(protocol_secret_set(config.protocol));
    assert_eq!(snapshot_details(&config, None).unwrap()[0].bytes, Some(packet.len() as u64));
    let (snapshot, _) = push_snapshot(&config, packet.as_bytes()).unwrap();
    assert_eq!(snapshot.bytes, Some(packet.len() as u64));
    assert_eq!(pull_from(&config, Some(name)).unwrap(), (name.into(), packet.as_bytes().into()));

    let requests = server.finish();
    let authorization = format!(
        "authorization: basic {}",
        base64::engine::general_purpose::STANDARD.encode("dav-user:dav-password")
    );
    for (_, headers) in &requests {
        assert!(headers.to_ascii_lowercase().contains(&authorization.to_ascii_lowercase()));
    }
    assert!(requests[0].1.starts_with("PROPFIND /dav/ HTTP/1.1\r\n"));
    assert!(requests[1].1.starts_with("PROPFIND /dav/ HTTP/1.1\r\n"));
    assert!(requests[2].1.starts_with(&format!("PUT /dav/{} HTTP/1.1\r\n", snapshot.name)));
    assert!(requests[3].1.starts_with("PROPFIND /dav/ HTTP/1.1\r\n"));
    assert!(requests[4].1.starts_with(&format!("GET /dav/{name} HTTP/1.1\r\n")));
}

#[test]
fn s3_saved_secret_reaches_the_authenticated_backend_over_loopback() {
    use crate::update_proxy::test_support::{Server, response};

    if !loopback_child(
        "backup_remote::credential_tests::s3_saved_secret_reaches_the_authenticated_backend_over_loopback",
    ) {
        return;
    }
    let _fixture = fixture();
    let server = Server::start(vec![response("200 OK", "", "<ListBucketResult/>")]);
    let config = BackupRemoteConfig {
        protocol: BackupProtocol::S3,
        s3_endpoint: format!("http://{}", server.address),
        s3_region: "us-east-1".into(),
        s3_bucket: "bucket/prefix".into(),
        s3_access_key: "synthetic-access-key".into(),
        allow_http: true,
        ..Default::default()
    };
    store_s3_secret(&config.s3_access_key, " synthetic-secret ").unwrap();
    assert!(protocol_secret_set(config.protocol));
    assert!(snapshot_details(&config, None).unwrap().is_empty());
    let address = server.address;
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    let headers = &requests[0].1;
    let header = |wanted: &str| {
        headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .unwrap()
            .1
            .trim()
    };
    assert!(
        headers.starts_with("GET /bucket/?list-type=2&prefix=prefix%2Fpebrel-backup- HTTP/1.1\r\n")
    );
    // The signing algorithm has its own AWS golden test; here the received
    // signature establishes that the backend used the stored synthetic key.
    assert_eq!(
        header("Authorization"),
        sigv4_authorization(
            "GET",
            &address.to_string(),
            "/bucket/",
            &[("list-type", "2"), ("prefix", "prefix/pebrel-backup-")],
            &sha256_hex(&[]),
            header("x-amz-date"),
            "us-east-1",
            "synthetic-access-key",
            "synthetic-secret",
        )
    );
}
