//! 冷启动、Runtime 与管理员交接共用的启动参数解析；窗口创建留在注册表。

use super::{ApiError, PathBuf, WorkspaceStartup};

/// Resolve explicit shell identity before any window or terminal is created.
pub(super) fn runtime_window_startup(
    cwd: Option<PathBuf>,
    shell_id: Option<&str>,
) -> Result<WorkspaceStartup, ApiError> {
    match shell_id {
        Some(id) => super::super::shell_launch::resolve_shell_at(id, cwd.as_deref())
            .map(|launch| WorkspaceStartup::LaunchTerminal { cwd, launch })
            .map_err(|error| ApiError::new("invalid_shell", error)),
        None => Ok(WorkspaceStartup::NewTerminal { cwd }),
    }
}

pub(super) fn initial_startup(
    cwd: Option<PathBuf>,
    command: Option<crate::config::ui_config::Program>,
    isolated: bool,
) -> WorkspaceStartup {
    if let Some(command) = command {
        let program = command.program().to_owned();
        let name = program.rsplit(['/', '\\']).next().unwrap_or(&program).to_owned();
        let launch =
            crate::session::LaunchSession::Shell { name, program, args: command.args().to_vec() };
        let launch = super::super::shell_launch::at_explicit_directory(launch, cwd.as_deref());
        return WorkspaceStartup::LaunchTerminal { cwd, launch };
    }
    if isolated {
        WorkspaceStartup::NewTerminal { cwd }
    } else if cwd.is_some() {
        WorkspaceStartup::Configured { cwd }
    } else {
        WorkspaceStartup::RestoreOrDefault
    }
}

#[cfg(test)]
mod startup_tests {
    #[cfg(feature = "gpui-test-support")]
    use super::super::open_runtime_window;
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn invalid_explicit_shell_cannot_create_a_runtime_window(cx: &mut gpui::TestAppContext) {
        let error = cx
            .update(|cx| {
                open_runtime_window(cx, None, Some("pebrel-missing-shell-id".into()), false)
            })
            .unwrap_err();
        assert_eq!(error.code, "invalid_shell");
        assert!(cx.update(|cx| cx.windows().is_empty()));
    }

    #[test]
    fn default_runtime_startup_keeps_the_requested_directory() {
        let cwd = Some(PathBuf::from("project"));
        assert!(
            matches!(runtime_window_startup(cwd.clone(), None).unwrap(), WorkspaceStartup::NewTerminal { cwd: actual } if actual == cwd)
        );
    }

    #[test]
    fn explicit_program_is_kept_with_its_arguments_and_directory() {
        let command = crate::config::ui_config::Program::WithArgs {
            program: "shell.exe".into(),
            args: vec!["--literal=two words".into()],
        };
        let cwd = Some(PathBuf::from("C:/work area"));
        let WorkspaceStartup::LaunchTerminal { launch, cwd: actual_cwd } =
            initial_startup(cwd.clone(), Some(command), true)
        else {
            panic!("explicit launch was discarded");
        };
        assert_eq!(actual_cwd, cwd);
        assert_eq!(
            launch,
            crate::session::LaunchSession::Shell {
                name: "shell.exe".into(),
                program: "shell.exe".into(),
                args: vec!["--literal=two words".into()]
            }
        );
    }

    #[test]
    fn explicit_wsl_command_encodes_the_requested_directory_before_pty_defaults() {
        let host = tempfile::tempdir().unwrap();
        for cwd in [host.path(), std::path::Path::new("/home/pebrel-fixture/project space")] {
            let command = crate::config::ui_config::Program::WithArgs {
                program: "wsl.exe".into(),
                args: vec!["-d".into(), "Ubuntu".into(), "--exec".into(), "pwd".into()],
            };
            let WorkspaceStartup::LaunchTerminal { launch, cwd: actual_cwd } =
                initial_startup(Some(cwd.into()), Some(command), false)
            else {
                panic!("explicit WSL launch was discarded");
            };
            let crate::session::LaunchSession::Shell { program, args, .. } = launch else {
                panic!("expected command shell");
            };
            let mut options = nebula_terminal::tty::Options {
                shell: Some(nebula_terminal::tty::Shell::new(program, args)),
                working_directory: actual_cwd,
                ..Default::default()
            };
            crate::platform::launch_directory::prepare_wsl(&mut options, Some("/different"));
            let directory = cwd.to_str().unwrap();
            let encoded =
                if directory.contains(' ') { format!("\"{directory}\"") } else { directory.into() };
            assert_eq!(
                options.shell.unwrap().args(),
                &["--cd", &encoded, "-d", "Ubuntu", "--exec", "pwd"]
            );
        }
    }

    #[test]
    fn privileged_startup_never_restores_the_ordinary_session() {
        assert!(matches!(initial_startup(None, None, false), WorkspaceStartup::RestoreOrDefault));
        assert!(matches!(
            initial_startup(Some(PathBuf::from("project")), None, false),
            WorkspaceStartup::Configured { cwd: Some(_) }
        ));
        assert!(matches!(
            initial_startup(None, None, true),
            WorkspaceStartup::NewTerminal { cwd: None }
        ));
    }
}
