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
        let (name, _) = normalize_fields(name, "group")?;
        if self.groups().len() >= MAX_GROUPS || self.groups().iter().any(|g| g.name == name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "duplicate name or group limit reached",
            ));
        }
        let id = new_id(&name, "group");
        self.organization.groups.push(CommandGroup { id: id.clone(), name });
        Ok(id)
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

    fn assign_group(&mut self, command_id: &str, group: Option<&str>) -> io::Result<()> {
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
