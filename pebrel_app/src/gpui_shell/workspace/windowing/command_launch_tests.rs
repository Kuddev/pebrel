use super::*;
use gpui::TestAppContext;
use serde_json::Value;

pub(super) struct LaunchFixture {
    pub(super) program: crate::config::ui_config::Program,
    pub(super) directory: PathBuf,
    arguments: Vec<String>,
    output: PathBuf,
}

impl LaunchFixture {
    pub(super) fn new() -> Self {
        let root = crate::platform::profile_test_isolation::root();
        let directory = root.join("项目 with spaces");
        std::fs::create_dir(&directory).unwrap();
        let arguments = [
            "--exact",
            "--nocapture",
            "--test-threads=1",
            "--",
            "gpui_shell::workspace::windowing::command_launch_tests::native_argv_fixture",
            "pebrel-argv-probe-only",
            "two words",
            "",
            "$(touch unexpected)",
            "--window",
            "--option=two words",
            "quote\"and\\path\\",
            "中文",
            "%PATH%",
        ]
        .map(String::from)
        .to_vec();
        let program = crate::config::ui_config::Program::WithArgs {
            program: std::env::current_exe().unwrap().to_str().unwrap().to_owned(),
            args: arguments.clone(),
        };
        Self { program, directory, arguments, output: root.join("launch-argv.json") }
    }

    pub(super) fn wait_for_record(&self) -> Option<Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(bytes) = std::fs::read(&self.output) {
                return Some(serde_json::from_slice(&bytes).unwrap());
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn verify(&self, record: Option<Value>) {
        let record = record.expect("native child did not record its arguments");
        assert_eq!(record["argv"], json!(self.arguments));
        let actual = PathBuf::from(record["cwd"].as_str().unwrap());
        assert_eq!(actual.canonicalize().unwrap(), self.directory.canonicalize().unwrap());
        assert!(!self.directory.join("unexpected").exists());
    }
}

#[test]
fn native_argv_fixture() {
    let argv: Vec<_> = std::env::args().skip(1).collect();
    if !argv.iter().any(|arg| arg == "pebrel-argv-probe-only") {
        return;
    }
    let root = crate::platform::profile_test_isolation::root();
    let data = json!({ "cwd": std::env::current_dir().unwrap(), "argv": argv });
    let pending = root.join("launch-argv.tmp");
    std::fs::write(&pending, serde_json::to_vec(&data).unwrap()).unwrap();
    std::fs::rename(pending, root.join("launch-argv.json")).unwrap();
}

pub(super) fn stop_reader(
    cx: &mut TestAppContext,
    view: &Entity<crate::gpui_shell::terminal::view::TerminalView>,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while cx.update(|cx| {
        view.read(cx)
            .session
            .as_ref()
            .unwrap()
            .notifier
            .0
            .send(pebrel_terminal::event_loop::Msg::Shutdown)
            .is_ok()
    }) {
        assert!(std::time::Instant::now() < deadline, "PTY reader did not shut down");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn explicit_startup_argv_survives_saved_workspace_and_native_restore(cx: &mut TestAppContext) {
    let filter = "gpui_shell::workspace::windowing::command_launch_tests::explicit_startup_argv_survives_saved_workspace_and_native_restore";
    if crate::platform::profile_test_isolation::run_exact_child(filter, None) {
        return;
    }
    cx.background_executor.allow_parking();
    transfer_tests::initialize_test(cx);
    let fixture = LaunchFixture::new();
    let WorkspaceStartup::LaunchTerminal { launch, .. } =
        initial_startup(Some(fixture.directory.clone()), Some(fixture.program.clone()), false)
    else {
        panic!("explicit program was discarded")
    };
    assert!(matches!(launch, crate::session::LaunchSession::Command { .. }));
    let mut tab =
        crate::session::TabSession::single(fixture.directory.to_str().unwrap().into(), None, None);
    tab.launch = Some(launch);
    tab.layout = Some(crate::session::LayoutSession::Pane {
        custom_name: None,
        launch: None,
        cwd: fixture.directory.to_str().unwrap().into(),
        agent: Some(crate::session::AgentSession {
            source: "codex".into(),
            session_id: Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into()),
            session_file: None,
        }),
    });
    let path = crate::platform::profile_test_isolation::root().join("command-workspace.json");
    crate::session::save_to(&path, &crate::session::Session::new(0, vec![tab])).unwrap();
    let restored = crate::session::load_from(&path).unwrap();
    assert!(matches!(
        restored.tabs[0].layout.as_ref(),
        Some(crate::session::LayoutSession::Pane { agent: Some(_), .. })
    ));
    let view = cx.update(|cx| {
        let (id, workspace) = transfer_tests::open_test_window(cx, 0);
        entry_by_id(id, cx)
            .unwrap()
            .handle
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    assert!(workspace.restore_tab(&restored.tabs[0], true, window, cx));
                    let view = workspace.tabs[0].focused_view().unwrap().clone();
                    assert!(
                        !view.read(cx).recovery_pending(),
                        "command stdin is not a shell resume channel"
                    );
                    assert!(view.read(cx).session_agent().is_none());
                    view
                })
            })
            .unwrap()
    });
    let record = fixture.wait_for_record();
    stop_reader(cx, &view);
    fixture.verify(record);
}
