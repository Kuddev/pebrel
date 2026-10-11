//! Batch operations share the existing lock/reload/write transaction and tombstones.

use super::*;

fn builtin_ids() -> HashSet<String> {
    use builtins::CommandPlatform::*;
    [Windows, Mac, Posix]
        .into_iter()
        .flat_map(|platform| {
            builtins::commands(crate::i18n::UiLanguage::EnUs, platform)
                .into_iter()
                .map(|command| command.id)
        })
        .collect()
}

impl SavedCommands {
    /// Stale and duplicate selections are harmless; unrelated concurrent edits survive.
    pub(crate) fn remove_many(&mut self, ids: &[String]) -> io::Result<usize> {
        let ids = ids.iter().cloned().collect();
        let (next, removed) = mutate_store(&self.path, |store| Ok(store.remove_ids(&ids)))?;
        *self = next;
        Ok(removed)
    }

    /// Clear every custom command and current built-in recipe on all platforms.
    /// Keep folders so that clearing commands does not silently discard organization.
    pub(crate) fn clear_commands(&mut self) -> io::Result<usize> {
        let (next, removed) = mutate_store(&self.path, |store| {
            let mut ids = builtin_ids();
            ids.extend(store.commands.iter().map(|command| command.id.clone()));
            Ok(store.remove_ids(&ids))
        })?;
        *self = next;
        Ok(removed)
    }

    pub(crate) fn clear_builtin_commands(&mut self) -> io::Result<usize> {
        self.remove_many(&builtin_ids().into_iter().collect::<Vec<_>>())
    }

    /// Restore deleted built-ins to their default folder without changing surviving
    /// commands or their explicitly assigned groups.
    pub(crate) fn restore_builtin_commands(&mut self) -> io::Result<usize> {
        let (next, restored) = mutate_store(&self.path, |store| {
            let restored = store.deleted_builtins.len();
            store.deleted_builtins.clear();
            Ok(restored)
        })?;
        *self = next;
        Ok(restored)
    }

    pub(super) fn remove_ids(&mut self, ids: &HashSet<String>) -> usize {
        let mut removed = 0;
        self.commands.retain(|command| {
            let keep = !ids.contains(&command.id);
            if !keep {
                removed += 1;
            }
            keep
        });
        for id in ids.iter().filter(|id| is_builtin_id(id)) {
            if self.deleted_builtins.insert(id.clone()) {
                removed += 1;
            }
        }
        self.organization.membership.retain(|id, _| !ids.contains(id));
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_delete_is_stale_safe_and_preserves_other_windows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut first = SavedCommands::load_from(&path).unwrap();
        first.create_group("Work").unwrap();
        let group = first.groups()[0].id.clone();
        let old = first.insert_in_group("Old", "echo old", false, Some(&group)).unwrap();
        let mut second = SavedCommands::load_from(&path).unwrap();
        let new = second.insert("New", "echo new", false).unwrap();
        assert_eq!(
            first
                .remove_many(&[
                    old.id.clone(),
                    old.id,
                    "missing".into(),
                    "builtin:docker_ps".into()
                ])
                .unwrap(),
            2
        );
        assert_eq!(first.commands(), &[new]);
        assert_eq!(first.groups().len(), 1);
        assert_eq!(SavedCommands::load_from(&path).unwrap(), first);
        assert_eq!(first.remove_many(&["missing".into(), "builtin:docker_ps".into()]).unwrap(), 0);
        first.restore_builtin_commands().unwrap();
        assert!(!first.deleted_builtins.contains("builtin:docker_ps"));
    }

    #[test]
    fn clear_and_restore_cover_all_platforms_and_keep_custom_folders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Keep folder").unwrap();
        let custom = saved.insert("Custom", "echo custom", false).unwrap();
        saved.clear_builtin_commands().unwrap();
        assert_eq!(saved.commands(), &[custom]);
        for platform in [
            builtins::CommandPlatform::Windows,
            builtins::CommandPlatform::Mac,
            builtins::CommandPlatform::Posix,
        ] {
            assert!(saved.builtin_commands(crate::i18n::UiLanguage::EnUs, platform).is_empty());
        }
        saved.restore_builtin_commands().unwrap();
        assert!(saved.deleted_builtins.is_empty());
        saved.clear_commands().unwrap();
        assert!(saved.commands().is_empty());
        assert_eq!(saved.groups().len(), 1);
        saved.restore_builtin_commands().unwrap();
        assert!(saved.commands().is_empty(), "restore does not invent deleted custom commands");
        assert_eq!(SavedCommands::load_from(&path).unwrap(), saved);
    }

    #[test]
    fn failed_batch_write_leaves_memory_and_store_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        let command = saved.insert("Keep", "echo keep", false).unwrap();
        let before = saved.clone();
        let bytes = std::fs::read(&path).unwrap();
        let _lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        assert_eq!(saved.remove_many(&[command.id]).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(saved, before);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
