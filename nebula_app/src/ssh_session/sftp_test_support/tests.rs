//! Exercise the production session/channel/controller against real loopback SSH.
use super::*;
use crate::ssh_sftp::{SftpConflictPolicy, SftpController, SftpPhase, SftpTransferOptions};

async fn finished(controller: &SftpController) -> crate::ssh_sftp::SftpSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = controller.snapshot();
            if matches!(snapshot.phase, SftpPhase::Ready | SftpPhase::Error | SftpPhase::Cancelled)
            {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("SFTP controller timed out")
}

#[test]
fn owned_sftp_session_uses_real_channels_and_never_falls_back_after_close() {
    runtime().unwrap().block_on(async {
        let fixture = Fixture::new().await;
        fixture
            .store
            .lock()
            .unwrap()
            .files
            .insert("/target/中文 space.txt".into(), Node::new(b"bytes"));
        let entries = crate::ssh_sftp::list_dir(&fixture.destination, "/target").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "中文 space.txt");
        assert_eq!(entries[0].size, 5);
        let destination = fixture.destination.clone();
        drop(fixture);
        let error = match open_sftp(&destination).await {
            Err(error) => error.to_string(),
            Ok(_) => panic!("closed fixture reused a session"),
        };
        assert!(error.contains("owned SFTP fixture is closed"), "{error}");
    });
}

#[test]
fn controller_uploads_unicode_bytes_and_preserves_existing_conflict_contracts() {
    runtime().unwrap().block_on(async {
        let selected = Fixture::new().await;
        let other = Fixture::new().await;
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("中文 space.txt");
        let original = b"original remote";
        let bytes = "local\0bytes\n完整内容".as_bytes();
        std::fs::write(&source, bytes).unwrap();
        let remote = "/target/中文 space.txt";
        selected.store.lock().unwrap().files.insert(remote.into(), Node::new(original));
        other.store.lock().unwrap().files.insert(remote.into(), Node::new(b"other host"));
        let controller =
            SftpController::new_at(&selected.destination, "/target", Arc::new(|| {})).unwrap();
        assert_eq!(finished(&controller).await.phase, SftpPhase::Ready);
        for policy in
            [SftpConflictPolicy::Skip, SftpConflictPolicy::KeepBoth, SftpConflictPolicy::Overwrite]
        {
            controller.upload_paths_with_options(
                vec![source.clone()],
                SftpTransferOptions { conflict: policy, skip_unchanged: false },
            );
            let snapshot = finished(&controller).await;
            assert_eq!(snapshot.phase, SftpPhase::Ready, "{:?}", snapshot.error);
            let store = selected.store.lock().unwrap();
            assert_eq!(
                store.files[remote].data,
                if policy == SftpConflictPolicy::Overwrite { bytes } else { original }
            );
            if policy == SftpConflictPolicy::KeepBoth {
                assert!(
                    store.files.iter().any(|(path, node)| path != remote && node.data == bytes)
                );
            }
            assert!(
                !store
                    .files
                    .keys()
                    .any(|path| path.contains("nebula-upload") || path.contains("nebula-backup"))
            );
        }
        assert_eq!(other.store.lock().unwrap().files[remote].data, b"other host");
        selected.store.lock().unwrap().fail_writes = true;
        std::fs::write(&source, b"must not publish").unwrap();
        controller.upload_paths_with_options(vec![source.clone()], SftpTransferOptions::default());
        assert_eq!(finished(&controller).await.phase, SftpPhase::Error);
        assert_eq!(selected.store.lock().unwrap().files[remote].data, bytes);
        selected.store.lock().unwrap().fail_writes = false;
        let folder = directory.path().join("目录 folder");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/second.txt"), b"nested bytes").unwrap();
        controller.upload_paths_with_options(vec![folder], SftpTransferOptions::default());
        assert_eq!(finished(&controller).await.phase, SftpPhase::Ready);
        assert_eq!(
            selected.store.lock().unwrap().files["/target/目录 folder/nested/second.txt"].data,
            b"nested bytes"
        );
        assert!(!other.store.lock().unwrap().files.contains_key("/target/目录 folder"));
        controller.upload_paths_with_options(vec![source], SftpTransferOptions::default());
        controller.cancel();
        assert_eq!(finished(&controller).await.phase, SftpPhase::Cancelled);
        assert_eq!(selected.store.lock().unwrap().files[remote].data, bytes);
    });
}
