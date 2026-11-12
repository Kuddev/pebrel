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
    if let Some(path) = &request.update_restore {
        if !crate::platform::elevation::is_elevated().is_ok_and(|value| value) {
            return Err(ApiError::new(
                "invalid_update_ticket",
                "Administrator recovery requires an elevated owner",
            ));
        }
        let sessions =
            crate::update_download::handoff::restore_ticket_from(path).ok_or_else(|| {
                ApiError::new("invalid_update_ticket", "Administrator update ticket is unavailable")
            })?;
        return open_update_windows(sessions, None, None, cx);
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
            update_restore: None,
        }
    }

    fn deliver(request: Request, cx: &mut App) -> Result<(), String> {
        let (request, reply) = Dispatch::new(request);
        dispatch_shell_events(vec![GpuiShellEvent::ElevatedLaunch(request)], cx);
        reply.try_recv().unwrap()
    }

    #[gpui::test]
    fn update_recovery_tracks_its_windows_and_retries_only_missing_slots(
        cx: &mut gpui::TestAppContext,
    ) {
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        configure("use_any_existing");
        initialize_test(cx);
        cx.update(|cx| {
            let (existing, _) = open_test_window(cx, 0);
            let (restored, _) = open_test_window(cx, 0);
            cx.global_mut::<WindowRegistry>().update_restore_windows = Some(vec![Some(restored)]);
            assert_eq!(ready_update_windows(cx), Some(1));
            open_test_window(cx, 0);
            assert_eq!(
                ready_update_windows(cx),
                Some(1),
                "new unrelated windows are not recovery targets"
            );
            entry_by_id(existing, cx)
                .unwrap()
                .handle
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            prune_entries(cx);
            assert_eq!(
                ready_update_windows(cx),
                Some(1),
                "closing an old window cannot block recovery"
            );

            cx.global_mut::<WindowRegistry>().update_restore_windows =
                Some(vec![Some(restored), None]);
            assert_eq!(
                ready_update_windows(cx),
                None,
                "unrelated windows cannot replace a missing target"
            );
            let mut tab = crate::session::TabSession::single(String::new(), None, None);
            tab.launch = Some(LaunchSession::Shell {
                name: "Recovery fixture".into(),
                program: "pebrel-test-missing-shell-executable".into(),
                args: Vec::new(),
            });
            let sessions = vec![
                crate::session::Session::new(0, Vec::new()),
                crate::session::Session::new(0, vec![tab]),
            ];
            let before = cx.global::<WindowRegistry>().entries.len();
            open_update_windows(sessions.clone(), None, None, cx).unwrap();
            assert_eq!(cx.global::<WindowRegistry>().entries.len(), before + 1);
            open_update_windows(sessions, None, None, cx).unwrap();
            assert_eq!(
                cx.global::<WindowRegistry>().entries.len(),
                before + 1,
                "retrying a partially created workspace must not duplicate existing windows"
            );
            let recovered =
                cx.global::<WindowRegistry>().update_restore_windows.as_ref().unwrap()[1].unwrap();
            assert_eq!(ready_update_windows(cx), None, "a failed PTY is not a successful recovery");
            entry_by_id(recovered, cx)
                .unwrap()
                .handle
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            prune_entries(cx);
            assert_eq!(
                ready_update_windows(cx),
                Some(2),
                "explicitly dismissed recovery windows do not reappear later"
            );
        });
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
