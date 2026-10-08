use super::*;
use gpui::{AppContext as _, Keystroke, TestAppContext, VisualTestContext};
use gpui_component::Root;

fn fixture(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<NebulaWorkspace>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load_with_runtime(
            nebula_settings::ThemeName::Nord,
            nebula_settings::RuntimeSettings::from_raw(&nebula_settings::RawSettings::from_text(
                "",
            )),
        ));
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

fn set_source(source: SplitShellSource, cx: &mut VisualTestContext) {
    // Exercise the live setting snapshot consumed by shortcuts. Persistence and
    // capsule interactions are covered by the shared settings tests, not here.
    cx.update(|_, cx| {
        let settings = cx.global_mut::<crate::gpui_shell::config::Settings>();
        settings.split_shell_source = source;
        settings.focus_follows_mouse = false;
    });
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

#[gpui::test]
fn changing_the_live_setting_changes_both_split_shortcuts(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = fixture(cx);
    let identity = workspace
        .read_with(&cx, |w, cx| w.tabs[0].focused_view().unwrap().read(cx).session_launch.clone());
    for (index, shortcut) in ["ctrl-shift-d", "ctrl-shift-s"].into_iter().enumerate() {
        set_source(SplitShellSource::Ask, &mut cx);
        press(shortcut, &mut cx);
        assert_eq!(count(&workspace, &cx), index + 1);
        assert!(
            workspace.read_with(&cx, |w, _| w.command_palette_open && w.pending_split.is_some())
        );
        press("escape", &mut cx);
        set_source(SplitShellSource::Focused, &mut cx);
        press(shortcut, &mut cx);
        assert_eq!(count(&workspace, &cx), index + 2);
        workspace.read_with(&cx, |w, cx| {
            assert!(!w.command_palette_open && w.pending_split.is_none());
            assert_eq!(w.tabs[0].focused_view().unwrap().read(cx).session_launch, identity);
        });
    }
}

#[gpui::test]
fn cancelling_ask_leaves_no_pane_or_pending_request(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = fixture(cx);
    set_source(SplitShellSource::Ask, &mut cx);
    for shortcut in ["ctrl-shift-d", "ctrl-shift-s"] {
        press(shortcut, &mut cx);
        assert!(workspace.read_with(&cx, |w, _| w.pending_split.is_some()));
        assert_eq!(count(&workspace, &cx), 1);
        press("escape", &mut cx);
        assert!(
            workspace.read_with(&cx, |w, _| w.pending_split.is_none() && !w.command_palette_open)
        );
        assert_eq!(count(&workspace, &cx), 1);
    }
}
