use super::*;
use crate::ssh_sftp::test_peer as peer;

fn run(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

use std::future::Future;

fn file(root: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn native_file_batch_keeps_order_contents_names_and_private_permissions() {
    let root = tempfile::tempdir().unwrap();
    let large: Vec<_> = (0..100_000).map(|index| (index % 251) as u8).collect();
    let first = file(root.path(), "报告 'quoted'.pdf", &large);
    let second = file(root.path(), "notes.txt", b"second");
    run(async {
        let (store, session) = peer::connect(b"untouched").await;
        let uploaded =
            upload_on(session, Some("/project".into()), vec![first, second], &Operation::default())
                .await
                .unwrap();
        assert_eq!(uploaded.paths(), ["/project/报告 'quoted'.pdf", "/project/notes.txt"]);
        uploaded.keep();
        let store = store.lock().unwrap();
        assert_eq!(store.files["/project/报告 'quoted'.pdf"].data, large);
        assert_eq!(store.files["/project/notes.txt"].data, b"second");
        assert_eq!(store.files["/project/notes.txt"].permissions & 0o777, 0o600);
        assert_eq!(store.files["/config"].data, b"untouched");
    });
}

#[test]
fn unknown_cwd_uses_the_same_sessions_home_directory() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path(), "report.txt", b"home");
    run(async {
        let (store, session) = peer::connect(b"").await;
        store.lock().unwrap().aliases.insert(".".into(), "/home/test".into());
        let uploaded = upload_on(session, None, vec![path], &Operation::default()).await.unwrap();
        assert_eq!(uploaded.paths(), ["/home/test/report.txt"]);
        uploaded.keep();
    });
}

#[test]
fn collision_preserves_the_existing_file_and_rolls_back_earlier_uploads() {
    let root = tempfile::tempdir().unwrap();
    let first = file(root.path(), "new.txt", b"first");
    let conflict = file(root.path(), "config", b"replacement");
    run(async {
        let (store, session) = peer::connect(b"original").await;
        assert!(
            upload_on(session, Some("/".into()), vec![first, conflict], &Operation::default())
                .await
                .is_err()
        );
        let store = store.lock().unwrap();
        assert_eq!(store.files.len(), 1);
        assert_eq!(store.files["/config"].data, b"original");
    });
}

#[test]
fn a_later_write_failure_removes_all_files_created_by_the_batch() {
    let root = tempfile::tempdir().unwrap();
    let first = file(root.path(), "first.txt", b"first");
    let second = file(root.path(), "second.txt", b"second");
    run(async {
        let (store, session) = peer::connect(b"original").await;
        store.lock().unwrap().writes_before_failure = Some(1);
        assert!(
            upload_on(session, Some("/".into()), vec![first, second], &Operation::default())
                .await
                .is_err()
        );
        assert_eq!(store.lock().unwrap().files.len(), 1);
    });
}

#[test]
fn cancellation_during_writes_rolls_back_partial_remote_files() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path(), "large.bin", &vec![42; 1024 * 1024]);
    run(async {
        let (store, session) = peer::connect(b"original").await;
        let operation = Operation::default();
        let cancel = operation.clone();
        store.lock().unwrap().after_write = Some(Box::new(move || cancel.cancel()));
        assert!(upload_on(session, Some("/".into()), vec![path], &operation).await.is_err());
        assert!(operation.is_cancelled());
        assert_eq!(store.lock().unwrap().files.len(), 1);
    });
}

#[test]
fn directories_duplicate_names_and_relative_paths_are_rejected_before_upload() {
    let root = tempfile::tempdir().unwrap();
    let first = file(root.path(), "same.txt", b"first");
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let second = file(&nested, "same.txt", b"second");
    run(async {
        let (store, session) = peer::connect(b"original").await;
        for paths in
            [vec![], vec![nested], vec![first, second], vec![PathBuf::from("relative.txt")]]
        {
            assert!(
                upload_on(session.clone(), Some("/".into()), paths, &Operation::default())
                    .await
                    .is_err()
            );
            assert_eq!(store.lock().unwrap().files.len(), 1);
        }
    });
}

#[test]
fn a_file_changed_during_upload_is_not_accepted() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path(), "changing.txt", b"original");
    run(async {
        let (store, session) = peer::connect(b"untouched").await;
        let changed = path.clone();
        store.lock().unwrap().after_write = Some(Box::new(move || {
            std::fs::write(changed, b"changed while uploading").unwrap();
        }));
        assert!(
            upload_on(session, Some("/".into()), vec![path], &Operation::default()).await.is_err()
        );
        assert_eq!(store.lock().unwrap().files.len(), 1);
    });
}

#[test]
fn abandoning_a_completed_result_removes_only_its_unaccepted_files() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path(), "unaccepted.txt", b"uploaded");
    run(async {
        let (store, session) = peer::connect(b"original").await;
        let uploaded =
            upload_on(session, Some("/".into()), vec![path], &Operation::default()).await.unwrap();
        assert_eq!(store.lock().unwrap().files.len(), 2);
        drop(uploaded);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if store.lock().unwrap().files.len() == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(store.lock().unwrap().files["/config"].data, b"original");
    });
}
