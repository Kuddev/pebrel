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
        workspace.sync_settings_layout(false);
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

fn numbered_tab_shortcuts() -> [&'static str; 2] {
    if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        ["cmd-1", "cmd-3"]
    } else {
        ["ctrl-1", "ctrl-3"]
    }
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
    let [first_tab, third_tab] = numbered_tab_shortcuts();
    for shortcut in [first_tab, "alt-1"] {
        cx.simulate_keystrokes(shortcut);
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes(third_tab);
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-tab");
        assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert!(workspace.read_with(&cx, |workspace, _| workspace.settings_open));
    }
    cx.simulate_keystrokes("ctrl-shift-w");
    assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.top_tab_count()), 2);
    cx.simulate_keystrokes(third_tab);
    assert!(!workspace.read_with(&cx, |workspace, _| workspace.settings_open));
}

#[gpui::test]
fn settings_survives_overflow_and_layout_changes(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(12, cx);
    cx.simulate_resize(gpui::size(px(760.0), px(900.0)));
    cx.simulate_keystrokes(numbered_tab_shortcuts()[0]);
    for position in
        [nebula_settings::TabsPositionName::Sidebar, nebula_settings::TabsPositionName::Top]
    {
        workspace.update(&mut cx, |workspace, cx| {
            workspace.tabs_position = position;
            workspace.sync_settings_layout(false);
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
fn instant_slide_animation_keeps_the_side_panel_static(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(1, cx);
    // With the slide animation off, folding the main sidebar for the settings
    // page must not arm the fold animation: the column snaps into place.
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
        workspace.settings_open = true;
        workspace.sidebar_collapsed = false;
        workspace.sync_settings_layout(true);
    });
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_collapsed, "sidebar mode folds for settings");
        assert!(!workspace.sidebar_fold_armed, "instant: fold animation not armed");
    });
    // The default slide mode keeps arming the fold as before.
    workspace.update(&mut cx, |workspace, _| {
        workspace.sidebar_collapsed = false;
        workspace.sync_settings_layout(false);
    });
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_fold_armed, "slide: fold animation armed");
    });
    workspace.update(&mut cx, |workspace, _| workspace.sync_settings_layout(true));
    workspace.read_with(&cx, |workspace, _| {
        assert!(workspace.sidebar_collapsed);
        assert!(!workspace.sidebar_fold_armed, "mode switch must clear an unchanged fold");
    });
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

#[gpui::test]
fn density_changes_real_tab_bounds_and_sidebar_drag_pitch(cx: &mut TestAppContext) {
    use nebula_settings::{DensityName, TabsPositionName};

    let (_directory, workspace, mut cx) = open_workspace(8, cx);
    // Settings starts at the end of the overflowing strip; tab 0 is offscreen.
    cx.simulate_keystrokes(numbered_tab_shortcuts()[0]);
    for position in [TabsPositionName::Sidebar, TabsPositionName::Top] {
        for (density, height, gap, title_height) in [
            (DensityName::Standard, 34.0, 8.0, 40.0),
            (DensityName::Compact, 32.0, 4.0, 40.0),
            (DensityName::Standard, 34.0, 8.0, 40.0),
        ] {
            workspace.update(&mut cx, |workspace, cx| {
                workspace.tabs_position = position;
                workspace.density = density;
                workspace.sync_settings_layout(tab_reveal_instant(cx));
                workspace.reveal_active_tab();
                cx.notify();
            });
            cx.run_until_parked();
            let sidebar = position == TabsPositionName::Sidebar;
            let first = tab_bounds(if sidebar { "sidebar-tab-0" } else { "top-tab-0" }, &mut cx);
            let second = tab_bounds(if sidebar { "sidebar-tab-1" } else { "top-tab-1" }, &mut cx);
            assert_eq!(f32::from(first.size.height), height);
            assert_eq!(f32::from(second.size.height), height);
            if sidebar {
                assert_eq!(f32::from(second.top() - first.top()), height + gap);
            }
            let native = cx.update(|window, _| crate::platform::window_chrome::layout(window));
            let title = tab_bounds("workspace-titlebar", &mut cx);
            assert_eq!(
                f32::from(title.size.height),
                native.map_or(title_height, |layout| layout.0)
            );

            // Padding belongs to the tab's actual click target in either density.
            let point = gpui::point(second.left() + px(8.0), second.bottom() - px(2.0));
            cx.simulate_click(point, Modifiers::default());
            workspace.read_with(&cx, |workspace, _| assert_eq!(workspace.active, 1));
            if sidebar {
                cx.simulate_mouse_down(point, gpui::MouseButton::Left, Modifiers::default());
                workspace.read_with(&cx, |workspace, _| {
                    assert_eq!(workspace.tab_drag.as_ref().unwrap().pitch, height + gap);
                });
                cx.simulate_mouse_up(point, gpui::MouseButton::Left, Modifiers::default());
            }
        }
    }
}

