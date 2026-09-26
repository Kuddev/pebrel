//! A bounded, coalescing save worker for local backup preferences. It outlives a
//! closing settings view just long enough to persist the last accepted edit.
use super::BackupRemoteConfig;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

#[derive(Default)]
struct Pending {
    revision: u64,
    next: Option<(u64, BackupRemoteConfig)>,
    result: Option<(u64, Result<(), String>)>,
    running: bool,
}

#[derive(Clone)]
pub(crate) struct ConfigWriter {
    state: Arc<Mutex<Pending>>,
    path: PathBuf,
}

impl Default for ConfigWriter {
    fn default() -> Self {
        // All settings windows share a queue, so an older worker cannot overwrite
        // a later accepted edit after its owning window has closed.
        static WRITER: OnceLock<ConfigWriter> = OnceLock::new();
        WRITER.get_or_init(|| Self { state: Arc::default(), path: super::config_path() }).clone()
    }
}

impl ConfigWriter {
    pub fn submit(&self, config: BackupRemoteConfig) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.revision += 1;
        let revision = state.revision;
        state.next = Some((revision, config));
        if !state.running {
            state.running = true;
            let writer = self.clone();
            let spawned =
                std::thread::Builder::new().name("backup-preferences".into()).spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(250));
                        let next =
                            writer.state.lock().unwrap_or_else(|e| e.into_inner()).next.take();
                        if let Some((revision, config)) = next {
                            let result = config.save_at(&writer.path);
                            writer.state.lock().unwrap_or_else(|e| e.into_inner()).result =
                                Some((revision, result));
                        }
                        let mut state = writer.state.lock().unwrap_or_else(|e| e.into_inner());
                        if state.next.is_none() {
                            state.running = false;
                            break;
                        }
                    }
                });
            if let Err(error) = spawned {
                state.running = false;
                state.next = None;
                state.result = Some((revision, Err(error.to_string())));
            }
        }
        revision
    }

    pub fn result(&self, revision: u64) -> Option<Result<(), String>> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .result
            .as_ref()
            .filter(|(done, _)| *done >= revision)
            .map(|(_, result)| result.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_design_backup_autosave_keeps_last_edit_and_selected_scope() {
        let directory = tempfile::tempdir().unwrap();
        let writer =
            ConfigWriter { state: Arc::default(), path: directory.path().join("backup.txt") };
        let mut config = BackupRemoteConfig::default();
        config.protocol = super::super::BackupProtocol::Folder;
        config.folder_path = "first".into();
        writer.submit(config.clone());
        config.folder_path = "last".into();
        config.selection.ssh = true;
        let revision = writer.submit(config.clone());
        let limit = std::time::Instant::now() + Duration::from_secs(5);
        while writer.result(revision).is_none() {
            assert!(std::time::Instant::now() < limit);
            std::thread::sleep(Duration::from_millis(10));
        }
        writer.result(revision).unwrap().unwrap();
        assert_eq!(
            BackupRemoteConfig::parse(&std::fs::read_to_string(&writer.path).unwrap()),
            config
        );
        let original = std::fs::read(&writer.path).unwrap();
        config.folder_path = "bad\nprotocol=s3".into();
        assert!(config.save_at(&writer.path).is_err());
        assert_eq!(std::fs::read(&writer.path).unwrap(), original);
    }

    #[test]
    fn pairing_design_backup_save_survives_view_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup.txt");
        let writer = ConfigWriter { state: Arc::default(), path: path.clone() };
        let mut config = BackupRemoteConfig::default();
        config.selection.command_history = true;
        writer.submit(config.clone());
        drop(writer);
        let limit = std::time::Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(std::time::Instant::now() < limit);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(BackupRemoteConfig::parse(&std::fs::read_to_string(path).unwrap()), config);
    }

    #[test]
    fn pairing_design_backup_lists_owned_archives_and_rejects_path_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let config = BackupRemoteConfig {
            protocol: super::super::BackupProtocol::Folder,
            folder_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        for name in
            ["pebrel-backup-20260918-010101.nbk", "nebula-backup-20260917-010101.nbk", "other.txt"]
        {
            std::fs::write(directory.path().join(name), b"encrypted fixture").unwrap();
        }
        let names = super::super::snapshots(&config).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], "pebrel-backup-20260918-010101.nbk");
        assert!(super::super::pull_from(&config, Some("../other.txt")).is_err());
        assert_eq!(
            super::super::pull_from(&config, Some(&names[1])).unwrap().1,
            b"encrypted fixture"
        );
        assert!(directory.path().join("other.txt").is_file());
    }
}
