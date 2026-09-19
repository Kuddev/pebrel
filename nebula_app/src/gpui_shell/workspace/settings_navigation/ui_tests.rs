use super::super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn open_workspace(
    count: usize,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<NebulaWorkspace>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let mut workspace_out = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                hub,
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            workspace.tabs_position = nebula_settings::TabsPositionName::Top;
            for index in 0..count {
                let path = directory.path().join(format!("tab-{index}.txt"));
                std::fs::write(&path, "fixture\n").unwrap();
                let view =
                    cx.new(|cx| crate::gpui_shell::code_tab::CodeTabView::new(path, window, cx));
                let subscription = cx.subscribe(&view, |_, _, _, _| {});
                workspace.insert_tab_at(
                    workspace.tabs.len(),
                    WorkspaceTab::Code { view, _subscription: subscription },
                    TabMeta::default(),
                );
            }
            workspace.open_settings(window, cx);
        });
        workspace_out = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    window.run_until_parked();
    window.simulate_resize(gpui::size(px(1280.0), px(900.0)));
    workspace_out.as_ref().unwrap().update(window, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Top;
        workspace.sync_settings_layout();
        cx.notify();
    });
    window.run_until_parked();
    (directory, workspace_out.unwrap(), window.clone())
}

fn click_tab(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = tab_bounds(selector, cx);
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn tab_bounds(selector: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.debug_bounds(selector).expect("visible tab bounds")
}

#[gpui::test]
fn switching_top_tabs_keeps_settings_reachable_until_explicit_close(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    let settings_id = workspace.read_with(&cx, |workspace, _| {
        assert_eq!(workspace.tabs_position, nebula_settings::TabsPositionName::Top);
        assert_eq!(workspace.top_tab_count(), 3);
        workspace.settings_surface.as_ref().unwrap().0.entity_id()
    });
    for (index, selector) in [(0, "top-tab-0"), (1, "top-tab-1"), (0, "top-tab-0")] {
        click_tab(selector, &mut cx);
        workspace.read_with(&cx, |workspace, _| {
            assert!(!workspace.settings_open);
            assert_eq!(workspace.active, index);
            assert_eq!(workspace.top_tab_count(), 3, "switching must retain the settings tab");
        });
        click_tab("top-tab-2", &mut cx);
        workspace.read_with(&cx, |workspace, _| {
            assert!(workspace.settings_open);
            assert_eq!(workspace.top_tab_count(), 3);
            assert_eq!(workspace.settings_surface.as_ref().unwrap().0.entity_id(), settings_id);
        });
    }
    click_tab("top-tab-0", &mut cx);
    let bounds = tab_bounds("top-tab-2", &mut cx);
    cx.simulate_mouse_down(bounds.center(), gpui::MouseButton::Middle, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), gpui::MouseButton::Middle, Modifiers::default());
    cx.run_until_parked();
    workspace.read_with(&cx, |workspace, _| {
        assert!(!workspace.settings_open);
        assert_eq!(workspace.active, 0);
        assert_eq!(workspace.top_tab_count(), 2);
    });
}

#[gpui::test]
fn keyboard_navigation_can_return_to_background_settings(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    for shortcut in ["ctrl-1", "alt-1"] {
        cx.simulate_keystrokes(shortcut);
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-3");
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-tab");
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
    }
    cx.simulate_keystrokes("ctrl-shift-w");
    assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.top_tab_count()), 2);
    cx.simulate_keystrokes("ctrl-3");
    assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
}

#[gpui::test]
fn settings_survives_overflow_and_layout_changes(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(12, cx);
    cx.simulate_resize(gpui::size(px(760.0), px(900.0)));
    cx.simulate_keystrokes("ctrl-1");
    for position in
        [nebula_settings::TabsPositionName::Sidebar, nebula_settings::TabsPositionName::Top]
    {
        workspace.update(&mut cx, |workspace, cx| {
            workspace.tabs_position = position;
            workspace.sync_settings_layout();
            cx.notify();
        });
        assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.top_tab_count()), 13);
    }
    cx.simulate_keystrokes("ctrl-shift-tab");
    let settings = tab_bounds("top-tab-12", &mut cx);
    assert!(settings.origin.x >= px(0.0) && settings.right() <= px(760.0));
    workspace.read_with(&cx, |workspace, _| assert!(workspace.settings_open));
}

#[gpui::test]
fn closing_last_regular_tab_keeps_settings_until_it_is_closed(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(1, cx);
    click_tab("top-tab-0", &mut cx);
    cx.simulate_keystrokes("ctrl-shift-w");
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.tabs.is_empty());
        assert!(workspace.settings_open);
        assert_eq!(workspace.top_tab_count(), 1);
    });
    cx.simulate_keystrokes("ctrl-shift-w");
    assert!(cx.read(|cx| cx.windows().is_empty()));
}
