//! Rendered settings navigation, card layout, and reset hit targets.

use super::*;

#[gpui::test]
fn settings_cards_fit_the_window_and_keep_wrapped_controls_inside(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (_, mut window) = open_settings(cx);
    click("settings-nav-1", &mut window);
    let title = window.read(|cx| {
        let language = crate::gpui_shell::config::ui_language(cx);
        language.text(crate::i18n::Message::CommonTheme)
    });
    let card_selector = format!("settings-group-card-{title}");
    let row_selector = format!("settings-row-{title}");
    for (width, wraps) in [(1400.0, false), (620.0, true)] {
        window.simulate_resize(size(px(width), px(1000.0)));
        draw(&mut window);
        let area = window.debug_bounds("settings-content-area").unwrap();
        let column = window.debug_bounds("settings-content-column").unwrap();
        let card =
            window.update(|window, _| window.debug_bounds_for_selector(&card_selector)).unwrap();
        let theme_picker = window.debug_bounds("open-theme-picker").unwrap();
        let toggle = window.debug_bounds("nebula-switch-follow_system_theme").unwrap();
        assert_eq!(column.size.width, area.size.width.min(px(design::CONTENT_MAX_WIDTH)));
        assert!((f32::from(column.center().x - area.center().x)).abs() < 1.0);
        assert_eq!(card.left() - column.left(), px(design::CONTENT_PAD_X));
        assert_eq!(column.right() - card.right(), px(design::CONTENT_PAD_X));
        for control in [theme_picker, toggle] {
            assert!(control.left() >= card.left() && control.right() <= card.right());
            assert!(control.top() >= card.top() && control.bottom() <= card.bottom());
        }
        let theme_row =
            window.update(|window, _| window.debug_bounds_for_selector(&row_selector)).unwrap();
        assert_eq!(theme_picker.center().y - theme_row.center().y > px(12.0), wraps);
    }
}

#[gpui::test]
fn modified_setting_reset_works_away_from_hover(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    let (pane, mut window) = open_settings(cx);
    click("settings-nav-6", &mut window);
    let default = SettingsPane::factory_defaults().copy_on_select;
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.toggle("copy_on_select", !default, window, cx));
    });
    window.simulate_mouse_move(gpui::point(px(2.0), px(2.0)), None, Modifiers::default());
    draw(&mut window);
    let selector = window.read(|cx| {
        let language = crate::gpui_shell::config::ui_language(cx);
        format!("setting-reset-{}", language.pick("选中即复制", "Copy on select"))
    });
    let reset = window
        .update(|window, _| window.debug_bounds_for_selector(&selector))
        .expect("modified setting has a reset button outside row hover");
    assert_eq!(reset.size, size(px(32.0), px(32.0)));
    click_point(gpui::point(reset.center().x, reset.top() + px(1.0)), &mut window);
    assert_eq!(pane.read_with(&window, |pane, _| pane.runtime.copy_on_select), default);
    assert_eq!(RuntimeSettings::load().copy_on_select, default);
    assert!(window.update(|window, _| window.debug_bounds_for_selector(&selector)).is_none());
}

#[gpui::test]
fn settings_navigation_scrolls_without_squeezing_rows_or_moving_fixed_controls(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    window.simulate_resize(size(px(1000.0), px(560.0)));
    draw(&mut window);
    let viewport = window.debug_bounds("settings-nav-scroll").unwrap();
    let back = window.debug_bounds("settings-back").unwrap();
    let restore = window.debug_bounds("settings-restore-defaults").unwrap();
    let first = window.debug_bounds("settings-nav-0").unwrap();
    let child = window.debug_bounds("settings-nav-child-1-0").unwrap();
    assert_eq!(first.size.height, px(SETTINGS_NAV_ROW_HEIGHT));
    assert_eq!(child.size.height, px(SETTINGS_NAV_CHILD_HEIGHT));
    assert_eq!(first.size.height, px(32.0));
    assert_eq!(child.size.height, px(28.0));
    let scrollbar = window.debug_bounds("settings-nav-scrollbar").unwrap();
    assert_eq!(first.right(), scrollbar.left());
    assert_eq!(scrollbar.right(), px(SETTINGS_NAV_WIDTH));
    assert_eq!(child.size.width, first.size.width - px(20.0));
    assert!(back.bottom() <= viewport.top());
    assert!(viewport.bottom() <= restore.top());
    assert!(restore.bottom() <= px(560.0));

    window.simulate_event(gpui::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-1000.0))),
        touch_phase: gpui::TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(&mut window);
    assert!(pane.read_with(&window, |pane, _| pane.nav_scroll.offset().y) < px(0.0));
    assert_eq!(window.debug_bounds("settings-back").unwrap(), back);
    assert_eq!(window.debug_bounds("settings-restore-defaults").unwrap(), restore);
    let backup = window.debug_bounds("settings-nav-9").unwrap();
    assert!(backup.top() >= viewport.top() && backup.bottom() <= viewport.bottom());
    assert_eq!(backup.size.height, px(SETTINGS_NAV_ROW_HEIGHT));
    click("settings-nav-9", &mut window);
    assert_eq!(pane.read_with(&window, |pane, _| pane.active_section), 9);

    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input
                .update(cx, |input, cx| input.replace_all("prediction", window, cx));
        });
    });
    draw(&mut window);
    let result = window.debug_bounds("settings-nav-3").unwrap();
    assert!(result.top() >= viewport.top() && result.bottom() <= viewport.bottom());
    assert_eq!(pane.read_with(&window, |pane, _| pane.nav_scroll.offset().y), px(0.0));
}

