//! 后台串行文件操作；UI 仅递增代次，不持有磁盘 I/O 锁。

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::Archive;

#[derive(Clone)]
pub(crate) struct OutputStore {
    inner: Arc<StoreState>,
}

struct StoreState {
    path: PathBuf,
    generation: AtomicU64,
    writer: Mutex<()>,
}

impl OutputStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(StoreState {
                path,
                generation: AtomicU64::new(0),
                writer: Mutex::new(()),
            }),
        }
    }

    pub(crate) fn next_generation(&self) -> u64 {
        self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 仅由后台执行器调用；返回 false 表示快照已被更新或清除取代。
    pub(crate) fn save(&self, generation: u64, archive: &Archive) -> io::Result<bool> {
        self.with_generation(generation, || archive.write_to(&self.inner.path))
    }

    /// 后台保存 sidecar 后再发布本机会话引用；发布失败保留上一份有效输出。
    pub(crate) fn save_and_publish(
        &self,
        generation: u64,
        archive: &Archive,
        publish: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<bool> {
        self.save_referenced(generation, archive, None, publish)
    }

    /// Once publish succeeds, report a committed session even if invalidated inside
    /// the callback; the next serialized checkpoint reconciles the new generation.
    pub(crate) fn save_referenced(
        &self,
        generation: u64,
        archive: &Archive,
        references: Option<&[String]>,
        publish: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<bool> {
        let _writer =
            self.inner.writer.lock().map_err(|_| io::Error::other("output writer poisoned"))?;
        if generation != self.inner.generation.load(Ordering::SeqCst) {
            return Ok(false);
        }
        // A successful callback has already published the session on disk. A later
        // generation may schedule a follow-up, but cannot undo that publication.
        // Keep both generations until the follow-up has finished.
        // 新快照先校验，绝不因为旧文件损坏而接受无效的新内容。
        archive.encode()?;
        let mut previous = match Archive::read_from(&self.inner.path) {
            Ok(archive) => Some(archive),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                log::warn!("Ignoring invalid command output archive: {error}");
                None
            },
            Err(error) => return Err(error),
        };
        // 引用必须标识不可变快照，提交期间两代同时可读。进程在发布前
        // 退出时旧 session 仍有效，发布后退出时新 session 也有效。
        if let Some(previous) = &mut previous {
            if previous.previous.is_some() {
                let references = references.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "interrupted output publication requires session references",
                    )
                })?;
                previous.retain_references(references);
            }
        }
        let mut staged = archive.clone();
        for (id, records) in &archive.panes {
            if let Some(old) = previous.as_ref().and_then(|previous| previous.panes.get(id))
                && serde_json::to_vec(old)? != serde_json::to_vec(records)?
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "output reference reused for different data",
                ));
            }
        }
        staged.previous = previous.clone().map(Box::new);
        staged.write_to(&self.inner.path)?;
        if generation != self.inner.generation.load(Ordering::SeqCst) {
            return self.restore_previous(previous.as_ref()).map(|()| false);
        }
        if let Err(error) = publish() {
            self.restore_previous(previous.as_ref()).map_err(|rollback| {
                io::Error::other(format!(
                    "session publish failed: {error}; output rollback failed: {rollback}"
                ))
            })?;
            return Err(error);
        }
        // 发布已成功，压缩失败不回滚引用；旧数据留到下一次成功提交清理。
        if generation == self.inner.generation.load(Ordering::SeqCst) {
            if let Err(error) = archive.write_to(&self.inner.path) {
                log::warn!("Could not prune previous command output: {error}");
            }
        }
        Ok(true)
    }

    /// Publish a reference-free session without treating a settings toggle as clear.
    pub(crate) fn publish_without_output(
        &self,
        generation: u64,
        publish: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<bool> {
        let _writer =
            self.inner.writer.lock().map_err(|_| io::Error::other("output writer poisoned"))?;
        if generation != self.inner.generation.load(Ordering::SeqCst) {
            return Ok(false);
        }
        publish()?;
        Ok(true)
    }

    fn restore_previous(&self, previous: Option<&Archive>) -> io::Result<()> {
        match previous {
            Some(archive) => archive.write_to(&self.inner.path),
            None => std::fs::remove_file(&self.inner.path),
        }
    }

    /// 清除完成后旧写入不能重新出现；调用方须等待结果再显示成功。
    pub(crate) fn clear(&self, generation: u64) -> io::Result<bool> {
        self.with_generation(generation, || match std::fs::remove_file(&self.inner.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
    }

    fn with_generation(
        &self,
        generation: u64,
        operation: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<bool> {
        let _writer =
            self.inner.writer.lock().map_err(|_| io::Error::other("output writer poisoned"))?;
        if generation != self.inner.generation.load(Ordering::SeqCst) {
            return Ok(false);
        }
        operation()?;
        // UI 可在 I/O 中使代次失效；后续清除在同一锁之后执行。
        Ok(generation == self.inner.generation.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent_output::storage::{Archive, tests::records};
    use std::collections::BTreeMap;

    fn store_fixture() -> (tempfile::TempDir, PathBuf, OutputStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outputs.json");
        let store = OutputStore::new(path.clone());
        (dir, path, store)
    }

    fn archive(id: &str) -> Archive {
        Archive::new(BTreeMap::from([(id.into(), records("C:>echo safe"))]))
    }

    #[test]
    fn regression_corrupt_archive_does_not_block_new_session_publication() {
        let (_dir, path, store) = store_fixture();
        std::fs::write(&path, b"{broken archive").unwrap();
        let published = std::cell::Cell::new(false);
        assert!(
            store
                .save_and_publish(store.next_generation(), &archive("new"), || {
                    published.set(true);
                    Ok(())
                })
                .unwrap()
        );
        assert!(published.get());
        assert!(Archive::read_from(&path).unwrap().panes.contains_key("new"));
    }

    #[test]
    fn regression_two_valid_large_generations_do_not_block_publication() {
        let (_dir, path, store) = store_fixture();
        let records = records("C:>echo safe");
        let entry_bytes = serde_json::to_vec(&records).unwrap().len();
        let count = 9 * 1024 * 1024 / entry_bytes;
        let large = |prefix: &str| {
            Archive::new(
                (0..count).map(|index| (format!("{prefix}-{index}"), records.clone())).collect(),
            )
        };
        let previous = large("previous");
        let next = large("next");
        previous.write_to(&path).unwrap();
        let old_len = std::fs::metadata(&path).unwrap().len();
        assert!(old_len > 8 * 1024 * 1024 && old_len < 16 * 1024 * 1024);
        assert!(
            store
                .save_and_publish(store.next_generation(), &next, || {
                    let staged = Archive::read_from(&path)?;
                    let mut staged = staged;
                    assert!(staged.take_records("previous-0").is_some());
                    assert!(staged.take_records("next-0").is_some());
                    Ok(())
                })
                .unwrap()
        );
        let restored = Archive::read_from(&path).unwrap();
        assert!(restored.panes.contains_key("next-0"));
        assert!(!restored.panes.contains_key("previous-0"));
    }

    #[test]
    fn interrupted_publication_keeps_only_referenced_generation_on_retry() {
        for reference in ["previous", "next"] {
            let (_dir, path, store) = store_fixture();
            let mut interrupted = archive("next");
            interrupted.previous = Some(Box::new(archive("previous")));
            interrupted.write_to(&path).unwrap();
            let references = vec![reference.to_string()];
            assert!(
                store
                    .save_referenced(
                        store.next_generation(),
                        &archive("third"),
                        Some(&references),
                        || {
                            let mut during = Archive::read_from(&path)?;
                            assert!(during.take_records(reference).is_some());
                            assert!(during.take_records("third").is_some());
                            let unreferenced =
                                if reference == "previous" { "next" } else { "previous" };
                            assert!(during.take_records(unreferenced).is_none());
                            Ok(())
                        }
                    )
                    .unwrap()
            );
            let after = Archive::read_from(&path).unwrap();
            assert!(after.previous.is_none());
            assert_eq!(after.panes.len(), 1);
        }
    }

    #[test]
    fn interrupted_publication_failed_retry_preserves_referenced_output() {
        let (_dir, path, store) = store_fixture();
        let mut interrupted = archive("next");
        interrupted.previous = Some(Box::new(archive("previous")));
        interrupted.write_to(&path).unwrap();
        assert!(
            store
                .save_referenced(
                    store.next_generation(),
                    &archive("third"),
                    Some(&["previous".into()]),
                    || { Err(io::Error::other("publication failed")) }
                )
                .is_err()
        );
        let mut after = Archive::read_from(&path).unwrap();
        assert!(after.take_records("previous").is_some());
        assert!(after.take_records("third").is_none());
    }

    #[test]
    fn delayed_save_cannot_resurrect_cleared_output() {
        let (_dir, path, store) = store_fixture();
        let old = store.next_generation();
        let clear = store.next_generation();
        store.clear(clear).unwrap();
        assert!(!store.save(old, &archive("old")).unwrap());
        assert!(!path.exists());
        let new = store.next_generation();
        assert!(store.save(new, &archive("new")).unwrap());
        assert!(Archive::read_from(&path).unwrap().panes.contains_key("new"));
    }

    #[test]
    fn out_of_order_save_cannot_reintroduce_closed_pane() {
        let (_dir, path, store) = store_fixture();
        let old = store.next_generation();
        let new = store.next_generation();
        store.save(new, &archive("remaining")).unwrap();
        assert!(!store.save(old, &archive("closed")).unwrap());
        let archive = Archive::read_from(&path).unwrap();
        assert_eq!(archive.panes.len(), 1);
        assert!(archive.panes.contains_key("remaining"));
    }

    #[test]
    fn checkpoint_publishes_references_only_after_output_is_durable() {
        let (_dir, path, store) = store_fixture();
        let generation = store.next_generation();
        let published = std::cell::Cell::new(false);
        assert!(
            store
                .save_and_publish(generation, &archive("new"), || {
                    assert!(Archive::read_from(&path)?.panes.contains_key("new"));
                    published.set(true);
                    Ok(())
                })
                .unwrap()
        );
        assert!(published.get());
        assert!(
            !store
                .save_and_publish(generation - 1, &archive("stale"), || {
                    panic!("stale checkpoint must not publish session references")
                })
                .unwrap()
        );
    }

    #[test]
    fn previous_references_remain_readable_until_new_session_is_published() {
        let (_dir, path, store) = store_fixture();
        store.save(store.next_generation(), &archive("previous")).unwrap();
        let next = store.next_generation();
        store
            .save_and_publish(next, &archive("new"), || {
                let mut during = Archive::read_from(&path)?;
                assert!(
                    during.take_records("previous").is_some(),
                    "old session must survive a crash before publication"
                );
                assert!(during.panes.contains_key("new"));
                Ok(())
            })
            .unwrap();
        let after = Archive::read_from(&path).unwrap();
        assert!(after.panes.contains_key("new"));
        assert!(!after.panes.contains_key("previous"));
    }

    #[test]
    fn failed_session_publication_restores_previous_output_archive() {
        let (_dir, path, store) = store_fixture();
        let old = store.next_generation();
        store.save(old, &archive("previous")).unwrap();
        let before = std::fs::read(&path).unwrap();
        let next = store.next_generation();
        assert!(
            store
                .save_and_publish(next, &archive("new"), || {
                    Err(io::Error::other("session write failed"))
                })
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(store.save_and_publish(next, &archive("new"), || Ok(())).unwrap());
    }

    #[test]
    fn reusing_reference_for_different_output_does_not_overwrite_previous_data() {
        let (_dir, path, store) = store_fixture();
        store.save(store.next_generation(), &archive("same-id")).unwrap();
        let before = std::fs::read(&path).unwrap();
        let changed =
            Archive::new(BTreeMap::from([("same-id".into(), records("C:>different command"))]));
        assert!(
            store
                .save_and_publish(store.next_generation(), &changed, || {
                    panic!("a conflicting identity must never be published")
                })
                .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn invalidation_during_publication_is_acknowledged_before_later_clear() {
        let (_dir, path, store) = store_fixture();
        let generation = store.next_generation();
        let clear = std::cell::Cell::new(0);
        assert!(
            store
                .save_and_publish(generation, &archive("new"), || {
                    clear.set(store.next_generation());
                    Ok(())
                })
                .unwrap()
        );
        assert!(store.clear(clear.get()).unwrap());
        assert!(!store.save(generation, &archive("new")).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn invalidation_inside_publication_reports_durable_session_and_keeps_old_reference() {
        let (dir, path, store) = store_fixture();
        let session_path = dir.path().join("session.json");
        let mut old_session = crate::session::Session::new(
            0,
            vec![crate::session::TabSession::single("C:/test".into(), None, None)],
        );
        old_session.tabs[0].output_refs = vec!["old".into()];
        archive("old").write_to(&path).unwrap();
        crate::session::save_local_to(&session_path, &old_session).unwrap();

        let mut new_session = old_session.clone();
        new_session.tabs[0].output_refs = vec!["new".into()];
        let generation = store.next_generation();
        let published = store
            .save_referenced(generation, &archive("new"), Some(&["old".into()]), || {
                crate::session::save_local_to(&session_path, &new_session)?;
                store.next_generation();
                Ok(())
            })
            .unwrap();
        assert!(published, "a written session cannot be reported as cancelled");
        assert_eq!(
            crate::session::load_local_from(&session_path).unwrap().tabs[0].output_refs,
            ["new"]
        );
        let mut staged = Archive::read_from(&path).unwrap();
        assert!(staged.take_records("old").is_some());
        assert!(staged.take_records("new").is_some());
    }

    #[test]
    fn reference_free_checkpoint_preserves_old_archive_on_failure_and_retry() {
        let (dir, path, store) = store_fixture();
        let session_path = dir.path().join("session.json");
        archive("old").write_to(&path).unwrap();
        let generation = store.next_generation();
        assert!(
            store
                .publish_without_output(generation, || Err(io::Error::other("session unavailable")))
                .is_err()
        );
        assert!(!session_path.exists());
        let before = std::fs::read(&path).unwrap();
        let session = crate::session::Session::new(
            0,
            vec![crate::session::TabSession::single("C:/test".into(), None, None)],
        );
        assert!(
            store
                .publish_without_output(generation, || {
                    crate::session::save_local_to(&session_path, &session)
                })
                .unwrap()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            crate::session::load_local_from(&session_path).unwrap().tabs[0].output_refs.is_empty()
        );
    }

    #[test]
    fn failed_save_preserves_previous_archive_and_allows_retry() {
        let (_dir, path, store) = store_fixture();
        let first = store.next_generation();
        store.save(first, &archive("before")).unwrap();
        let next = store.next_generation();
        let mut invalid = archive("after");
        invalid.version = 999;
        assert!(store.save(next, &invalid).is_err());
        assert!(Archive::read_from(&path).unwrap().panes.contains_key("before"));
        assert!(store.save(next, &archive("after")).unwrap());
    }
}
