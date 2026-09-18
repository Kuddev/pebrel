//! User-managed shell commands shown by the GPUI command manager.

pub(crate) mod builtins;
mod groups;
pub(crate) use groups::BUILTIN_GROUP_ID;

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STORE_VERSION: u32 = 1;
const STORE_FILE: &str = "saved_commands.json";
const MAX_COMMANDS: usize = 40;
const MAX_ID_CHARS: usize = 80;
const MAX_NAME_CHARS: usize = 80;
const MAX_COMMAND_CHARS: usize = 4_000;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedCommand {
    pub id: String,
    pub name: String,
    pub command: String,
    /// `false` 表示只插入当前终端，留给用户补参数或手动确认后再执行。
    #[serde(default = "default_append_enter")]
    pub append_enter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CommandStore {
    version: u32,
    #[serde(default)]
    commands: Vec<SavedCommand>,
    #[serde(default)]
    organization: groups::Organization,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    deleted_builtins: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedCommands {
    path: PathBuf,
    commands: Vec<SavedCommand>,
    organization: groups::Organization,
    deleted_builtins: BTreeSet<String>,
}

impl Default for SavedCommands {
    fn default() -> Self {
        Self {
            path: store_path(),
            commands: Vec::new(),
            organization: groups::Organization::default(),
            deleted_builtins: BTreeSet::new(),
        }
    }
}

impl SavedCommands {
    pub(crate) fn load() -> io::Result<Self> {
        Self::load_from(&store_path())
    }

    pub(crate) fn load_from(path: &Path) -> io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self { path: path.to_owned(), ..Self::default() });
            },
            Err(error) => return Err(error),
        };
        let store: CommandStore = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if store.version != STORE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported saved command version {}", store.version),
            ));
        }
        validate_store(&store.commands)?;
        let saved = Self {
            path: path.to_owned(),
            commands: store.commands,
            organization: store.organization,
            deleted_builtins: store.deleted_builtins,
        };
        if saved.deleted_builtins.iter().any(|id| !is_builtin_id(id)) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid builtin command id"));
        }
        saved.validate_groups()?;
        Ok(saved)
    }

    pub(crate) fn commands(&self) -> &[SavedCommand] {
        &self.commands
    }

    pub(crate) fn builtin_commands(
        &self,
        language: crate::i18n::UiLanguage,
        platform: builtins::CommandPlatform,
    ) -> Vec<SavedCommand> {
        builtins::commands(language, platform)
            .into_iter()
            .filter(|command| !self.deleted_builtins.contains(&command.id))
            .collect()
    }

    pub(crate) fn reload(&mut self) -> io::Result<()> {
        *self = Self::load_from(&self.path)?;
        Ok(())
    }

    pub(crate) fn insert(
        &mut self,
        name: &str,
        command: &str,
        append_enter: bool,
    ) -> io::Result<SavedCommand> {
        let path = self.path.clone();
        let (next, inserted) = mutate_store(&path, |store| {
            let commands = &mut store.commands;
            if commands.len() >= MAX_COMMANDS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("saved command limit is {MAX_COMMANDS}"),
                ));
            }
            let (name, command) = normalize_fields(name, command)?;
            let mut saved =
                SavedCommand { id: new_id(&name, &command), name, command, append_enter };
            while commands.iter().any(|existing| existing.id == saved.id) {
                saved.id = new_id(&saved.name, &saved.command);
            }
            commands.push(saved.clone());
            Ok(saved)
        })?;
        *self = next;
        Ok(inserted)
    }

    pub(crate) fn update(
        &mut self,
        id: &str,
        name: &str,
        command: &str,
        append_enter: bool,
    ) -> io::Result<()> {
        let path = self.path.clone();
        let (next, ()) = mutate_store(&path, |store| {
            let commands = &mut store.commands;
            let (name, command) = normalize_fields(name, command)?;
            let Some(saved) = commands.iter_mut().find(|saved| saved.id == id) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "saved command not found"));
            };
            saved.name = name;
            saved.command = command;
            saved.append_enter = append_enter;
            Ok(())
        })?;
        *self = next;
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: &str) -> io::Result<()> {
        let path = self.path.clone();
        let (next, ()) = mutate_store(&path, |store| {
            if is_builtin_id(id) {
                store.deleted_builtins.insert(id.to_owned());
                store.organization.membership.remove(id);
                return Ok(());
            }
            let commands = &mut store.commands;
            let Some(index) = commands.iter().position(|saved| saved.id == id) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "saved command not found"));
            };
            commands.remove(index);
            store.organization.membership.remove(id);
            Ok(())
        })?;
        *self = next;
        Ok(())
    }
}