#[gpui::test]
fn top_tab_styles_size_titles_individually_and_preserve_hover_close(cx: &mut TestAppContext) {
    use nebula_settings::TabStyle;
    let (_directory, workspace, mut cx) = open_workspace(3, cx);
    cx.simulate_keystrokes(numbered_tab_shortcuts()[0]);
    workspace.update(&mut cx, |workspace, cx| {
        for (meta, name) in workspace.tab_meta.iter_mut().zip([
            "sh",
            "项目控制台",
            "a very long terminal title that exceeds the maximum tab width",
        ]) {
            meta.custom_name = Some(name.into());
        }
        cx.notify();
    });
    for style in [TabStyle::Chrome, TabStyle::Minimal, TabStyle::Tag] {
        cx.update(|_, cx| cx.global_mut::<crate::gpui_shell::config::Settings>().tab_style = style);
        let first = tab_bounds("top-tab-0", &mut cx);
        let second = tab_bounds("top-tab-1", &mut cx);
        let long = tab_bounds("top-tab-2", &mut cx);
        assert!(
            first.size.width < second.size.width && second.size.width < long.size.width,
            "{style:?}: {first:?} {second:?} {long:?}"
        );
        assert_eq!(long.size.width, px(220.0));
        if style == TabStyle::Chrome {
            assert_eq!(first.right() - second.left(), px(10.0), "Chrome feet overlap");
            let label = cx.debug_bounds("top-tab-label-0").unwrap();
            assert!(
                (label.left() - first.left() - px(20.0 + TAB_LABEL_ICON_W + 8.0)).abs() <= px(1.0),
                "selected title and leading icon clear the flared body"
            );
        } else {
            assert!(first.right() < second.left());
        }
        cx.simulate_mouse_move(long.center(), None, Modifiers::default());
        tab_bounds("top-tab-2", &mut cx);
        let close = cx.debug_bounds("top-close-tab-2").expect("hover close button");
        assert!(close.left() >= long.left() && close.right() <= long.right());
        if style == TabStyle::Chrome {
            assert_eq!(long.right() - close.right(), px(20.0), "close clears the flared body");
        }
    }
    let close = tab_bounds("top-close-tab-2", &mut cx);
    cx.simulate_click(close.center(), Modifiers::default());
    assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.tabs.len()), 2);
}

#[gpui::test]
fn long_top_titles_keep_static_fading_when_selected_or_hovered(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tab_meta[0].custom_name = Some("sh".into());
        workspace.tab_meta[1].custom_name = Some(
            "a very long terminal title that exceeds the maximum tab width and must scroll".into(),
        );
        cx.notify();
    });
    cx.update(|window, _| window.activate_window());
    click_tab("top-tab-0", &mut cx);
    let long = tab_bounds("top-tab-1", &mut cx);
    let fade = cx.debug_bounds("top-tab-title-fade-1").expect("inactive titles fade");
    assert!(cx.debug_bounds("top-tab-title-moving-1").is_none());

    let hover = gpui::point(long.left() + px(14.0), long.center().y);
    cx.simulate_mouse_move(hover, None, Modifiers::default());
    tab_bounds("top-tab-1", &mut cx);
    let label = cx.debug_bounds("top-tab-label-1").unwrap();
    let text = cx.debug_bounds("top-tab-title-text-1").expect("full title retains its origin");
    assert_eq!(cx.debug_bounds("top-tab-title-fade-1"), Some(fade));
    assert!(cx.debug_bounds("top-tab-title-moving-1").is_none());
    let close = cx.debug_bounds("top-close-tab-1").unwrap();
    assert!(text.size.width > label.size.width);
    assert!(label.right() <= close.left(), "fading text must end before the close slot");
    assert_eq!(tab_bounds("top-tab-1", &mut cx), long);

    let short = tab_bounds("top-tab-0", &mut cx);
    cx.simulate_mouse_move(short.center(), None, Modifiers::default());
    tab_bounds("top-tab-1", &mut cx);
    assert_eq!(cx.debug_bounds("top-tab-title-fade-1"), Some(fade));
    assert!(cx.debug_bounds("top-tab-title-moving-1").is_none());
    assert!(cx.debug_bounds("top-tab-title-moving-0").is_none(), "short titles stay still");

    cx.simulate_click(hover, Modifiers::default());
    cx.simulate_mouse_move(short.center(), None, Modifiers::default());
    tab_bounds("top-tab-1", &mut cx);
    assert_eq!(cx.debug_bounds("top-tab-title-fade-1"), Some(fade));
    assert!(cx.debug_bounds("top-tab-title-moving-1").is_none(), "selection must stay static");
    assert_eq!(cx.debug_bounds("top-tab-title-text-1"), Some(text));
    click_tab("top-tab-0", &mut cx);
    assert!(cx.debug_bounds("top-tab-title-moving-1").is_none());
}

