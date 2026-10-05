use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use gpui::{AppContext as _, Keystroke, TestAppContext, VisualTestContext};
use gpui_component::Root;

fn fixture(
    cx: &mut TestAppContext,
    picker: bool,
) -> (tempfile::TempDir, Entity<NebulaWorkspace>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    // Native appearance initialization reloads settings from disk. Use the real
    // preference authority, with callers holding the bytes guard and fixture lock.
    nebula_settings::persist_keys(&[
        ("split_shell_picker", if picker { "1" } else { "0" }.into()),
        ("focus_follows_mouse", "0".into()),
    ])
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
        let mut settings =
            crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
        settings.split_shell_picker = picker;
        settings.focus_follows_mouse = false;
        settings.shell_id = Some("unrelated-default".into());
        cx.set_global(settings);
    });
    let mut entity = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            let mut workspace = NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                crate::runtime_api::RuntimeHub::new(),
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            );
            workspace.add_terminal_with(
                LaunchSession::Shell {
                    name: "Source shell".into(),
                    program: directory
                        .path()
                        .join("missing-source-shell")
                        .to_string_lossy()
                        .into_owned(),
                    args: vec!["--login".into()],
                },
                Some(directory.path().to_path_buf()),
                None,
                window,
                cx,
            );
            workspace
        });
        entity = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    window.simulate_resize(gpui::size(px(1000.0), px(750.0)));
    window.run_until_parked();
    (directory, entity.unwrap(), window.clone())
}

fn press(combo: &str, cx: &mut VisualTestContext) {
    let keystroke = Keystroke::parse(combo).unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
}

fn count(workspace: &Entity<NebulaWorkspace>, cx: &VisualTestContext) -> usize {
    workspace.read_with(cx, |w, _| match &w.tabs[0] {
        WorkspaceTab::Terminal { panes, .. } => panes.len(),
        _ => panic!("terminal tab expected"),
    })
}

fn install_choice(
    workspace: &Entity<NebulaWorkspace>,
    directory: &std::path::Path,
    cx: &mut VisualTestContext,
) {
    workspace.update(cx, |w, cx| {
        let selected = crate::shell_detect::DetectedShell {
            name: "Chosen shell".into(),
            id: "chosen".into(),
            program: directory.join("missing-chosen-shell").to_string_lossy().into_owned(),
            args: vec!["--chosen".into()],
        };
        w.palette_override = Some(shell_palette_rows(
            vec![selected],
            Vec::new(),
            Vec::<(String, String)>::new(),
            "chosen",
            workspace_ui_language(),
            1.0,
        ));
        cx.notify();
    });
}

#[gpui::test]
fn both_split_shortcuts_inherit_the_focused_pane_not_the_default_or_tab_identity(
    cx: &mut TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    let (directory, workspace, mut cx) = fixture(cx, false);
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            let chosen = LaunchSession::Profile {
                name: "Other profile".into(),
                command: directory
                    .path()
                    .join("missing-profile-shell")
                    .to_string_lossy()
                    .into_owned(),
                args: vec!["--profile".into()],
                cwd: None,
                shell_id: Some("other".into()),
            };
            let source = match &w.tabs[0] {
                WorkspaceTab::Terminal { focused, .. } => *focused,
                _ => unreachable!(),
            };
            w.split_at(
                PendingSplit { pane_id: source, direction: SplitDirection::LeftRight },
                Some(chosen),
                window,
                cx,
            )
            .unwrap();
        })
    });
    for shortcut in ["ctrl-shift-d", "ctrl-shift-s"] {
        let identity = workspace.read_with(&cx, |w, cx| {
            w.tabs[0].focused_view().unwrap().read(cx).session_launch.clone()
        });
        press(shortcut, &mut cx);
        workspace.read_with(&cx, |w, cx| {
            assert!(!w.command_palette_open);
            assert_eq!(w.tabs[0].focused_view().unwrap().read(cx).session_launch, identity);
            assert_ne!(w.meta(0).launch.as_ref(), Some(&identity));
        });
    }
    assert_eq!(count(&workspace, &cx), 4);
}

