//! 管理员交接只适配启动请求；窗口选择、终端创建和聚焦复用现有窗口注册表。

use super::*;
use crate::platform::elevation::handover::{Dispatch, Request};
use crate::session::LaunchSession;

pub(super) fn dispatch(request: Arc<Dispatch>, cx: &mut App) {
    request.run(|request| open(request, cx).map(|_| ()).map_err(|error| error.message));
}

fn startup(request: &Request) -> Result<WorkspaceStartup, ApiError> {
    if request.shell_id.is_some() {
        runtime_window_startup(request.cwd.clone(), request.shell_id.as_deref())
    } else {
        Ok(initial_startup(request.cwd.clone(), request.command.clone(), true))
    }
}

fn open(request: &Request, cx: &mut App) -> Result<(u64, u64), ApiError> {
    if cx.global::<WindowRegistry>().quit_pending {
        return Err(ApiError::new("shutting_down", "the elevated window owner is closing"));
    }
    let startup = startup(request)?;
    let behavior = nebula_settings::RuntimeSettings::load().windowing_behavior;
    let target = (behavior != nebula_settings::WindowingBehaviorName::UseNew)
        .then(|| select_mru_window(behavior, cx))
        .flatten();
    let Some(target) = target else {
        let (id, workspace) =
            open_workspace_window(cx, startup, None, None, true, WindowRole::Regular)
                .map_err(|error| ApiError::new("window_create_failed", error.to_string()))?;
        return Ok((id, workspace.read(cx).active_terminal_pane_id().unwrap_or_default()));
    };
    let (launch, cwd) = match startup {
        WorkspaceStartup::LaunchTerminal { launch, cwd } => (launch, cwd),
        WorkspaceStartup::NewTerminal { cwd } => (LaunchSession::Default, cwd),
        _ => return Err(ApiError::new("invalid_startup", "handover cannot restore a session")),
    };
    let workspace = target.workspace.clone();
    let pane = target
        .handle
        .update(cx, move |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.add_terminal_with(launch, cwd, None, window, cx)
            })
        })
        .and_then(|value| value)
        .map_err(|error| ApiError::new("target_not_found", error.to_string()))?;
    // 先按当前桌面/MRU 策略选择，再聚焦实际承载新标签的窗口。
    focus_entry(&target, Some(pane), cx);
    Ok((target.runtime_window_id, pane))
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::super::transfer_tests::{initialize_test, open_test_window};
    use super::*;
    use crate::config::ui_config::Program;
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};

    fn configure(behavior: &str) {
        let path = nebula_settings::settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("windowing_behavior={behavior}\nrestore_session=0\ntray=0\n"))
            .unwrap();
    }

    fn request(directory: &std::path::Path, argument: &str) -> Request {
        Request {
            cwd: Some(directory.to_owned()),
            command: Some(Program::WithArgs {
                program: "pebrel-test-missing-shell-executable".into(),
                args: vec![argument.into()],
            }),
            shell_id: None,
        }
    }

    fn deliver(request: Request, cx: &mut App) -> Result<(), String> {
        let (request, reply) = Dispatch::new(request);
        dispatch_shell_events(vec![GpuiShellEvent::ElevatedLaunch(request)], cx);
        reply.try_recv().unwrap()
    }

    #[gpui::test]
    fn elevated_launch_reuses_window_and_preserves_each_program_and_directory(
        cx: &mut gpui::TestAppContext,
    ) {
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        configure("use_any_existing");
        initialize_test(cx);
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        cx.update(|cx| {
            let (id, workspace) = open_test_window(cx, 0);
            deliver(request(first_dir.path(), "first literal"), cx).unwrap();
            let first = workspace.read(cx).tabs[0].focused_view().unwrap().clone();
            configure("use_existing");
            deliver(request(second_dir.path(), "second $(literal)"), cx).unwrap();
            assert_eq!(cx.global::<WindowRegistry>().entries.len(), 1);
            assert_eq!(workspace.read(cx).runtime_window_id, id);
            assert_eq!(workspace.read(cx).tabs.len(), 2);
            assert_eq!(workspace.read(cx).tabs[0].focused_view().unwrap(), &first);
            for (index, argument, directory) in [
                (0, "first literal", first_dir.path()),
                (1, "second $(literal)", second_dir.path()),
            ] {
                let workspace = workspace.read(cx);
                let metadata = workspace.meta(index);
                let Some(LaunchSession::Shell { program, args, .. }) = &metadata.launch else {
                    panic!("explicit startup identity was replaced by the default shell");
                };
                assert_eq!(program, "pebrel-test-missing-shell-executable");
                assert_eq!(args, &[argument.to_owned()]);
                let view = workspace.tabs[index].focused_view().unwrap().read(cx);
                assert_eq!(view.local_cwd().as_deref(), Some(directory));
            }
        });
    }

    #[gpui::test]
    fn elevated_launch_new_window_policy_and_invalid_requests_preserve_existing_tabs(
        cx: &mut gpui::TestAppContext,
    ) {
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        configure("use_new");
        initialize_test(cx);
        let directory = tempfile::tempdir().unwrap();
        cx.update(|cx| {
            let (_, original) = open_test_window(cx, 1);
            for argument in ["first", "second"] {
                deliver(request(directory.path(), argument), cx).unwrap();
            }
            assert_eq!(cx.global::<WindowRegistry>().entries.len(), 3);
            assert_eq!(original.read(cx).tabs.len(), 1);
            let mut invalid = request(directory.path(), "unchanged");
            invalid.shell_id = Some("pebrel-missing-shell-id".into());
            assert!(deliver(invalid, cx).is_err());
            assert_eq!(cx.global::<WindowRegistry>().entries.len(), 3);
            cx.global_mut::<WindowRegistry>().quit_pending = true;
            assert!(deliver(request(directory.path(), "late"), cx).is_err());
            assert_eq!(cx.global::<WindowRegistry>().entries.len(), 3);
        });
    }
}
