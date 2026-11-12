use super::*;
#[path = "../document/tests/peer.rs"]
mod peer;

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

async fn cleaned(store: &Arc<Mutex<peer::Store>>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.lock().unwrap().files.len() != 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("discarded intake files must be cleaned up");
}

fn intake() -> FileIntake {
    FileIntake::new("fixture".into(), None, Arc::new(|| {}))
}

#[test]
fn files_stream_to_private_remote_paths_in_order_without_overwriting_project_files() {
    run(async {
        let (store, session) = peer::connect(b"existing project file").await;
        let local = tempfile::tempdir().unwrap();
        let first = local.path().join("report '中.txt");
        let second = local.path().join("config");
        let bytes: Vec<_> = (0..700_000).map(|i| (i % 251) as u8).collect();
        std::fs::write(&first, &bytes).unwrap();
        std::fs::write(&second, b"new file").unwrap();
        let operation = intake();
        let result =
            upload(session, Some("/"), vec![first, second], &operation.context).await.unwrap();
        assert!(result.paths[0].ends_with("/report '中.txt"));
        assert!(result.paths[1].ends_with("/config"));
        {
            let store = store.lock().unwrap();
            assert_eq!(store.files["/config"].data, b"existing project file");
            assert_eq!(store.files[&result.paths[0]].data, bytes);
            assert_eq!(store.files[&result.paths[1]].data, b"new file");
            let parent = result.paths[0].rsplit_once('/').unwrap().0;
            assert_eq!(store.files[parent].permissions & 0o777, 0o700);
        }
        assert_eq!(operation.progress().transferred, 700_008);
        result.retain();
        assert_eq!(store.lock().unwrap().files.len(), 4);
    });
}

#[test]
fn dropping_an_unaccepted_result_cleans_up_the_complete_batch() {
    run(async {
        let (store, session) = peer::connect(b"original").await;
        store.lock().unwrap().aliases.insert(".".into(), "/home/test".into());
        let local = tempfile::tempdir().unwrap();
        let file = local.path().join("test.txt");
        std::fs::write(&file, b"payload").unwrap();
        let operation = intake();
        let result = upload(session, None, vec![file], &operation.context).await.unwrap();
        assert!(result.paths[0].starts_with("/home/test/.pebrel-drop-"));
        drop(result);
        cleaned(&store).await;
        assert_eq!(store.lock().unwrap().files["/config"].data, b"original");
    });
}

#[test]
fn failed_or_cancelled_uploads_return_no_paths_and_remove_partial_files() {
    run(async {
        for cancel in [false, true] {
            let (store, session) = peer::connect(b"original").await;
            let local = tempfile::tempdir().unwrap();
            let file = local.path().join("large.bin");
            std::fs::write(&file, vec![7; 1024 * 1024]).unwrap();
            let operation = intake();
            if cancel {
                store.lock().unwrap().cancel_after_write =
                    Some(operation.context.task_control.clone());
            } else {
                store.lock().unwrap().fail_writes = true;
            }
            assert!(upload(session, Some("/"), vec![file], &operation.context).await.is_err());
            cleaned(&store).await;
        }
    });
}

#[test]
fn cancelled_intake_never_creates_a_remote_directory() {
    run(async {
        let (store, session) = peer::connect(b"original").await;
        let operation = intake();
        operation.cancel();
        assert!(
            upload(session, Some("/"), vec![PathBuf::from("missing")], &operation.context)
                .await
                .is_err()
        );
        assert_eq!(store.lock().unwrap().files.len(), 1);
    });
}

#[test]
fn same_name_files_and_directories_keep_distinct_paths_and_empty_directories() {
    run(async {
        let (store, session) = peer::connect(b"original").await;
        let local = tempfile::tempdir().unwrap();
        let left = local.path().join("left");
        let right = local.path().join("right");
        std::fs::create_dir_all(left.join("docs/empty")).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join("same.txt"), b"first").unwrap();
        std::fs::write(right.join("same.txt"), b"second").unwrap();
        let operation = intake();
        let result = upload(
            session,
            Some("/"),
            vec![left.join("same.txt"), right.join("same.txt"), left.join("docs")],
            &operation.context,
        )
        .await
        .unwrap();
        assert_ne!(result.paths[0], result.paths[1]);
        {
            let store = store.lock().unwrap();
            assert_eq!(store.files[&result.paths[0]].data, b"first");
            assert_eq!(store.files[&result.paths[1]].data, b"second");
            assert!(store.files.contains_key(&format!("{}/empty", result.paths[2])));
        }
        drop(result);
        cleaned(&store).await;
    });
}