fn mutate_store<T>(
    path: &Path,
    mutate: impl FnOnce(&mut SavedCommands) -> io::Result<T>,
) -> io::Result<(SavedCommands, T)> {
    let Some(_lock) = crate::atomic_file::try_lock(path)? else {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "saved command store is locked by another process",
        ));
    };
    // 锁内重新读盘：多个 Nebula 窗口同时管理命令时，不能拿各自启动时的旧快照
    // 覆盖对方刚写入的列表。
    let mut saved = SavedCommands::load_from(path)?;
    let result = mutate(&mut saved)?;
    validate_store(&saved.commands)?;
    saved.validate_groups()?;
    let store = CommandStore {
        version: STORE_VERSION,
        commands: saved.commands.clone(),
        organization: saved.organization.clone(),
        deleted_builtins: saved.deleted_builtins.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&store).map_err(io::Error::other)?;
    crate::atomic_file::write(path, &bytes)?;
    Ok((saved, result))
}

fn is_builtin_id(id: &str) -> bool {
    id.starts_with("builtin:") && id.len() > 8 && id.len() <= MAX_ID_CHARS
}

fn normalize_fields(name: &str, command: &str) -> io::Result<(String, String)> {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "command name is required"));
    }
    if name.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command name must be a single line",
        ));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("command name exceeds {MAX_NAME_CHARS} characters"),
        ));
    }
    if command.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "command text is required"));
    }
    if command.contains('\0') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "command contains NUL"));
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("command text exceeds {MAX_COMMAND_CHARS} characters"),
        ));
    }
    Ok((name.to_owned(), command.to_owned()))
}

fn validate_store(commands: &[SavedCommand]) -> io::Result<()> {
    if commands.len() > MAX_COMMANDS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("saved command store exceeds {MAX_COMMANDS} entries"),
        ));
    }
    let mut ids = HashSet::with_capacity(commands.len());
    for saved in commands {
        if saved.id.trim().is_empty()
            || saved.id.chars().count() > MAX_ID_CHARS
            || !ids.insert(saved.id.as_str())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved command ids must be non-empty, unique, and at most 80 characters",
            ));
        }
        normalize_fields(&saved.name, &saved.command)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }
    Ok(())
}

fn new_id(name: &str, command: &str) -> String {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let material = format!("{time}:{}:{sequence}:{name}:{command}", std::process::id());
    let digest = Sha256::digest(material.as_bytes());
    let suffix = digest[..10].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("cmd-{suffix}")
}

const fn default_append_enter() -> bool {
    true
}

pub(crate) fn store_path() -> PathBuf {
    crate::platform::dirs::data_dir().join(STORE_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_deletion_survives_reload_and_other_window_edits() {
        use crate::i18n::UiLanguage;
        use builtins::CommandPlatform;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(STORE_FILE);
        std::fs::write(&path, r#"{"version":1,"commands":[]}"#).unwrap();
        let mut first = SavedCommands::load_from(&path).unwrap();
        let mut other_window = first.clone();
        first.create_group("Containers").unwrap();
        let group = first.groups()[0].id.clone();
        first.move_to_group("builtin:docker_exec", Some(&group)).unwrap();
        first.remove("builtin:docker_exec").unwrap();
        first.remove("builtin:python_install_windows").unwrap();
        let custom = other_window.insert("My Git command", "git status", false).unwrap();
        let reloaded = SavedCommands::load_from(&path).unwrap();
        assert_eq!(reloaded.commands(), &[custom]);
        assert_eq!(reloaded.groups()[0].id, group);
        assert!(!reloaded.organization.membership.contains_key("builtin:docker_exec"));
        for language in [UiLanguage::EnUs, UiLanguage::ZhCn] {
            for platform in [CommandPlatform::Windows, CommandPlatform::Mac, CommandPlatform::Posix]
            {
                let commands = reloaded.builtin_commands(language, platform);
                assert!(commands.iter().all(|row| row.id != "builtin:docker_exec"
                    && row.id != "builtin:python_install_windows"));
                assert!(commands.iter().any(|row| row.id == "builtin:conda_create"));
            }
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(other_window.move_to_group("builtin:docker_exec", Some(&group)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn versioned_store_round_trips_and_rejects_invalid_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(STORE_FILE);
        let (saved, inserted) = mutate_store(&path, |store| {
            let commands = &mut store.commands;
            let (name, command) = normalize_fields(" Start backend ", " cargo run ")?;
            let saved =
                SavedCommand { id: "cmd-test".to_owned(), name, command, append_enter: false };
            commands.push(saved.clone());
            Ok(saved)
        })
        .unwrap();

        assert_eq!(inserted.name, "Start backend");
        assert_eq!(saved, SavedCommands::load_from(&path).unwrap());
        assert_eq!(saved.commands()[0].command, "cargo run");
        assert!(!saved.commands()[0].append_enter);
        assert!(normalize_fields("bad\nname", "echo ok").is_err());
        assert!(normalize_fields("name", "\0").is_err());
    }
}