#[gpui::test]
fn picker_escape_and_click_keep_the_captured_anchor_and_direction(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    let (directory, workspace, mut cx) = fixture(cx, true);
    let original =
        workspace.read_with(&cx, |w, cx| w.tabs[0].focused_view().unwrap().read(cx).pane_id);
    press("ctrl-shift-d", &mut cx);
    assert_eq!(count(&workspace, &cx), 1);
    assert!(workspace.read_with(&cx, |w, _| w.pending_split.is_some()));
    press("escape", &mut cx);
    assert!(workspace.read_with(&cx, |w, _| w.pending_split.is_none() && !w.command_palette_open));
    assert_eq!(count(&workspace, &cx), 1);
    // Immediate/API splitting stays non-interactive even with the preference enabled.
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.split_focused(SplitDirection::LeftRight, window, cx).unwrap();
            w.focus_pane(0, original, window, cx);
        })
    });
    press("ctrl-shift-s", &mut cx);
    install_choice(&workspace, directory.path(), &mut cx);
    workspace.update(&mut cx, |w, cx| {
        let WorkspaceTab::Terminal { panes, focused, .. } = &mut w.tabs[0] else { unreachable!() };
        *focused = panes[1].id; // A focus change must not retarget the pending request.
        cx.notify();
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx.debug_bounds("command-palette-row-0").expect("real launcher row");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.read_with(&cx, |w, cx| {
        assert_eq!(w.tabs.len(), 1, "a split choice must not create a tab");
        let WorkspaceTab::Terminal { tree, focused, .. } = &w.tabs[0] else { unreachable!() };
        let SplitTree::Split { first, .. } = tree else { panic!("outer split lost") };
        assert_eq!(first.leaves(), vec![original, *focused]);
        let SplitTree::Split { direction, .. } = first.as_ref() else { panic!("wrong anchor") };
        assert_eq!(*direction, SplitDirection::TopBottom);
        let LaunchSession::Shell { name, args, .. } =
            &w.tabs[0].focused_view().unwrap().read(cx).session_launch
        else {
            panic!("chosen identity lost")
        };
        assert_eq!(name, "Chosen shell");
        assert_eq!(args, &["--chosen"]);
        assert!(w.pending_split.is_none() && !w.command_palette_open);
    });
    assert_eq!(count(&workspace, &cx), 3);
}

#[gpui::test]
fn closed_source_choice_cannot_split_the_new_focus_or_open_a_tab(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    let (directory, workspace, mut cx) = fixture(cx, true);
    let source =
        workspace.read_with(&cx, |w, cx| w.tabs[0].focused_view().unwrap().read(cx).pane_id);
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.add_terminal_with(
                LaunchSession::Shell {
                    name: "Surviving shell".into(),
                    program: directory
                        .path()
                        .join("missing-survivor-shell")
                        .to_string_lossy()
                        .into_owned(),
                    args: Vec::new(),
                },
                Some(directory.path().to_path_buf()),
                None,
                window,
                cx,
            );
            w.activate_tab(w.tab_of_pane(source).unwrap(), window, cx);
        })
    });
    press("ctrl-shift-d", &mut cx);
    install_choice(&workspace, directory.path(), &mut cx);
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            assert_eq!(w.pending_split.unwrap().pane_id, source);
            w.close_pane(w.tab_of_pane(source).unwrap(), source, window, cx);
            w.run_selected_palette_action(window, cx);
            assert_eq!(w.tabs.len(), 1, "closed source must not open an unrelated tab");
            let WorkspaceTab::Terminal { panes, .. } = &w.tabs[0] else { unreachable!() };
            assert_eq!(panes.len(), 1, "closed source must not split the new focus");
            assert!(w.pending_split.is_none());
        })
    });
}
