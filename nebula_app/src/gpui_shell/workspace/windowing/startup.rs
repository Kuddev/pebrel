//! First-window launch intent, session selection, and visibility.

use super::*;

pub(crate) fn open_initial_window(
    cx: &mut App,
    ai_events: std::sync::mpsc::Receiver<crate::ai_hook::AiHookEvent>,
    shell_events: std::sync::mpsc::Receiver<GpuiShellEvent>,
    initial_cwd: Option<PathBuf>,
    initial_command: Option<crate::config::ui_config::Program>,
    shell_id: Option<String>,
    launched_at_login: bool,
) {
    if !crate::platform::elevation::requires_isolation()
        && let Some(sessions) = crate::update_download::handoff::restore_ticket()
    {
        let mut sessions = sessions.into_iter();
        if let Some(first) = sessions.next() {
            open_workspace_window(
                cx,
                WorkspaceStartup::RestoreUpdate(first),
                Some(ai_events),
                Some(shell_events),
                true,
                WindowRole::Regular,
            )
            .expect("failed to reopen updated workspace");
            for session in sessions {
                if let Err(error) = open_workspace_window(
                    cx,
                    WorkspaceStartup::RestoreUpdate(session),
                    None,
                    None,
                    true,
                    WindowRole::Regular,
                ) {
                    log::warn!("Could not reopen an updated window: {error}");
                }
            }
            return;
        }
    }
    let startup = if shell_id.is_some() {
        match runtime_window_startup(initial_cwd, shell_id.as_deref()) {
            Ok(startup) => startup,
            Err(error) => {
                log::error!("Could not resolve requested shell: {error:?}");
                return;
            },
        }
    } else {
        initial_startup(
            initial_cwd,
            initial_command,
            crate::platform::elevation::requires_isolation(),
        )
    };
    let start_hidden = initial_window_hidden(
        &startup,
        &nebula_settings::RuntimeSettings::load(),
        launched_at_login,
    );
    open_workspace_window_with_visibility(
        cx,
        startup,
        Some(ai_events),
        Some(shell_events),
        true,
        WindowRole::Regular,
        start_hidden,
    )
    .expect("failed to open Pebrel GPUI window");
}

fn initial_window_hidden(
    startup: &WorkspaceStartup,
    settings: &nebula_settings::RuntimeSettings,
    launched_at_login: bool,
) -> bool {
    matches!(startup, WorkspaceStartup::RestoreOrDefault)
        && crate::platform::startup::start_hidden(settings, launched_at_login)
}

fn initial_startup(
    cwd: Option<PathBuf>,
    command: Option<crate::config::ui_config::Program>,
    isolated: bool,
) -> WorkspaceStartup {
    if let Some(command) = command {
        let program = command.program().to_owned();
        let name = program.rsplit(['/', '\\']).next().unwrap_or(&program).to_owned();
        return WorkspaceStartup::LaunchTerminal {
            cwd,
            launch: crate::session::LaunchSession::Shell {
                name,
                program,
                args: command.args().to_vec(),
            },
        };
    }
    if cwd.is_some() || isolated {
        WorkspaceStartup::NewTerminal { cwd }
    } else {
        WorkspaceStartup::RestoreOrDefault
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn launch_visibility_reaches_the_actual_workspace(cx: &mut gpui::TestAppContext) {
        use nebula_settings::{RawSettings, RuntimeSettings};

        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            super::super::super::init(cx);
            initialize(cx, crate::runtime_api::RuntimeHub::new());
        });
        let settings = RuntimeSettings::from_raw(&RawSettings::from_text("silent_start=1\ntray=1"));
        for launched_at_login in [false, true] {
            let hidden = initial_window_hidden(
                &WorkspaceStartup::RestoreOrDefault,
                &settings,
                launched_at_login,
            );
            let (_, workspace) = cx.update(|cx| {
                // An empty workspace avoids starting a real user's shell/session;
                // visibility uses the same constructor as the first process window.
                open_workspace_window_with_visibility(
                    cx,
                    WorkspaceStartup::Empty,
                    None,
                    None,
                    true,
                    WindowRole::Regular,
                    hidden,
                )
                .unwrap()
            });
            assert_eq!(workspace.read_with(cx, |workspace, _| workspace.window_hidden), hidden);
        }
    }

    #[test]
    fn only_login_session_restore_can_start_hidden() {
        use nebula_settings::{RawSettings, RuntimeSettings};

        let settings = RuntimeSettings::from_raw(&RawSettings::from_text("silent_start=1\ntray=1"));
        let restored = initial_startup(None, None, false);
        assert!(!initial_window_hidden(&restored, &settings, false));
        assert_eq!(
            initial_window_hidden(&restored, &settings, true),
            crate::platform::CAPABILITIES.hide_window_on_close
        );
        for startup in [
            initial_startup(Some(PathBuf::from("project")), None, false),
            initial_startup(
                None,
                Some(crate::config::ui_config::Program::Just("sh".into())),
                false,
            ),
            initial_startup(None, None, true),
            WorkspaceStartup::RestoreUpdate(crate::session::Session::new(0, vec![])),
            WorkspaceStartup::Empty,
        ] {
            assert!(!initial_window_hidden(&startup, &settings, true));
        }
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn invalid_explicit_shell_cannot_create_a_runtime_window(cx: &mut gpui::TestAppContext) {
        let error = cx
            .update(|cx| open_runtime_window(cx, None, Some("pebrel-missing-shell-id".into())))
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
    fn privileged_startup_never_restores_the_ordinary_session() {
        assert!(matches!(initial_startup(None, None, false), WorkspaceStartup::RestoreOrDefault));
        assert!(matches!(
            initial_startup(None, None, true),
            WorkspaceStartup::NewTerminal { cwd: None }
        ));
    }
}
