use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point};
use nebula_settings::TabsPositionName;

fn open_workspace(
    mode: TabsPositionName,
    cx: &mut TestAppContext,
) -> (Entity<NebulaWorkspace>, VisualTestContext) {
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        super::super::init(cx);
        super::super::windowing::initialize(cx, hub.clone());
        let mut settings =
            crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
        settings.ui_language = crate::display::UiLanguage::EnUs;
        settings.shell_id = Some(if cfg!(windows) { "cmd" } else { "sh" }.to_owned());
        cx.set_global(settings);
        crate::gpui_shell::theme::apply_chrome_theme(cx);
    });
    let mut workspace = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut workspace = NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                hub,
                super::super::windowing::WorkspaceStartup::Empty,
                super::super::windowing::WindowRole::Regular,
                cx,
            );
            workspace.tabs_position = mode;
            workspace
        });
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.run_until_parked();
    let workspace = workspace.unwrap();
    window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.tabs_position = mode;
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    assert_eq!(workspace.read_with(window, |view, _| view.tabs_position), mode);
    (workspace, window.clone())
}

fn click_project(cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds("open-project-folder").expect("project toolbar control");
    assert_eq!(bounds.size.width, px(32.0));
    assert_eq!(bounds.size.height, px(32.0));
    // Hit the padding, not just the folder glyph.
    cx.simulate_click(bounds.origin + point(px(3.0), px(3.0)), Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn both_toolbar_layouts_open_one_folder_picker_and_cancel_without_a_tab(cx: &mut TestAppContext) {
    for mode in [TabsPositionName::Sidebar, TabsPositionName::Top] {
        let (workspace, mut window) = open_workspace(mode, cx);
        click_project(&mut window);
        assert!(window.did_prompt_for_paths());
        assert!(workspace.read_with(&window, |view, _| view.project_picker.is_some()));
        click_project(&mut window);
        window.simulate_path_prompt_response(|options| {
            assert!(options.directories);
            assert!(!options.files && !options.multiple);
            assert_eq!(options.prompt.as_deref(), Some("Open project folder…"));
            None
        });
        window.run_until_parked();
        assert!(!window.did_prompt_for_paths(), "repeat click must not queue another picker");
        workspace.read_with(&window, |view, _| {
            assert!(view.tabs.is_empty());
            assert!(view.project_picker.is_none());
        });
        click_project(&mut window);
        assert!(window.did_prompt_for_paths(), "cancel must release the pending request");
        window.simulate_path_prompt_response(|_| None);
        window.run_until_parked();
    }
}

#[gpui::test]
fn folder_button_is_reachable_with_tab_and_enter(cx: &mut TestAppContext) {
    let (workspace, mut window) = open_workspace(TabsPositionName::Top, cx);
    // Bootstrap focus in the synthetic empty window, then exercise Root's real
    // reverse/forward Tab routing and this button's local Enter handler.
    window.update(|window, cx| {
        let focus = workspace.read(cx).project_focus.clone();
        focus.focus(window, cx);
    });
    window.run_until_parked();
    window.update(|window, cx| assert!(workspace.read(cx).project_focus.is_focused(window)));
    for key in ["shift-tab", "tab", "enter"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
    window.run_until_parked();
    assert!(window.did_prompt_for_paths());
    window.simulate_path_prompt_response(|_| None);
    window.run_until_parked();
    assert!(workspace.read_with(&window, |view, _| view.tabs.is_empty()));
}

#[gpui::test]
fn invalid_selection_warns_without_opening_a_terminal_and_allows_retry(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("not-a-folder.txt");
    std::fs::write(&file, "fixture").unwrap();
    let (workspace, mut window) = open_workspace(TabsPositionName::Sidebar, cx);
    for path in [file, directory.path().join("removed")] {
        click_project(&mut window);
        window.simulate_path_prompt_response(|_| Some(vec![path]));
        window.run_until_parked();
        workspace.read_with(&window, |view, _| {
            assert!(view.tabs.is_empty());
            assert!(view.project_picker.is_none());
        });
        window.update(|window, cx| {
            let root = window.root::<Root>().flatten().unwrap();
            assert!(!root.read(cx).notification.read(cx).notifications().is_empty());
        });
    }
}

#[gpui::test]
fn shell_start_failure_preserves_the_selected_directory_and_the_old_tab(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project 中文 & $ with spaces");
    std::fs::create_dir(&project).unwrap();
    let (workspace, mut window) = open_workspace(TabsPositionName::Sidebar, cx);
    // Real PTY reader threads are intentionally exercised by native_tests, not
    // by GPUI's deterministic single-threaded test scheduler.
    let launch = LaunchSession::Shell {
        name: "Unavailable fixture shell".into(),
        program: directory.path().join("not-an-executable").to_string_lossy().into_owned(),
        args: Vec::new(),
    };
    let original = window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.add_terminal_with(
                launch.clone(),
                Some(directory.path().to_owned()),
                None,
                window,
                cx,
            );
            view.tabs[0].focused_view().unwrap().clone()
        })
    });
    window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.open_project_selection(
                launch,
                std::future::ready(Ok(Some(project.clone()))),
                window,
                cx,
            );
        });
    });
    window.run_until_parked();
    workspace.read_with(&window, |view, cx| {
        assert_eq!(view.tabs.len(), 2);
        assert!(view.project_picker.is_none());
        let selected = view.tabs[view.active].focused_view().unwrap();
        assert_ne!(selected.entity_id(), original.entity_id());
        assert_eq!(selected.read(cx).cwd, project.to_string_lossy());
        assert_eq!(original.read(cx).cwd, directory.path().to_string_lossy());
        assert!(
            selected.read(cx).session.is_none(),
            "the missing shell must fail without a reader thread"
        );
    });
    workspace.read_with(&window, |view, cx| {
        for tab in &view.tabs {
            if let WorkspaceTab::Terminal { panes, .. } = tab {
                for pane in panes {
                    pane.view.read(cx).shutdown();
                }
            }
        }
    });
}

