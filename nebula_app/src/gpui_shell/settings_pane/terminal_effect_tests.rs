//! Keyboard routing and delayed picker/confirmation results use the real settings controls.
use super::*;

fn open_effect_settings(
    cx: &mut TestAppContext,
    effects: &str,
) -> (Entity<SettingsPane>, VisualTestContext) {
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(
        nebula_settings::settings_path(),
        format!("{TEST_SETTINGS}background_image_opacity=0.35\n{effects}"),
    )
    .unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.runtime = RuntimeSettings::load();
        pane.active_section = 1;
        cx.notify();
    });
    window.update(|_, cx| crate::gpui_shell::wallpaper::refresh(cx));
    draw(&mut window);
    let bounds = window.debug_bounds("nebula-btn-terminal-effect-choose").unwrap();
    window.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(px(900.0), px(700.0)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(400.0) - bounds.center().y)),
        touch_phase: gpui::TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(&mut window);
    let bounds = window.debug_bounds("nebula-btn-terminal-effect-choose").unwrap();
    assert!(bounds.top() >= px(0.0) && bounds.bottom() <= px(1000.0));
    (pane, window)
}

#[gpui::test]
fn keyboard_picker_enable_reload_and_remove_use_rendered_controls(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let (pane, mut window) = open_effect_settings(cx, "terminal_effect_animation=off\n");
    // 从相邻的可聚焦下拉框出发，验证真实 Tab 顺序，而非直接调用目标按钮回调。
    window.update(|window, cx| {
        let focus =
            pane.read(cx).select_of("terminal_effect_animation").unwrap().read(cx).focus_handle(cx);
        focus.focus(window, cx);
    });
    draw(&mut window);
    press("shift-tab", &mut window); // Enable; disabled Reload is skipped.
    press("shift-tab", &mut window); // Add WGSL.
    press("enter", &mut window);
    assert!(window.did_prompt_for_paths());
    window.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && options.multiple);
        Some(vec![std::path::PathBuf::from("效果.wgsl")])
    });
    draw(&mut window);
    assert_eq!(RuntimeSettings::load().terminal_effects.paths, ["效果.wgsl"]);
    assert!(!RuntimeSettings::load().terminal_effects.enabled);
    press("tab", &mut window);
    let toggle_focus = window.update(|window, cx| window.focused(cx));
    press("space", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    press("escape", &mut window);
    assert!(!RuntimeSettings::load().terminal_effects.enabled);
    press("enter", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    press("enter", &mut window);
    assert!(RuntimeSettings::load().terminal_effects.enabled);
    // 确认按钮在对话框退出动画后恢复焦点；虚拟时钟不会随测试按键自动前进。
    window.executor().advance_clock(*gpui_component::dialog::ANIMATION_DURATION);
    draw(&mut window);
    assert_eq!(window.update(|window, cx| window.focused(cx)), toggle_focus);
    let revision =
        window.read(|cx| crate::gpui_shell::wallpaper::terminal_effect_configuration(cx).1);
    press("tab", &mut window);
    press("space", &mut window);
    assert_eq!(
        window.read(|cx| crate::gpui_shell::wallpaper::terminal_effect_configuration(cx).1),
        revision + 1,
    );
    press("tab", &mut window); // Disabled Up/Down are skipped for the single source.
    press("enter", &mut window);
    let runtime = RuntimeSettings::load();
    assert!(runtime.terminal_effects.paths.is_empty());
    assert!(!runtime.terminal_effects.enabled);
    assert_eq!(runtime.background_image_opacity, 0.35);
}

#[gpui::test]
fn cancelled_and_empty_picker_results_preserve_enabled_sources(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let (pane, mut window) = open_effect_settings(
        cx,
        "terminal_effect_enabled=true\nterminal_effect_path=existing.wgsl\n",
    );
    let before = settings_file_snapshot();
    for response in [None, Some(Vec::new())] {
        click("nebula-btn-terminal-effect-choose", &mut window);
        assert!(window.did_prompt_for_paths());
        window.simulate_path_prompt_response(move |_| response);
        draw(&mut window);
        assert_eq!(settings_file_snapshot(), before);
        assert!(pane.read_with(&mut window, |pane, _| pane.terminal_effect_picker.is_none()));
    }
}

#[gpui::test]
fn delayed_picker_does_not_overwrite_newer_settings(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let (pane, mut window) = open_effect_settings(cx, "terminal_effect_path=original.wgsl\n");
    click("nebula-btn-terminal-effect-choose", &mut window);
    assert!(window.did_prompt_for_paths());
    pane.update(&mut window, |pane, cx| {
        pane.persist(&[("terminal_effect_path", "newer.wgsl".into())], cx);
    });
    let newer = settings_file_snapshot();
    window.simulate_path_prompt_response(|_| Some(vec![std::path::PathBuf::from("late.wgsl")]));
    draw(&mut window);
    assert_eq!(settings_file_snapshot(), newer);
    assert_eq!(RuntimeSettings::load().terminal_effects.paths, ["newer.wgsl"]);
    assert!(pane.read_with(&mut window, |pane, _| pane.terminal_effect_picker.is_none()));
}

#[gpui::test]
fn stale_confirmation_does_not_authorize_replaced_sources(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let (pane, mut window) = open_effect_settings(cx, "terminal_effect_path=original.wgsl\n");
    click("nebula-btn-terminal-effect-toggle", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    pane.update(&mut window, |pane, cx| {
        pane.persist(&[("terminal_effect_path", "replacement.wgsl".into())], cx);
    });
    press("enter", &mut window);
    let runtime = RuntimeSettings::load();
    assert_eq!(runtime.terminal_effects.paths, ["replacement.wgsl"]);
    assert!(!runtime.terminal_effects.enabled);
}
