use super::*;

fn snapshot() -> Snapshot {
    let mut files = BTreeMap::new();
    for name in [
        "claude",
        "codex",
        "codex_config",
        "opencode",
        "pi",
        "manifest",
        "disabled",
        "pebrel-hook",
        "bridge.py",
        "shell.py",
        "bashrc",
        ".zshenv",
        ".zprofile",
        ".zshrc",
    ] {
        files.insert(
            name.into(),
            File { path: format!("/home/a b/'user/{name}"), sha256: None, content: None },
        );
    }
    Snapshot {
        version: 1,
        root: "/home/a b/'user".into(),
        python: "/usr/bin/python3".into(),
        files,
        providers: ["claude", "codex", "opencode", "pi"]
            .into_iter()
            .map(|name| (name.into(), true))
            .collect(),
        codex_version: "codex-cli 0.154.0".into(),
        codex_features: "hooks stable true".into(),
    }
}

fn put(snapshot: &mut Snapshot, name: &str, content: &str) {
    let file = snapshot.files.get_mut(name).unwrap();
    file.content = Some(content.into());
    file.sha256 = Some(digest(content));
}

fn apply(snapshot: &mut Snapshot, edits: Vec<Edit>) {
    for edit in edits {
        let file = snapshot.files.get_mut(&edit.name).unwrap();
        assert_eq!(file.sha256, edit.expected);
        file.sha256 = edit.content.as_deref().map(digest);
        file.content = edit.content;
    }
}

