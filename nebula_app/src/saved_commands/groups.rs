//! Group metadata shares the command store's lock/reload/atomic-write transaction.

use std::collections::{BTreeMap, HashSet};

use super::*;

pub(crate) const BUILTIN_GROUP_ID: &str = "builtin";
const MAX_GROUPS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CommandGroup {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Organization {
    #[serde(default)]
    pub groups: Vec<CommandGroup>,
    // Absence uses the default. Explicit null keeps a builtin outside its default group.
    #[serde(default)]
    pub membership: BTreeMap<String, Option<String>>,
}

impl SavedCommands {
    pub(crate) fn groups(&self) -> &[CommandGroup] {
        &self.organization.groups
    }

    pub(crate) fn group_for<'a>(&'a self, command_id: &str) -> Option<&'a str> {
        match self.organization.membership.get(command_id) {
            Some(group) => group.as_deref(),
            None if command_id.starts_with("builtin:") => Some(BUILTIN_GROUP_ID),
            None => None,
        }
    }

    pub(crate) fn create_group(&mut self, name: &str) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| store.add_group(name).map(|_| ()))?;
        *self = next;
        Ok(())
    }

    fn add_group(&mut self, name: &str) -> io::Result<String> {
        let name = self.normalized_group_name(name, None)?;
        if self.groups().len() >= MAX_GROUPS {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "group limit reached"));
        }
        let id = new_id(&name, "group");
        self.organization.groups.push(CommandGroup { id: id.clone(), name });
        Ok(id)
    }

    fn normalized_group_name(&self, name: &str, except_id: Option<&str>) -> io::Result<String> {
        let (name, _) = normalize_fields(name, "group")?;
        if self
            .groups()
            .iter()
            .any(|group| Some(group.id.as_str()) != except_id && group.name == name)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "duplicate group name"));
        }
        Ok(name)
    }

    pub(crate) fn rename_group(&mut self, group_id: &str, name: &str) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| {
            let name = store.normalized_group_name(name, Some(group_id))?;
            let Some(group) = store.organization.groups.iter_mut().find(|g| g.id == group_id)
            else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "group no longer exists"));
            };
            group.name = name;
            Ok(())
        })?;
        *self = next;
        Ok(())
    }

    pub(crate) fn move_to_group(
        &mut self,
        command_id: &str,
        group: Option<&str>,
    ) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| store.assign_group(command_id, group))?;
        *self = next;
        Ok(())
    }

    pub(super) fn assign_group(&mut self, command_id: &str, group: Option<&str>) -> io::Result<()> {
        if !self.has_command(command_id) || !self.has_group(group) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "command or group no longer exists",
            ));
        }
        self.organization.membership.insert(command_id.to_owned(), group.map(str::to_owned));
        Ok(())
    }

    pub(crate) fn remove_group(&mut self, group_id: &str) -> io::Result<()> {
        let (next, ()) = mutate_store(&self.path, |store| store.delete_group(group_id))?;
        *self = next;
        Ok(())
    }

    fn delete_group(&mut self, group_id: &str) -> io::Result<()> {
        let Some(index) = self.organization.groups.iter().position(|group| group.id == group_id)
        else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "group no longer exists"));
        };
        self.organization.groups.remove(index);
        for group in self.organization.membership.values_mut() {
            if group.as_deref() == Some(group_id) {
                *group = None;
            }
        }
        Ok(())
    }

    fn has_command(&self, id: &str) -> bool {
        // Builtin ids persist across the platform-specific views of the same store.
        (is_builtin_id(id) && !self.deleted_builtins.contains(id))
            || self.commands.iter().any(|command| command.id == id)
    }

    fn has_group(&self, id: Option<&str>) -> bool {
        id.is_none_or(|id| {
            id == BUILTIN_GROUP_ID || self.groups().iter().any(|group| group.id == id)
        })
    }

    pub(super) fn validate_groups(&self) -> io::Result<()> {
        let invalid =
            || io::Error::new(io::ErrorKind::InvalidData, "invalid command group metadata");
        if self.groups().len() > MAX_GROUPS {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for group in self.groups() {
            if group.id.is_empty()
                || group.id.len() > MAX_ID_CHARS
                || group.id == BUILTIN_GROUP_ID
                || !ids.insert(&group.id)
            {
                return Err(invalid());
            }
            normalize_fields(&group.name, "group").map_err(|_| invalid())?;
        }
        for (command, group) in &self.organization.membership {
            if !self.has_command(command) || !self.has_group(group.as_deref()) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_preserves_group_identity_membership_and_order_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        saved.create_group("Servers").unwrap();
        let group = saved.groups()[0].id.clone();
        saved.insert_in_group("Build", "cargo build", false, Some(&group)).unwrap();
        saved.insert_in_group("Test", "cargo test", true, Some(&group)).unwrap();
        saved.move_to_group("builtin:docker_ps", Some(&group)).unwrap();
        let mut expected = saved.clone();
        expected.organization.groups[0].name = "项目".into();
        saved.rename_group(&group, "  项目  ").unwrap();
        assert_eq!(saved, expected);
        assert_eq!(SavedCommands::load_from(&path).unwrap(), expected);
        saved.rename_group(&group, "项目").unwrap();
        assert_eq!(saved, expected, "saving an unchanged name is valid");
    }

    #[test]
    fn rename_rejects_invalid_names_and_fixed_or_missing_groups_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        saved.create_group("Servers").unwrap();
        let group = saved.groups()[0].id.clone();
        let before = saved.clone();
        let bytes = std::fs::read(&path).unwrap();
        for name in ["", "  ", " Servers ", "two\nlines", "a\0b", &"字".repeat(MAX_NAME_CHARS + 1)]
        {
            assert_eq!(
                saved.rename_group(&group, name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(saved, before);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        for id in [BUILTIN_GROUP_ID, "", "missing"] {
            assert_eq!(
                saved.rename_group(id, "New name").unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            assert_eq!(saved, before);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn rename_reloads_other_windows_changes_and_rejects_stale_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        let group = saved.groups()[0].id.clone();
        let mut other = saved.clone();
        other.create_group("Servers").unwrap();
        other.insert_in_group("Build", "cargo build", false, Some(&group)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(saved.rename_group(&group, "Servers").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        saved.rename_group(&group, "Project").unwrap();
        other.organization.groups[0].name = "Project".into();
        assert_eq!(saved, other);
        other.remove_group(&group).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(saved.rename_group(&group, "Gone").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn rename_remains_available_at_the_group_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        for index in 0..MAX_GROUPS {
            saved.create_group(&format!("Group {index}")).unwrap();
        }
        assert!(saved.create_group("Overflow").is_err());
        let group = saved.groups()[0].id.clone();
        saved.rename_group(&group, "Renamed").unwrap();
        assert_eq!(saved.groups().len(), MAX_GROUPS);
        assert_eq!(saved.groups()[0].name, "Renamed");
    }

    #[test]
    fn inserting_inside_a_folder_is_atomic_and_rejects_a_deleted_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let mut saved = SavedCommands::load_from(&path).unwrap();
        saved.create_group("Work").unwrap();
        let group = saved.groups()[0].id.clone();
        let command = saved.insert_in_group("Build", "cargo build", true, Some(&group)).unwrap();
        assert_eq!(
            SavedCommands::load_from(&path).unwrap().group_for(&command.id),
            Some(group.as_str())
        );
        saved.remove_group(&group).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(saved.insert_in_group("Lost", "echo lost", false, Some(&group)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn old_store_defaults_and_explicit_builtin_removal_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        std::fs::write(&path, r#"{"version":1,"commands":[]}"#).unwrap();
        let old = SavedCommands::load_from(&path).unwrap();
        assert_eq!(old.group_for("builtin:docker_ps"), Some(BUILTIN_GROUP_ID));
        let (moved, ()) =
            mutate_store(&path, |store| store.assign_group("builtin:docker_ps", None)).unwrap();
        assert_eq!(moved.group_for("builtin:docker_ps"), None);
        assert_eq!(SavedCommands::load_from(&path).unwrap(), moved);
    }

    #[test]
    fn group_transactions_preserve_other_windows_and_reject_stale_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_FILE);
        let (_, first) = mutate_store(&path, |store| store.add_group("Work")).unwrap();
        let (_, second) = mutate_store(&path, |store| store.add_group("Servers")).unwrap();
        mutate_store(&path, |store| store.assign_group("builtin:docker_ps", Some(&first))).unwrap();
        let (saved, ()) = mutate_store(&path, |store| store.delete_group(&first)).unwrap();
        assert_eq!(saved.groups()[0].id, second);
        assert_eq!(saved.group_for("builtin:docker_ps"), None);
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            mutate_store(&path, |store| store.assign_group("builtin:docker_ps", Some(&first)))
                .is_err()
        );
        assert!(
            mutate_store(&path, |store| store.assign_group("missing-command", Some(&second)))
                .is_err()
        );
        assert!(mutate_store(&path, |store| store.add_group("Servers")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
