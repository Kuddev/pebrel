use super::*;

fn fixture(directory: &Path) -> NebulaHistory {
    let mut history = NebulaHistory::default();
    for (scope, record) in [
        (HistoryScope::Local, r#"{"cmd":"echo local"}"#),
        (HistoryScope::Wsl("test".into()), r#"{"scope":"wsl","target":"test","cmd":"echo guest"}"#),
        (
            HistoryScope::Ssh("user@host".into()),
            r#"{"scope":"ssh","target":"user@host","cmd":"echo remote"}"#,
        ),
    ] {
        let path = directory.join(scope.file_name());
        std::fs::write(&path, format!("{record}\n")).unwrap();
        history.load_file(&scope, &path);
    }
    history
}

#[test]
fn command_history_clear_removes_all_categories_and_survives_reload() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = fixture(directory.path());
    let saved = directory.path().join("pebrel_commands.json");
    let shell_history = directory.path().join(".bash_history");
    std::fs::write(&saved, "saved fixture").unwrap();
    std::fs::write(&shell_history, "shell fixture").unwrap();
    assert_eq!(history.pools.len(), 3);
    history.clear_at(directory.path()).unwrap();
    assert!(history.pools.is_empty());
    for file in history_file_names() {
        assert!(!directory.path().join(file).exists());
    }
    let mut reloaded = NebulaHistory::default();
    reloaded.load_file(&HistoryScope::Local, &directory.path().join(LOCAL_HISTORY_FILE));
    assert!(reloaded.hint(&HistoryScope::Local, "echo ").is_none());
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "saved fixture");
    assert_eq!(std::fs::read_to_string(shell_history).unwrap(), "shell fixture");
    // A missing file is already cleared, including an in-memory-only category.
    reloaded.pools.entry(HistoryScope::Local).or_default().insert("echo memory".into());
    reloaded.clear_at(directory.path()).unwrap();
    assert!(reloaded.pools.is_empty());
}

#[test]
fn command_history_clear_reports_partial_failure_and_preserves_failed_entries() {
    let directory = tempfile::tempdir().unwrap();
    let mut history = fixture(directory.path());
    let blocked = directory.path().join(WSL_HISTORY_FILE);
    std::fs::remove_file(&blocked).unwrap();
    std::fs::create_dir(&blocked).unwrap();
    let error = history.clear_at(directory.path()).unwrap_err();
    assert!(error.contains(WSL_HISTORY_FILE));
    assert!(history.hint(&HistoryScope::Local, "echo ").is_none());
    assert!(history.hint(&HistoryScope::Ssh("user@host".into()), "echo ").is_none());
    assert_eq!(history.hint(&HistoryScope::Wsl("test".into()), "echo "), Some("guest"));
    std::fs::remove_dir(blocked).unwrap();
    history.clear_at(directory.path()).unwrap();
    assert!(history.pools.is_empty());
}

#[test]
fn disabling_persistence_keeps_scoped_suggestions_until_restart() {
    let directory = tempfile::tempdir().unwrap();
    let scopes =
        [HistoryScope::Local, HistoryScope::Wsl("ubuntu".into()), HistoryScope::Ssh("box".into())];
    let mut saved = NebulaHistory::load_at(directory.path(), true);
    for scope in &scopes {
        saved.record(scope, "echo saved", "/");
    }
    let mut memory = NebulaHistory::load_at(directory.path(), false);
    for scope in &scopes {
        let path = directory.path().join(scope.file_name());
        let before = std::fs::read(&path).unwrap();
        assert_eq!(memory.hint(scope, "echo s"), Some("aved"));
        memory.record(scope, "echo transient", "/");
        assert_eq!(memory.hint(scope, "echo t"), Some("ransient"));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    let mut resumed = NebulaHistory::load_at(directory.path(), true);
    for scope in &scopes {
        assert_eq!(resumed.hint(scope, "echo t"), None);
        resumed.record(scope, "echo new", "/");
    }
    let reloaded = NebulaHistory::load_at(directory.path(), false);
    for scope in &scopes {
        assert_eq!(reloaded.hint(scope, "echo n"), Some("ew"));
        assert_eq!(reloaded.hint(scope, "echo s"), Some("aved"));
    }
}

#[test]
fn memory_only_history_keeps_command_text_out_of_diagnostics() {
    const CHILD: &str = "PEBREL_TEST_HISTORY_DIAGNOSTICS_CHILD";
    const MARKER: &str = "echo synthetic-history-marker";
    if std::env::var_os(CHILD).is_some() {
        let mut history = NebulaHistory::load();
        history.record(&HistoryScope::Local, MARKER, "/test");
        assert!(history.hint(&HistoryScope::Local, "echo synthetic").is_some());
        return;
    }

    for mode in ["off", "unreadable", "default"] {
        let directory = tempfile::tempdir().unwrap();
        let settings = directory.path().join("pebrel_settings.txt");
        match mode {
            "off" => std::fs::write(settings, "command_history_persist=0\n").unwrap(),
            "unreadable" => std::fs::create_dir(settings).unwrap(),
            _ => {},
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("memory_only_history_keeps_command_text_out_of_diagnostics")
            .env(CHILD, "1")
            .env("PEBREL_CONFIG_DIR", directory.path())
            .env("PEBREL_DEBUG_LOG", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

        let log = std::fs::read_to_string(directory.path().join("pebrel_debug.log")).unwrap();
        assert!(log.contains("history_load"), "diagnostics must be active");
        assert_eq!(log.contains(MARKER), mode == "default");
        assert_eq!(directory.path().join(LOCAL_HISTORY_FILE).is_file(), mode == "default");
    }
}
