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
