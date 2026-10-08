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
fn cycling_split_mode_shortcut_persists_syncs_the_selector_and_shows_in_app_toasts(
    cx: &mut TestAppContext,
) {
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
    use gpui_component::WindowExt as _;

    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[("split_shell_source", "default".into())]).unwrap();
    let raw = vec![("ctrl+alt+m".into(), "CycleSplitShellSource".into())];
    nebula_settings::persist_keybinds(&raw).unwrap();
    let (_directory, workspace, mut cx) = fixture(cx);
    workspace.update(&mut cx, |w, cx| w.apply_custom_keybinds(cx));

    for (index, source) in
        [SplitShellSource::Focused, SplitShellSource::Ask, SplitShellSource::Default]
            .into_iter()
            .enumerate()
    {
        let previous_toasts = cx.update(|window, cx| {
            window.notifications(cx).iter().map(|note| note.entity_id()).collect::<Vec<_>>()
        });
        press("ctrl-alt-m", &mut cx);
        assert_eq!(nebula_settings::RuntimeSettings::load().split_shell_source, source);
        cx.update(|window, cx| {
            assert_eq!(
                cx.global::<crate::gpui_shell::config::Settings>().split_shell_source,
                source
            );
            assert_eq!(
                window
                    .notifications(cx)
                    .iter()
                    .filter(|note| !previous_toasts.contains(&note.entity_id()))
                    .count(),
                1,
                "switching shows a new in-app toast",
            );
            workspace.update(cx, |w, cx| {
                assert_eq!(w.tabs.len(), 1);
                assert!(w.pending_split.is_none());
                if index == 0 {
                    w.open_settings(window, cx);
                }
                let pane = w.settings_surface.as_ref().unwrap().0.read(cx);
                assert_eq!(pane.runtime.split_shell_source, source);
                assert_eq!(
                    pane.select_of("split_shell_source")
                        .unwrap()
                        .read(cx)
                        .selected_index(cx)
                        .unwrap()
                        .row,
                    (index + 1) % 3,
                );
            });
        });
        cx.run_until_parked();
    }
    assert_eq!(count(&workspace, &cx), 1);
    nebula_settings::persist_keybinds(&[("ctrl+alt+n".into(), "CycleSplitShellSource".into())])
        .unwrap();
    workspace.update(&mut cx, |w, cx| w.apply_custom_keybinds(cx));
    press("ctrl-alt-m", &mut cx);
    assert_eq!(
        nebula_settings::RuntimeSettings::load().split_shell_source,
        SplitShellSource::Default
    );
    press("ctrl-alt-n", &mut cx);
    assert_eq!(
        nebula_settings::RuntimeSettings::load().split_shell_source,
        SplitShellSource::Focused
    );
    nebula_settings::persist_keybinds(&[]).unwrap();
    workspace.update(&mut cx, |w, cx| w.apply_custom_keybinds(cx));
    press("ctrl-alt-n", &mut cx);
    assert_eq!(
        nebula_settings::RuntimeSettings::load().split_shell_source,
        SplitShellSource::Focused
    );
}