#[test]
fn install_repeat_upgrade_remove_restores_foreign_configuration() {
    let mut snapshot = snapshot();
    put(
        &mut snapshot,
        "codex_config",
        "# user\nnotify = ['user-notifier', 'argument']\nmodel = 'kept'\n",
    );
    put(
        &mut snapshot,
        "claude",
        r#"{"model":"kept","hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    );
    let edits = snapshot.plan(Action::Install).unwrap().unwrap();
    apply(&mut snapshot, edits);
    assert!(snapshot.raw("codex_config").unwrap().unwrap().contains("--chain"));
    assert!(snapshot.raw("codex").unwrap().unwrap().contains("PermissionRequest"));
    assert!(snapshot.plan(Action::Automatic).unwrap().unwrap().is_empty());
    let manifest_raw = snapshot.raw("manifest").unwrap().unwrap().to_owned();
    let mut manifest: Manifest = serde_json::from_str(&manifest_raw).unwrap();
    put(&mut snapshot, "bridge.py", "previous owned bridge\n");
    manifest.assets.insert("bridge.py".into(), digest("previous owned bridge\n"));
    put(&mut snapshot, "manifest", &serde_json::to_string(&manifest).unwrap());
    let edits = snapshot.plan(Action::Install).unwrap().unwrap();
    apply(&mut snapshot, edits);
    assert_eq!(snapshot.raw("bridge.py").unwrap(), Some(BRIDGE));
    let edits = snapshot.plan(Action::Remove).unwrap().unwrap();
    apply(&mut snapshot, edits);
    let config = snapshot.raw("codex_config").unwrap().unwrap();
    let doc = config.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(notify(&doc).unwrap(), Some(vec!["user-notifier".into(), "argument".into()]));
    assert!(config.contains("# user") && config.contains("model = 'kept'"));
    assert!(!config.contains("hooks = true"));
    let claude: Value = serde_json::from_str(snapshot.raw("claude").unwrap().unwrap()).unwrap();
    assert_eq!(claude["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(claude["model"], "kept");
    assert!(snapshot.raw("bridge.py").unwrap().is_none());
    assert!(snapshot.plan(Action::Automatic).unwrap().is_none());
    assert!(snapshot.plan(Action::Install).unwrap().is_some());
}

#[test]
fn edited_assets_commands_and_notify_are_preserved_without_partial_plan() {
    for target in ["bridge.py", "codex_config", "codex"] {
        let mut snapshot = snapshot();
        let edits = snapshot.plan(Action::Install).unwrap().unwrap();
        apply(&mut snapshot, edits);
        let value = match target {
            "codex_config" => "notify = ['custom']\n".into(),
            "codex" => {
                snapshot.raw("codex").unwrap().unwrap().replace("--hooks=full", "--user-customized")
            },
            _ => "user changed bridge".into(),
        };
        put(&mut snapshot, target, &value);
        assert!(snapshot.plan(Action::Install).is_err(), "{target}");
        assert!(snapshot.plan(Action::Remove).is_err(), "{target}");
        assert_eq!(snapshot.raw(target).unwrap(), Some(value.as_str()));
    }
}

#[test]
fn opt_out_and_older_clients_keep_notify_without_claiming_native_coverage() {
    let mut snapshot = snapshot();
    put(&mut snapshot, "codex_config", "[features]\nhooks = false\n");
    let edits = snapshot.plan(Action::Install).unwrap().unwrap();
    apply(&mut snapshot, edits);
    assert!(snapshot.raw("codex").unwrap().is_none());
    assert!(snapshot.raw("codex_config").unwrap().unwrap().contains("notify"));
    assert!(snapshot.raw("codex_config").unwrap().unwrap().contains("hooks = false"));
    let mut old = super::tests::snapshot();
    old.codex_features.clear();
    let edits = old.plan(Action::Automatic).unwrap().unwrap();
    apply(&mut old, edits);
    assert!(old.raw("codex").unwrap().is_none());
}

#[test]
fn transport_requests_are_framed_and_bootstrap_keeps_token_out_of_assets() {
    let snapshot = snapshot();
    assert!(snapshot.bootstrap("bad token").is_err());
    let token = "0123456789abcdef0123456789abcdef";
    let command = snapshot.bootstrap(token).unwrap();
    assert!(command.contains("'\\''user/shell.py'"));
    let plan = snapshot.plan(Action::Install).unwrap().unwrap();
    assert!(!serde_json::to_string(&plan).unwrap().contains(token));
    assert!(response("banner\nPEBREL_INTEGRATION={\"version\":1,\"applied\":true}\n").is_ok());
    assert!(response("PEBREL_INTEGRATION={\"version\":1,\"error\":\"ValueError\"}\n").is_err());
}

/// Cross-language acceptance driver: the supplied snapshot must come from an
/// isolated SSH test account. It exports production plans, not a second policy.
#[test]
#[ignore = "requires PEBREL_SSH_ACCEPTANCE_DIR with an isolated snapshot.json"]
fn export_isolated_ssh_acceptance_plan() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("PEBREL_SSH_ACCEPTANCE_DIR").unwrap());
    let mut snapshot: Snapshot =
        serde_json::from_str(&std::fs::read_to_string(directory.join("snapshot.json")).unwrap())
            .unwrap();
    let install = snapshot.plan(Action::Install).unwrap().unwrap();
    std::fs::write(
        directory.join("install.py"),
        request(&json!({"action":"apply", "files":install})),
    )
    .unwrap();
    std::fs::write(
        directory.join("bootstrap.txt"),
        snapshot.bootstrap("0123456789abcdef0123456789abcdef").unwrap(),
    )
    .unwrap();
    apply(&mut snapshot, install);
    assert!(snapshot.plan(Action::Automatic).unwrap().unwrap().is_empty());
    let remove = snapshot.plan(Action::Remove).unwrap().unwrap();
    std::fs::write(
        directory.join("remove.py"),
        request(&json!({"action":"apply", "files":remove})),
    )
    .unwrap();
}

#[test]
fn wsl_installs_guest_command_hooks_and_preserves_custom_notify() {
    let mut snapshot = snapshot().for_wsl();
    put(&mut snapshot, "codex_config", "notify = ['user-notifier']\nmodel = 'kept'\n");
    let edits = snapshot.plan(Action::Install).unwrap().unwrap();
    assert!(!edits.iter().any(|edit| matches!(edit.name.as_str(), "opencode" | "pi")));
    apply(&mut snapshot, edits);
    let hooks: Value = serde_json::from_str(snapshot.raw("codex").unwrap().unwrap()).unwrap();
    for name in [
        "SessionStart",
        "UserPromptSubmit",
        "PermissionRequest",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "Interrupt",
        "SessionEnd",
    ] {
        let group = hooks["hooks"][name].to_string();
        assert!(group.contains("--hooks=full"), "{name}: {group}");
        assert!(group.contains("/home/"));
        assert!(!group.contains(".exe"));
    }
    assert!(snapshot.raw("codex_config").unwrap().unwrap().contains("user-notifier"));
    assert!(snapshot.plan(Action::Automatic).unwrap().unwrap().is_empty());
}

#[test]
fn wsl_replaces_the_legacy_windows_helper_without_chaining_a_dead_executable() {
    let mut snapshot = snapshot().for_wsl();
    put(
        &mut snapshot,
        "codex_config",
        "notify = ['D:/Program Files/Nebula Terminal/runtime/nebula-hook.exe', 'codex']\n",
    );
    let edits = snapshot.plan(Action::Install).unwrap().unwrap();
    apply(&mut snapshot, edits);
    let config =
        snapshot.raw("codex_config").unwrap().unwrap().parse::<toml_edit::DocumentMut>().unwrap();
    let argv = notify(&config).unwrap().unwrap();
    assert_eq!(argv.len(), 2);
    assert!(argv[0].starts_with("/home/"));
    assert_eq!(argv[1], "codex");
    assert!(!argv[0].contains(".exe"));
}