#[gpui::test]
fn chrome_tabs_omit_separators_when_selected_or_hovered(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(4, cx);
    workspace.update(&mut cx, |workspace, cx| {
        for meta in &mut workspace.tab_meta {
            meta.custom_name = Some("sh".into());
        }
        cx.notify();
    });
    cx.update(|_, cx| {
        cx.global_mut::<crate::gpui_shell::config::Settings>().tab_style =
            nebula_settings::TabStyle::Chrome;
    });
    for selected in ["top-tab-2", "top-tab-0", "top-tab-4"] {
        click_tab(selected, &mut cx);
        let neighbor = tab_bounds("top-tab-1", &mut cx);
        cx.simulate_mouse_move(neighbor.center(), None, Modifiers::default());
        tab_bounds(selected, &mut cx);
        for separator in [
            "top-tab-separator-0",
            "top-tab-separator-1",
            "top-tab-separator-2",
            "top-tab-separator-3",
            "top-tab-separator-4",
        ] {
            assert!(cx.debug_bounds(separator).is_none());
        }
    }
}

#[gpui::test]
fn changing_tab_count_cancels_a_pending_horizontal_drag_snapshot(cx: &mut TestAppContext) {
    let (directory, workspace, mut cx) = open_workspace(2, cx);
    click_tab("top-tab-0", &mut cx);
    let path = directory.path().join("added-during-gesture.txt");
    std::fs::write(&path, "fixture\n").unwrap();
    for add in [true, false] {
        let bounds = tab_bounds("top-tab-0", &mut cx);
        let press = gpui::point(bounds.left() + px(14.0), bounds.center().y);
        cx.simulate_mouse_down(press, gpui::MouseButton::Left, Modifiers::default());
        workspace.read_with(&cx, |workspace, _| assert!(workspace.tab_drag.is_some()));
        workspace.update_in(&mut cx, |workspace, window, cx| {
            if add {
                let view = cx.new(|cx| {
                    crate::gpui_shell::code_tab::CodeTabView::new(path.clone(), window, cx)
                });
                let subscription = cx.subscribe(&view, |_, _, _, _| {});
                workspace.insert_tab_at(
                    workspace.tabs.len(),
                    WorkspaceTab::Code { view, _subscription: subscription },
                    TabMeta::default(),
                );
            } else {
                workspace.close_top_tab(2, window, cx);
            }
            cx.notify();
        });
        tab_bounds(if add { "top-tab-3" } else { "top-tab-2" }, &mut cx);
        workspace.read_with(&cx, |workspace, _| assert!(workspace.tab_drag.is_none()));
        cx.simulate_mouse_up(press, gpui::MouseButton::Left, Modifiers::default());
    }
}

#[gpui::test]
fn chrome_overlap_keeps_the_previous_tabs_close_button_clickable(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(3, cx);
    cx.update(|_, cx| {
        cx.global_mut::<crate::gpui_shell::config::Settings>().tab_style =
            nebula_settings::TabStyle::Chrome;
    });
    click_tab("top-tab-2", &mut cx);
    let previous = tab_bounds("top-tab-1", &mut cx);
    cx.simulate_mouse_move(previous.center(), None, Modifiers::default());
    tab_bounds("top-tab-1", &mut cx);
    let close = cx.debug_bounds("top-close-tab-1").unwrap();
    let selected = cx.debug_bounds("top-tab-2").unwrap();
    assert_eq!(selected.left() - close.right(), px(10.0));
    cx.simulate_click(close.center(), Modifiers::default());
    cx.run_until_parked();
    workspace.read_with(&cx, |workspace, _| {
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.active, 1, "closing the neighbor must retain the selected tab");
    });
}

#[gpui::test]
fn top_tab_drag_reorders_mixed_width_titles_in_both_directions(cx: &mut TestAppContext) {
    let (_directory, workspace, mut cx) = open_workspace(2, cx);
    cx.simulate_keystrokes(numbered_tab_shortcuts()[0]);
    workspace.update(&mut cx, |workspace, cx| {
        workspace.tab_meta[0].custom_name = Some("sh".into());
        workspace.tab_meta[1].custom_name = Some("a much longer terminal title".into());
        cx.notify();
    });
    for (source, target, expected) in [
        ("top-tab-0", "top-tab-1", "a much longer terminal title"),
        ("top-tab-1", "top-tab-0", "sh"),
    ] {
        let source_bounds = tab_bounds(source, &mut cx);
        // A short tab's center can be its close button; drag its leading padding.
        let source = gpui::point(source_bounds.left() + px(14.0), source_bounds.center().y);
        let target = tab_bounds(target, &mut cx).center();
        cx.simulate_mouse_down(source, gpui::MouseButton::Left, Modifiers::default());
        workspace.read_with(&cx, |workspace, _| {
            assert!(workspace.tab_drag.is_some(), "tab padding must arm dragging")
        });
        cx.simulate_mouse_move(target, Some(gpui::MouseButton::Left), Modifiers::default());
        workspace.read_with(&cx, |workspace, _| {
            assert!(
                workspace.tab_drag.as_ref().is_some_and(|drag| drag.active),
                "moving beyond the threshold must activate dragging"
            )
        });
        cx.simulate_mouse_up(target, gpui::MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        workspace.read_with(&cx, |workspace, _| {
            assert_eq!(workspace.tabs.len(), 2);
            assert_eq!(workspace.tab_meta[0].custom_name.as_deref(), Some(expected));
        });
    }
}