#[gpui::test]
fn cycling_split_mode_save_failure_keeps_the_current_mode_and_settings_bytes(
    cx: &mut TestAppContext,
) {
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
    use gpui_component::WindowExt as _;

    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let (_directory, workspace, mut cx) = fixture(cx);
    set_source(SplitShellSource::Ask, &mut cx);
    nebula_settings::persist_keybinds(&[("ctrl+alt+m".into(), "CycleSplitShellSource".into())])
        .unwrap();
    workspace.update(&mut cx, |w, cx| w.apply_custom_keybinds(cx));
    // Invalid UTF-8 makes the existing read-before-write boundary fail without overwriting bytes.
    std::fs::write(nebula_settings::settings_path(), [0xff]).unwrap();
    cx.update(|window, cx| {
        for note in window.notifications(cx).iter() {
            note.update(cx, |note, cx| note.dismiss(window, cx));
        }
    });
    press("ctrl-alt-m", &mut cx);
    cx.update(|window, cx| {
        assert_eq!(
            cx.global::<crate::gpui_shell::config::Settings>().split_shell_source,
            SplitShellSource::Ask
        );
        assert_eq!(window.notifications(cx).len(), 1, "save failure shows an in-app warning");
    });
    assert_eq!(std::fs::read(nebula_settings::settings_path()).unwrap(), [0xff]);
    assert_eq!(count(&workspace, &cx), 1);
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
fn profile_save_and_restore_uses_the_saved_pane_directory(cx: &mut TestAppContext) {
    let (directory, workspace, mut cx) = fixture(cx);
    let startup = directory.path().join("startup");
    let live = directory.path().join("live");
    std::fs::create_dir(&startup).unwrap();
    std::fs::create_dir(&live).unwrap();
    let profile = LaunchSession::Profile {
        name: "Project".into(),
        command: directory.path().join("missing-profile-shell").to_string_lossy().into_owned(),
        args: vec!["--login".into()],
        cwd: Some(startup.to_string_lossy().into_owned()),
        shell_id: None,
    };
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.add_terminal_with(profile.clone(), Some(live.clone()), None, window, cx);
            let pane = w.tabs[w.active].focused_view().unwrap().clone();
            assert_eq!(
                pane.read(cx).cwd,
                startup.to_string_lossy(),
                "fresh profile keeps startup cwd"
            );
            pane.update(cx, |view, _| view.cwd = live.to_string_lossy().into_owned());
            let json = serde_json::to_string(&w.snapshot_session(cx)).unwrap();
            let saved: crate::session::Session = serde_json::from_str(&json).unwrap();
            let tab = saved.tabs.last().unwrap();
            let Some(crate::session::LayoutSession::Pane { cwd, launch, .. }) = &tab.layout else {
                panic!("expected a saved profile pane");
            };
            assert_eq!(cwd, &live.to_string_lossy());
            assert_eq!(launch.as_ref(), Some(&profile));
            assert!(w.restore_tab(tab, false, window, cx));
            let restored = w.tabs.last().unwrap().focused_view().unwrap().read(cx);
            assert_eq!(restored.cwd, live.to_string_lossy());
            assert_eq!(restored.session_launch, profile);

            let mut invalid = tab.clone();
            let Some(crate::session::LayoutSession::Pane { cwd, .. }) = &mut invalid.layout else {
                unreachable!();
            };
            *cwd = directory.path().join("missing-directory").to_string_lossy().into_owned();
            assert!(w.restore_tab(&invalid, false, window, cx));
            assert_eq!(
                w.tabs.last().unwrap().focused_view().unwrap().read(cx).cwd,
                startup.to_string_lossy()
            );
        });
    });
}

#[cfg(unix)]
#[gpui::test]
fn focused_split_keeps_the_original_unix_default_after_preference_changes(cx: &mut TestAppContext) {
    let (directory, workspace, mut cx) = fixture(cx);
    let program = nebula_terminal::tty::default_shell_program().unwrap();
    let id = std::path::Path::new(&program).file_name().unwrap().to_str().unwrap();
    let args = crate::platform::shell::interactive_args(id);
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.add_terminal_with(
                LaunchSession::Default,
                Some(directory.path().to_path_buf()),
                None,
                window,
                cx,
            );
            let original = w.tabs[w.active].focused_view().unwrap().read(cx).session_launch.clone();
            let LaunchSession::Shell { program: saved_program, args: saved_args, .. } = &original
            else {
                panic!("default shell must be frozen at pane creation");
            };
            assert_eq!(saved_program, &program);
            assert_eq!(saved_args, &args);
            let settings = cx.global_mut::<crate::gpui_shell::config::Settings>();
            settings.shell_id = Some(if id == "zsh" { "bash" } else { "zsh" }.into());
            settings.split_shell_source = SplitShellSource::Focused;
            w.request_split(SplitDirection::LeftRight, window, cx);
            assert_eq!(w.tabs[w.active].focused_view().unwrap().read(cx).session_launch, original);
            w.split_focused(SplitDirection::TopBottom, window, cx).unwrap();
            assert_eq!(w.tabs[w.active].focused_view().unwrap().read(cx).session_launch, original);
            let WorkspaceTab::Terminal { panes, .. } = &w.tabs[w.active] else { unreachable!() };
            assert_eq!(panes.len(), 3);
            for (index, pane) in panes.iter().enumerate() {
                let view = pane.view.read(cx);
                if index > 0 {
                    assert_eq!(
                        view.exec_context.as_ref().unwrap().shell_program(),
                        Some(program.as_str())
                    );
                }
                view.shutdown();
            }
        });
    });
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
