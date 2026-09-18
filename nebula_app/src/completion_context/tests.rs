use super::*;

fn pane(env: SuggestEnv, token: &str) -> NebulaPaneState {
    let mut state = NebulaPaneState::default();
    state.suggest_env = env;
    state.cwd = "/parent".into();
    state.completion_shell_report(SHELL_VAR, token);
    state
}

#[test]
fn typed_ssh_never_uses_local_history_and_only_parent_identity_restores_it() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_submitted("ssh user@box");
    let remote = state.suggest_env.history_scope();
    assert!(matches!(remote, HistoryScope::Ssh(_)));
    // A plain fish shell keeps using its remote history until the enclosing
    // pwsh reports its prompt again. Individual remote commands do not pop it.
    state.completion_submitted("set -gx FISH_VAR value");
    state.completion_submitted("exit");
    assert_eq!(state.suggest_env.history_scope(), remote);
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env.history_scope(), HistoryScope::Local);
    assert_eq!(state.cwd, "/parent");
}

#[test]
fn reported_nested_shells_restore_their_own_ancestors() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_submitted("pebrel ssh bastion");
    state.completion_shell_report(SHELL_VAR, "bash:bastion");
    let bastion = state.suggest_env.clone();
    assert!(matches!(bastion, SuggestEnv::Shell { .. }));
    assert!(!bastion.can_query_remote_paths());
    state.cwd = "/bastion".into();
    state.completion_submitted("ssh inner");
    state.completion_shell_report(SHELL_VAR, "zsh:inner");
    assert_ne!(state.suggest_env, bastion);
    state.completion_submitted("exit");
    assert_ne!(state.suggest_env, bastion);
    state.completion_shell_report(SHELL_VAR, "bash:bastion");
    assert_eq!(state.suggest_env, bastion);
    assert_eq!(state.cwd, "/bastion");
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env, SuggestEnv::Local);
}

#[test]
fn failed_or_cancelled_connections_restore_without_using_an_exit_status() {
    let root = SuggestEnv::Wsl { distro: "Debian".into() };
    let mut state = pane(root.clone(), "wsl|Debian|bash:parent");
    for command in ["ssh nonexistent.invalid", "ssh host", "wsl.exe -d Missing"] {
        state.completion_submitted(command);
        assert_ne!(state.suggest_env, root);
        state.completion_shell_report(SHELL_VAR, "wsl|Debian|bash:parent");
        assert_eq!(state.suggest_env, root);
    }
}

#[test]
fn transitions_drop_ghost_popup_suppression_mirrors_and_pending_directory() {
    let mut state = pane(SuggestEnv::Ssh { destination: "outer".into() }, "bash:outer");
    state.suggestion = "outer-path".into();
    state.pending_remote_dir = Some("/outer".into());
    state.completion_suppressed_line = Some("ls".into());
    state.line_buf = "ssh inner".into();
    state.screen_line = state.line_buf.clone();
    state.completion_items.push(crate::display::NebulaCompletionItem {
        label: "outer-path".into(),
        insert: "outer-path".into(),
        replace_chars: 0,
        kind: crate::display::NebulaCompletionKind::History,
    });
    state.completion_selected = Some(0);
    state.completion_submitted("ssh inner");
    assert!(state.suggestion.is_empty());
    assert!(state.completion_items.is_empty());
    assert!(state.completion_selected.is_none());
    assert!(state.pending_remote_dir.is_none());
    assert!(state.completion_suppressed_line.is_none());
    assert!(state.screen_line.is_empty());
    assert!(state.line_buf.is_empty());
    assert!(state.cwd.is_empty());
}

#[test]
fn typed_route_identity_includes_parent_and_connection_options() {
    fn scope(root: SuggestEnv, line: &str) -> HistoryScope {
        let mut state = pane(root, "parent");
        state.completion_submitted(line);
        state.completion_shell_report(SHELL_VAR, "child");
        state.suggest_env.history_scope()
    }
    let direct = scope(SuggestEnv::Local, "ssh alias");
    assert_ne!(direct, HistoryScope::Ssh("alias".into()));
    assert_ne!(direct, scope(SuggestEnv::Local, "ssh -p 2200 alias"));
    assert_ne!(direct, scope(SuggestEnv::Local, "ssh -F other-config alias"));
    assert_ne!(direct, scope(SuggestEnv::Ssh { destination: "bastion".into() }, "ssh alias"));
    assert_eq!(direct, scope(SuggestEnv::Local, "ssh alias"));
}