#[test]
fn explicit_project_directory_overrides_only_the_new_profile_launch() {
    let original = LaunchSession::Profile {
        name: "Custom shell".into(),
        command: "custom-shell".into(),
        args: vec!["--login".into()],
        cwd: Some("old-project".into()),
        shell_id: Some("custom".into()),
    };
    let path = PathBuf::from("chosen-project");
    let chosen = project_launch_at(original.clone(), &path);
    let LaunchSession::Profile { name, command, args, cwd, shell_id } = chosen else {
        panic!("profile identity must be preserved");
    };
    assert_eq!(cwd.as_deref(), Some("chosen-project"));
    assert_eq!((name.as_str(), command.as_str()), ("Custom shell", "custom-shell"));
    assert_eq!(args, ["--login"]);
    assert_eq!(shell_id.as_deref(), Some("custom"));
    assert!(
        matches!(original, LaunchSession::Profile { cwd: Some(cwd), .. } if cwd == "old-project")
    );
}

#[gpui::test]
fn closing_the_workspace_cancels_a_pending_picker_result(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, mut window) = open_workspace(TabsPositionName::Sidebar, cx);
    let (send, receive) = futures::channel::oneshot::channel();
    window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.open_project_selection(
                LaunchSession::Default,
                async move { receive.await.map_err(std::io::Error::other)? },
                window,
                cx,
            );
        })
    });
    let weak = workspace.downgrade();
    drop(workspace);
    window.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert!(weak.upgrade().is_none(), "the picker must not keep a closed workspace alive");
    assert!(
        send.send(Ok(Some(directory.path().to_owned()))).is_err(),
        "a late result must be cancelled"
    );
}

#[gpui::test]
fn picker_failure_releases_the_request_and_shows_an_actionable_warning(cx: &mut TestAppContext) {
    let (workspace, mut window) = open_workspace(TabsPositionName::Sidebar, cx);
    window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.open_project_selection(
                LaunchSession::Default,
                std::future::ready(Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "fixture picker failure",
                ))),
                window,
                cx,
            );
        })
    });
    window.run_until_parked();
    workspace.read_with(&window, |view, _| {
        assert!(view.tabs.is_empty());
        assert!(view.project_picker.is_none());
    });
    window.update(|window, cx| {
        let root = window.root::<Root>().flatten().unwrap();
        assert_eq!(root.read(cx).notification.read(cx).notifications().len(), 1);
    });
}

#[test]
fn validation_preserves_paths_and_rejects_files_or_missing_directories() {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("中文 project & $ with spaces");
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(validate_project_directory(folder.clone()).unwrap(), folder);
    let file = directory.path().join("README.md");
    std::fs::write(&file, "fixture").unwrap();
    assert!(validate_project_directory(file).is_err());
    assert!(validate_project_directory(directory.path().join("removed")).is_err());
}
