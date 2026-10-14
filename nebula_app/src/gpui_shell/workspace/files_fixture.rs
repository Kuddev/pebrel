//! Synthetic terminal owners for file interaction tests. No shell, credentials,
//! SSH config resolution or remote authentication is launched by these panes.
use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;

pub(super) const SIDE_EFFECT_OVERRIDES: &[&str] = &[
    "PEBREL_CONFIG_FILE",
    "NEBULA_CONFIG_FILE",
    "PEBREL_GPUI_CONFIG",
    "NEBULA_GPUI_CONFIG",
    "NEBULA_GPUI_OPEN_DOC",
    "SSH_AUTH_SOCK",
    "PEBREL_FILES_NATIVE_QA",
    "NEBULA_PTY_RECORD",
];
const WORKER_SWITCHES: [&str; 5] = [
    "auto_check_updates",
    "auto_download_updates",
    "restore_session",
    "resume_ai",
    "refresh_environment",
];

pub(super) fn prepare(directory: &Path, language: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(directory.join(".ssh"))?;
    std::fs::write(directory.join(".ssh/config"), "")?;
    let mut updates = nebula_settings::AgentHook::all_updates(false);
    updates.extend(WORKER_SWITCHES.map(|key| (key, "0".into())));
    updates.extend([("language", language.into()), ("tray", "0".into())]);
    std::fs::write(
        directory.join("pebrel_settings.txt"),
        nebula_settings::apply_updates("", &updates),
    )
}

pub(super) fn child_command(directory: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    for name in SIDE_EFFECT_OVERRIDES {
        command.env_remove(name);
    }
    command.env("PEBREL_CONFIG_DIR", directory).env("PEBREL_TEST_SSH_HOME", directory);
    command
}

pub(super) fn assert_isolated(directory: &Path) {
    assert_eq!(Some(directory.as_os_str()), std::env::var_os("PEBREL_CONFIG_DIR").as_deref());
    assert_eq!(nebula_settings::settings_path(), directory.join("pebrel_settings.txt"));
    let raw = nebula_settings::RawSettings::try_load().unwrap();
    for (key, _) in nebula_settings::AgentHook::all_updates(false)
        .into_iter()
        .chain(WORKER_SWITCHES.map(|key| (key, "0".into())))
    {
        assert_eq!(raw.bool_on(key), Some(false), "unexpected worker setting: {key}");
    }
    let settings = nebula_settings::RuntimeSettings::from_raw(&raw);
    assert!(!settings.auto_check_updates && !settings.auto_download_updates);
    assert!(!settings.restore_session && !settings.resume_ai && !settings.refresh_environment);
    assert_eq!(crate::ssh::ssh_config_path(), Some(directory.join(".ssh/config")));
    assert_eq!(crate::ssh::read_ssh_config().unwrap(), "");
    for name in SIDE_EFFECT_OVERRIDES {
        assert!(std::env::var_os(name).is_none(), "unexpected side-effect override: {name}");
    }
}

pub(super) fn local_launch(directory: &Path) -> TerminalLaunch {
    let program = directory.join("pebrel-test-missing-shell-executable");
    assert!(program.is_absolute() && !program.exists());
    TerminalLaunch::Local {
        cwd: Some(directory.to_path_buf()),
        shell: Some(nebula_terminal::tty::Shell::new(
            program.to_str().expect("owned fixture path is UTF-8").into(),
            vec![],
        )),
        shell_name: None,
    }
}

pub(super) fn append_pane(
    workspace: &mut NebulaWorkspace,
    directory: &Path,
    destination: Option<&str>,
    ready: bool,
    snapshot: Option<&str>,
    window: &mut Window,
    cx: &mut Context<NebulaWorkspace>,
) -> u64 {
    let mut pane = workspace.new_pane((80, 24), local_launch(directory), None, window, cx);
    let id = pane.id;
    pane.custom_name =
        Some(format!("{}-{id}", if destination.is_some() { "SSH" } else { "local" }));
    pane.view.update(cx, |view, cx| {
        let _ = view.install_completion_test_session();
        view.cwd =
            if destination.is_some() { "/target".into() } else { directory.display().to_string() };
        view.ssh_destination = destination.map(str::to_owned);
        if ready {
            view.files_test_ssh_ready(cx);
        }
    });
    if let Some(WorkspaceTab::Terminal { panes, tree, .. }) = workspace.tabs.first_mut() {
        tree.split_leaf(tree.first_leaf(), id, SplitDirection::LeftRight, 0.5);
        panes.push(pane);
    } else {
        workspace.tabs.push(WorkspaceTab::Terminal {
            panes: vec![pane],
            tree: SplitTree::Leaf(id),
            focused: id,
            zoomed: false,
            broadcast: false,
        });
        workspace.tab_meta.push(TabMeta::default());
    }
    if let Some((destination, path)) = destination.zip(snapshot) {
        workspace.remote_browser.files_test_snapshot(id, destination, path);
    }
    cx.notify();
    id
}