#[test]
fn accepted_shell_text_catches_native_recall_and_does_not_duplicate_transition() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_submitted("");
    state.completion_shell_report(COMMAND_VAR, "pwsh:parent\nssh remembered-host");
    assert!(matches!(state.suggest_env, SuggestEnv::Shell { .. }));
    state.completion_shell_report(COMMAND_VAR, "pwsh:parent\nssh remembered-host");
    state.completion_shell_report(SHELL_VAR, "bash:remote");
    assert!(matches!(state.suggest_env, SuggestEnv::Shell { .. }));
}

#[test]
fn another_shell_on_the_same_host_keeps_that_hosts_history() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_shell_report(SHELL_VAR, "unexpected:child");
    assert_eq!(state.suggest_env.history_scope(), HistoryScope::Local);
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env, SuggestEnv::Local);
}

#[test]
fn execution_arguments_separate_variable_targets_and_restore_before_next_command() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_submitted("ssh $targetHost");
    state.completion_shell_report(ARGV_VAR, "pwsh:parent\nssh\0one.example");
    let first = state.suggest_env.clone();
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env, SuggestEnv::Local);
    state.completion_submitted("ssh $targetHost");
    state.completion_shell_report(ARGV_VAR, "pwsh:parent\nssh\0two.example");
    assert_ne!(state.suggest_env, first);
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env, SuggestEnv::Local);
    // An actual argument may contain shell operators as literal text.
    state.completion_shell_report(
        ARGV_VAR,
        "pwsh:parent\nssh\0-o\0ProxyCommand=ssh -W %h:%p jump\0one.example",
    );
    assert!(matches!(state.suggest_env, SuggestEnv::Shell { .. }));
}

#[test]
fn an_empty_input_snapshot_does_not_disable_remote_history() {
    let mut state = pane(SuggestEnv::Local, "pwsh:parent");
    state.completion_submitted("ssh fish-host");
    let remote = state.suggest_env.clone();
    state.completion_submitted("");
    assert_eq!(state.suggest_env, remote);
    state.completion_shell_report(SHELL_VAR, "pwsh:parent");
    assert_eq!(state.suggest_env, SuggestEnv::Local);
}

#[test]
fn launch_classifies_named_default_wsl_and_direct_ssh() {
    assert_eq!(launch_environment("pwsh.exe", &[]), SuggestEnv::Local);
    assert!(matches!(
        launch_environment(r"C:\Windows\System32\wsl.exe", &[]),
        SuggestEnv::Wsl { .. }
    ));
    assert_eq!(
        launch_environment("wsl.exe", &["-d".into(), "Ubuntu".into()]),
        SuggestEnv::Wsl { distro: "Ubuntu".into() }
    );
    assert!(matches!(launch_environment("ssh.exe", &["host".into()]), SuggestEnv::Shell { .. }));
}

#[test]
fn unnamed_wsl_uses_the_distro_reported_by_its_shell() {
    let state = pane(SuggestEnv::Wsl { distro: String::new() }, "wsl|Ubuntu|bash:guest");
    assert_eq!(state.suggest_env, SuggestEnv::Wsl { distro: "Ubuntu".into() });
}

#[test]
fn only_an_actual_ssh_or_wsl_invocation_changes_history_scope() {
    for line in [
        "ssh -p 2200 user@host",
        "SSH.EXE host",
        "pebrel ssh host",
        "wsl.exe",
        r#"& 'C:\Program Files\OpenSSH\ssh.exe' host"#,
        r#"& 'C:\Windows\System32\wsl.exe' -d Ubuntu"#,
    ] {
        assert!(connection_environment(&SuggestEnv::Local, line).is_some(), "{line}");
    }
    for line in [
        "",
        "git status",
        "echo ssh host",
        "echo 'ssh host; exit'",
        "ssh -N host",
        "ssh host ls",
        "echo ready; ssh host",
        "sudo ls",
        "fish",
        "exit",
    ] {
        assert!(connection_environment(&SuggestEnv::Local, line).is_none(), "{line}");
    }
}