#[gpui::test]
fn settings_cards_use_desktop_width_and_keep_outer_spacing(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (_, mut window) = open_settings(cx);
    click("settings-nav-child-1-0", &mut window);
    let card_selector = window.read(|cx| {
        let language = crate::gpui_shell::config::ui_language(cx);
        format!("settings-group-card-{}", NavChild::Theme.label(language))
    });
    for width in [1400.0, 900.0, 620.0] {
        window.simulate_resize(size(px(width), px(1000.0)));
        draw(&mut window);
        let area = window.debug_bounds("settings-content-area").unwrap();
        let divider = window.debug_bounds("settings-nav-divider").unwrap();
        assert_eq!(divider.left(), px(248.0));
        assert_eq!(divider.size.width, px(1.0));
        assert_eq!(area.left(), divider.right());
        let column = window.debug_bounds("settings-content-column").unwrap();
        let card =
            window.update(|window, _| window.debug_bounds_for_selector(&card_selector)).unwrap();
        assert_eq!(column.size.width, area.size.width.min(px(design::CONTENT_MAX_WIDTH)));
        if width == 1400.0 {
            assert_eq!(card.size.width, px(896.0));
        }
        assert!((f32::from(column.center().x - area.center().x)).abs() < 1.0);
        assert_eq!(card.left() - column.left(), px(design::CONTENT_PAD_X));
        assert_eq!(column.right() - card.right(), px(design::CONTENT_PAD_X));
        assert!(card.top() - column.top() >= px(design::CONTENT_PAD_TOP));
        assert!(column.bottom() - card.bottom() >= px(design::CONTENT_PAD_BOTTOM));
        let switch = window.debug_bounds("nebula-switch-follow_system_theme").unwrap();
        assert!(switch.size.height >= px(crate::gpui_shell::widgets::SETTINGS_HIT_SIZE));
        assert!(switch.left() >= card.left() && switch.right() <= card.right());
    }
}

#[gpui::test]
fn modified_setting_reset_remains_clickable_away_from_hover_and_disappears_after_reset(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    let (pane, mut window) = open_settings(cx);
    let default = SettingsPane::factory_defaults().copy_on_select;
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.active_section = 6;
            pane.active_nav_child = Some(NavChild::Mouse);
            pane.toggle("copy_on_select", !default, window, cx);
        });
    });
    draw(&mut window);
    let label = window
        .read(|cx| crate::gpui_shell::config::ui_language(cx).pick("选中即复制", "Copy on select"));
    let selector = Box::leak(format!("setting-reset-{label}").into_boxed_str());
    window.simulate_mouse_move(gpui::point(px(2.0), px(2.0)), None, Modifiers::default());
    draw(&mut window);
    let reset = window.debug_bounds(selector).expect("modified setting has a reset button");
    assert_eq!(reset.size, size(px(32.0), px(32.0)));
    let face_selector = Box::leak(format!("setting-reset-face-{label}").into_boxed_str());
    let face = window.debug_bounds(face_selector).expect("inset reset face");
    assert_eq!(face.size, size(px(30.0), px(26.0)));
    assert_eq!(face.center(), reset.center());
    // The outer padding must still activate the button.
    click_point(gpui::point(reset.center().x, reset.top() + px(1.0)), &mut window);
    assert_eq!(pane.read_with(&window, |pane, _| pane.runtime.copy_on_select), default);
    assert_eq!(RuntimeSettings::load().copy_on_select, default);
    assert!(window.debug_bounds(selector).is_none());
}
