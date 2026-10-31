//! Exercise parsed tab launches through the real window dispatcher and PTY owner.
use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use crate::runtime_api::RuntimeTaskState;
use gpui::TestAppContext;
use serde_json::Value;
use transfer_tests::{initialize_test, open_test_window};

fn configure(behavior: &str) {
    let path = nebula_settings::settings_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("windowing_behavior={behavior}\nrestore_session=0\ntray=0\nfetch=0\n"),
    )
    .unwrap();
}

fn deliver(params: Value, cx: &mut App) -> Result<Value, ApiError> {
    let command = RuntimeCommand::test_request("tab.new", params)?;
    let (dispatch, reply) = RuntimeDispatch::test(command);
    dispatch_runtime(dispatch, cx);
    reply.try_recv().unwrap()
}

fn assert_launch(
    workspace: &Entity<NebulaWorkspace>,
    directory: &std::path::Path,
    argument: &str,
    cx: &App,
) {
    let workspace = workspace.read(cx);
    assert_eq!(
        workspace.tab_meta[workspace.active].launch,
        Some(crate::session::LaunchSession::Shell {
            name: "pebrel-test-missing-shell-executable".into(),
            program: "pebrel-test-missing-shell-executable".into(),
            args: vec![argument.into()],
        })
    );
    let view = workspace.tabs[workspace.active].focused_view().unwrap().read(cx);
    assert_eq!(view.local_cwd().as_deref(), Some(directory));
    assert_eq!(view.runtime_task_state(), RuntimeTaskState::Failed);
}

#[gpui::test]
fn runtime_tab_launch_selects_mru_or_explicit_window_and_retains_failed_programs(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    configure("use_any_existing");
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        let (first, first_workspace) = open_test_window(cx, 0);
        let (second, second_workspace) = open_test_window(cx, 0);
        let entry = entry_by_id(first, cx).unwrap();
        entry.handle.update(cx, |_, window, cx| mark_active(first, window, cx)).unwrap();
        let params = json!({"cwd": directory.path(), "argv": ["pebrel-test-missing-shell-executable", "two words"]});
        let result = deliver(params.clone(), cx).unwrap();
        assert_eq!(result["action"]["window_id"], first);
        assert_launch(&first_workspace, directory.path(), "two words", cx);
        assert!(second_workspace.read(cx).tabs.is_empty());
        let mut explicit = params.clone();
        explicit["window_id"] = json!(second);
        // Explicit targeting must also override the create-new-window preference.
        configure("use_new");
        let result = deliver(explicit.clone(), cx).unwrap();
        assert_eq!(result["action"]["window_id"], second);
        assert_launch(&second_workspace, directory.path(), "two words", cx);
        explicit["window_id"] = json!(u64::MAX);
        assert_eq!(deliver(explicit, cx).unwrap_err().code, "target_not_found");
        assert_eq!(first_workspace.read(cx).tabs.len(), 1);
        assert_eq!(second_workspace.read(cx).tabs.len(), 1);
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 2);
    });
}

#[gpui::test]
fn runtime_tab_launch_preserves_command_when_a_window_must_be_created(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    configure("use_any_existing");
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        for behavior in ["use_any_existing", "use_new"] {
            configure(behavior);
            let result = deliver(json!({"cwd": directory.path(), "argv": ["pebrel-test-missing-shell-executable", behavior]}), cx).unwrap();
            let id = result["action"]["window_id"].as_u64().unwrap();
            let workspace = entry_by_id(id, cx).unwrap().workspace.upgrade().unwrap();
            assert_launch(&workspace, directory.path(), behavior, cx);
        }
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 2);
    });
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn runtime_tab_launch_executes_literal_arguments_in_the_requested_directory(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    configure("use_any_existing");
    // This fixture deliberately receives events from a real PTY reader thread.
    cx.background_executor.allow_parking();
    initialize_test(cx);
    let directory = tempfile::tempdir().unwrap();
    let view = cx.update(|cx| {
        let (id, workspace) = open_test_window(cx, 0);
        let result = deliver(
            json!({"window_id": id, "cwd": directory.path(), "argv": [
                "/bin/sh", "-c", "printf '%s\\n' \"$PWD\" \"$@\" > launch.txt; read -r line",
                "probe", "two words", "", "$(touch unexpected)", "--window",
            ]}),
            cx,
        )
        .unwrap();
        assert_eq!(result["action"]["window_id"], id);
        assert_eq!(workspace.read(cx).tabs.len(), 1);
        assert_eq!(cx.global::<WindowRegistry>().entries.len(), 1);
        workspace.read(cx).tabs[0].focused_view().unwrap().clone()
    });
    // The child process runs independently of the virtual GPUI executor.
    let expected =
        format!("{}\ntwo words\n\n$(touch unexpected)\n--window\n", directory.path().display());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let output = loop {
        let output = std::fs::read_to_string(directory.path().join("launch.txt")).ok();
        if output.as_deref() == Some(&expected) || std::time::Instant::now() >= deadline {
            break output;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Stop and wait for the reader before the test scheduler is destroyed, including
    // when the output assertion below fails. Channel disconnection means the PTY
    // event loop has been dropped; drain its queued UI work on this owning thread.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while cx.update(|cx| {
        view.read(cx)
            .session
            .as_ref()
            .unwrap()
            .notifier
            .0
            .send(nebula_terminal::event_loop::Msg::Shutdown)
            .is_ok()
    }) {
        assert!(std::time::Instant::now() < deadline, "PTY reader did not shut down");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.run_until_parked();
    assert_eq!(output.as_deref(), Some(expected.as_str()));
    assert!(!directory.path().join("unexpected").exists());
}
