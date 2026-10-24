//! Manual order uses the existing arrays and the shared store transaction.

use super::*;

#[derive(Clone, Copy)]
pub(crate) enum MoveDirection {
    Up,
    Down,
}

impl SavedCommands {
    pub(crate) fn can_reorder_group(&self, id: &str, direction: MoveDirection) -> bool {
        self.group_neighbor(id, direction).is_some()
    }

    pub(crate) fn can_reorder_command(&self, id: &str, direction: MoveDirection) -> bool {
        self.command_neighbor(id, direction).is_some()
    }

    pub(crate) fn reorder_group(&mut self, id: &str, direction: MoveDirection) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| {
            if !store.groups().iter().any(|group| group.id == id) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "custom group not found"));
            }
            if let Some((from, to)) = store.group_neighbor(id, direction) {
                store.organization.groups.swap(from, to);
            }
            Ok(())
        })?;
        *self = next;
        Ok(())
    }

    pub(crate) fn reorder_command(&mut self, id: &str, direction: MoveDirection) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| {
            if is_builtin_id(id) || !store.commands.iter().any(|command| command.id == id) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "custom command not found"));
            }
            // Resolve membership and neighbors after reloading under the lock. Global
            // neighbors may belong to other folders and must keep their relative order.
            if let Some((from, to)) = store.command_neighbor(id, direction) {
                store.commands.swap(from, to);
            }
            Ok(())
        })?;
        *self = next;
        Ok(())
    }

    fn group_neighbor(&self, id: &str, direction: MoveDirection) -> Option<(usize, usize)> {
        let from = self.groups().iter().position(|group| group.id == id)?;
        let to = match direction {
            MoveDirection::Up => from.checked_sub(1)?,
            MoveDirection::Down => from + 1,
        };
        (to < self.groups().len()).then_some((from, to))
    }

    fn command_neighbor(&self, id: &str, direction: MoveDirection) -> Option<(usize, usize)> {
        if is_builtin_id(id) {
            return None;
        }
        let from = self.commands.iter().position(|command| command.id == id)?;
        let group = self.group_for(id);
        let same_group = |&index: &usize| self.group_for(&self.commands[index].id) == group;
        let to = match direction {
            MoveDirection::Up => (0..from).rev().find(same_group)?,
            MoveDirection::Down => (from + 1..self.commands.len()).find(same_group)?,
        };
        Some((from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MoveDirection::{Down, Up};

    #[test]
    fn group_order_round_trips_and_preserves_membership_and_newer_window_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        saved.create_group("Servers").unwrap();
        let work = saved.groups()[0].id.clone();
        let servers = saved.groups()[1].id.clone();
        let command = saved.insert_in_group("Build", "cargo build", false, Some(&work)).unwrap();
        let mut stale = saved.clone();
        saved.create_group("Containers").unwrap();
        let containers = saved.groups()[2].id.clone();

        stale.reorder_group(&servers, Up).unwrap();
        let mut loaded = SavedCommands::load_from(&path).unwrap();
        assert_eq!(
            loaded.groups().iter().map(|g| &g.id).collect::<Vec<_>>(),
            [&servers, &work, &containers]
        );
        assert_eq!(loaded.group_for(&command.id), Some(work.as_str()));
        assert_eq!(loaded.commands(), &[command]);
        assert!(!loaded.can_reorder_group(&servers, Up));
        assert!(!loaded.can_reorder_group(&containers, Down));
        loaded.reorder_group(&servers, Down).unwrap();
        assert_eq!(SavedCommands::load_from(&path).unwrap().groups(), saved.groups());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(json["version"], 1);
    }

    #[test]
    fn command_moves_swap_only_same_group_neighbors_in_interleaved_storage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        let group = saved.groups()[0].id.clone();
        let a = saved.insert_in_group("A", "echo a", false, Some(&group)).unwrap();
        let b = saved.insert("B", "echo b", true).unwrap();
        let c = saved.insert_in_group("C", "echo c", true, Some(&group)).unwrap();
        let d = saved.insert("D", "echo d", false).unwrap();
        saved.move_to_group("builtin:docker_ps", Some(&group)).unwrap();
        saved.remove("builtin:docker_exec").unwrap();
        let metadata = saved.organization.clone();
        let deleted = saved.deleted_builtins.clone();

        assert!(!saved.can_reorder_command(&a.id, Up));
        assert!(!saved.can_reorder_command(&c.id, Down));
        saved.reorder_command(&c.id, Up).unwrap();
        assert_eq!(saved.commands(), &[c.clone(), b.clone(), a.clone(), d.clone()]);
        saved.reorder_command(&b.id, Down).unwrap();
        assert_eq!(saved.commands(), &[c, d, a, b]);
        assert_eq!(saved.organization, metadata);
        assert_eq!(saved.deleted_builtins, deleted);
        assert_eq!(SavedCommands::load_from(&path).unwrap(), saved);
    }

    #[test]
    fn commands_use_current_order_and_membership_without_losing_other_windows_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        let a = saved.insert("A", "echo a", false).unwrap();
        let b = saved.insert("B", "echo b", true).unwrap();
        let mut stale = saved.clone();
        saved.create_group("Work").unwrap();
        let group = saved.groups()[0].id.clone();
        saved.move_to_group(&b.id, Some(&group)).unwrap();
        let c = saved.insert_in_group("C", "echo c", false, Some(&group)).unwrap();
        saved.update(&b.id, "Edited B", "echo edited", false).unwrap();

        stale.reorder_command(&b.id, Down).unwrap();
        let loaded = SavedCommands::load_from(&path).unwrap();
        assert_eq!(loaded.commands()[0], a);
        assert_eq!(loaded.commands()[1], c);
        assert_eq!(loaded.commands()[2].id, b.id);
        assert_eq!(loaded.commands()[2].command, "echo edited");
        assert!(!loaded.commands()[2].append_enter);
        assert_eq!(loaded.group_for(&b.id), Some(group.as_str()));
        assert_eq!(loaded.groups(), saved.groups());
    }

    #[test]
    fn edges_are_noops_and_missing_fixed_or_locked_targets_preserve_disk_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Only folder").unwrap();
        let group = saved.groups()[0].id.clone();
        let a = saved.insert("A", "echo a", false).unwrap();
        let b = saved.insert("B", "echo b", false).unwrap();
        let before = std::fs::read(&path).unwrap();
        for direction in [Up, Down] {
            assert!(!saved.can_reorder_group(&group, direction));
            saved.reorder_group(&group, direction).unwrap();
            assert!(!saved.can_reorder_group(BUILTIN_GROUP_ID, direction));
            assert!(!saved.can_reorder_command("builtin:docker_ps", direction));
            assert!(saved.reorder_group(BUILTIN_GROUP_ID, direction).is_err());
            assert!(saved.reorder_group("missing", direction).is_err());
            assert!(saved.reorder_command("builtin:docker_ps", direction).is_err());
            assert!(saved.reorder_command("missing", direction).is_err());
        }
        saved.reorder_command(&a.id, Up).unwrap();
        saved.reorder_command(&b.id, Down).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let snapshot = saved.clone();
        let _lock = crate::atomic_file::try_lock(&path).unwrap().unwrap();
        assert_eq!(
            saved.reorder_command(&a.id, Down).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(saved.reorder_group(&group, Up).unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(saved, snapshot);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn deleted_targets_do_not_restore_stale_rows_or_groups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        let group = saved.groups()[0].id.clone();
        let command = saved.insert("Build", "cargo build", false).unwrap();
        let mut stale = saved.clone();
        saved.remove(&command.id).unwrap();
        saved.remove_group(&group).unwrap();
        let before = std::fs::read(&path).unwrap();
        let snapshot = stale.clone();
        assert!(stale.reorder_command(&command.id, Down).is_err());
        assert!(stale.reorder_group(&group, Up).is_err());
        assert_eq!(stale, snapshot);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
