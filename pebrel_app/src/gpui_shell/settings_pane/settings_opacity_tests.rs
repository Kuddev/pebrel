//! The preference is exercised through the production switch and reset controls.
use super::*;

#[gpui::test]
fn settings_opacity_switch_supports_keyboard_mouse_and_reset(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    std::fs::create_dir_all(pebrel_settings::settings_dir()).unwrap();
    std::fs::write(
        pebrel_settings::settings_path(),
        format!("{TEST_SETTINGS}opacity=0.3\ncustom=keep\n"),
    )
    .unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.runtime = RuntimeSettings::load();
        pane.active_section = 1;
        pane.appearance_section = Some(AppearanceSection::Interface);
        cx.notify();
    });
    window.simulate_resize(size(px(1100.0), px(1200.0)));
    draw(&mut window);
    let selector = "nebula-switch-settings_follow_opacity";
    let bounds = window.debug_bounds(selector).expect("visible opacity choice");
    assert!(bounds.top() >= px(0.0) && bounds.bottom() <= px(1200.0));
    assert!(!RuntimeSettings::load().settings_follow_opacity);
    window.update(|window, cx| {
        pane.read(cx).select_of("blur").unwrap().read(cx).focus_handle(cx).focus(window, cx);
    });
    draw(&mut window);
    press("shift-tab", &mut window); // Background blur's details button.
    press("shift-tab", &mut window); // The preceding opacity switch.
    for enabled in [true, false] {
        press("space", &mut window);
        assert_eq!(RuntimeSettings::load().settings_follow_opacity, enabled);
        assert_eq!(
            pane.read_with(&window, |pane, _| pane.runtime.settings_follow_opacity),
            enabled
        );
        assert_eq!(
            window.read(|cx| cx
                .global::<crate::gpui_shell::config::Settings>()
                .settings_follow_opacity),
            enabled
        );
    }
    click(selector, &mut window);
    assert!(RuntimeSettings::load().settings_follow_opacity);
    let reset = window.read(|cx| {
        format!(
            "setting-reset-{}",
            crate::gpui_shell::config::ui_language(cx)
                .text(crate::i18n::Message::SettingsOpacityFollow)
        )
    });
    let reset_bounds = window.update(|window, _| window.debug_bounds_for_selector(&reset)).unwrap();
    click_point(reset_bounds.center(), &mut window);
    assert!(!RuntimeSettings::load().settings_follow_opacity);
    assert_eq!(RuntimeSettings::load().opacity, 0.3);
    assert!(
        std::fs::read_to_string(pebrel_settings::settings_path()).unwrap().contains("custom=keep")
    );
    assert!(window.update(|window, _| window.debug_bounds_for_selector(&reset)).is_none());

    window.simulate_resize(size(px(620.0), px(800.0)));
    draw(&mut window);
    let bounds = window.debug_bounds(selector).unwrap();
    window.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(px(450.0), px(700.0)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(400.0) - bounds.center().y)),
        touch_phase: gpui::TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(&mut window);
    let bounds = window.debug_bounds(selector).unwrap();
    assert!(bounds.left() >= px(0.0) && bounds.right() <= px(620.0));
    assert!(bounds.top() >= px(0.0) && bounds.bottom() <= px(800.0));
    let padding = gpui::point(bounds.left() + px(1.0), bounds.top() + px(1.0));
    window.simulate_mouse_move(padding, None, Modifiers::default());
    window.simulate_mouse_down(padding, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    assert!(!RuntimeSettings::load().settings_follow_opacity);
    window.simulate_mouse_up(padding, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    assert!(RuntimeSettings::load().settings_follow_opacity);
    click(selector, &mut window);

    // Exercise a real failed save without changing permissions or user settings.
    let path = pebrel_settings::settings_path();
    let saved = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    click(selector, &mut window);
    let (visible, applied, notified) = window.update(|window, cx| {
        let root = window.root::<Root>().flatten().unwrap();
        (
            pane.read(cx).runtime.settings_follow_opacity,
            cx.global::<crate::gpui_shell::config::Settings>().settings_follow_opacity,
            !root.read(cx).notification.read(cx).notifications().is_empty(),
        )
    });
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, &saved).unwrap();
    assert!(!visible && !applied, "failed saves keep the committed choice");
    assert!(notified, "failed saves have visible feedback");
    assert_eq!(std::fs::read(&path).unwrap(), saved);
}

#[gpui::test]
fn settings_opacity_choice_fades_surfaces_and_keeps_text_and_popups_opaque(
    cx: &mut TestAppContext,
) {
    use crate::gpui_shell::config::Settings;
    use pebrel_settings::{RawSettings, RuntimeSettings};

    let _lock = lock_theme_studio();
    cx.update(|cx| {
        gpui_component::init(cx);
        for name in [ThemeName::Nord, ThemeName::NordLight] {
            let runtime = RuntimeSettings::from_raw(&RawSettings::default());
            cx.set_global(Settings::load_with_runtime(name, runtime));
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            crate::gpui_shell::wallpaper::test_install_visual_effects(
                cx,
                1.0,
                pebrel_settings::BlurModeName::None,
            );
            let foreground = cx.theme().foreground;
            let default_card =
                design::settings_card(cx).style().background.as_ref().unwrap().color().unwrap();
            for enabled in [false, true] {
                cx.global_mut::<Settings>().settings_follow_opacity = enabled;
                for opacity in [1.0, 0.5, 0.0, 1.0] {
                    crate::gpui_shell::wallpaper::set_opacity_live(opacity, cx);
                    let expected = if enabled { opacity } else { 1.0 };
                    assert!(
                        (crate::gpui_shell::theme::settings_navigation_bg(cx).a - expected).abs()
                            < 1e-6
                    );
                    let mut card = design::settings_card(cx);
                    let background = card.style().background.as_ref().unwrap().color().unwrap();
                    assert_eq!(background, default_card.opacity(expected));
                    assert_eq!(background.is_transparent(), expected == 0.0);
                    assert_eq!(card.style().opacity, None, "children retain their opacity");
                    if expected == 0.0 {
                        assert!(
                            crate::gpui_shell::theme::settings_card_shadow(cx)
                                .iter()
                                .all(|shadow| shadow.color.a == 0.0)
                        );
                        assert_eq!(crate::gpui_shell::theme::settings_canvas_tint(cx).a, 0.0);
                    }
                    assert_eq!(crate::gpui_shell::theme::settings_panel_bg(cx).a, 1.0);
                    assert_eq!(cx.theme().foreground, foreground);
                }
            }
        }
    });
}

/// Render the actual Settings page and wallpaper with the native font/GPU stack.
/// Supply an isolated PEBREL_CONFIG_DIR and an empty PEBREL_OPACITY_QA_DIR, then
/// capture this process's off-screen window before creating `capture-complete`.
#[test]
#[ignore = "requires a desktop and isolated settings for native screenshot acceptance"]
fn native_settings_opacity_preview() {
    use gpui::{Bounds, WindowBounds, WindowOptions, point};
    use std::{path::PathBuf, time::Duration};

    struct Preview(Entity<SettingsPane>);
    impl Render for Preview {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(crate::gpui_shell::theme::settings_panel_bg(cx))
                .child(crate::gpui_shell::wallpaper::window_layer(None, cx))
                .child(div().absolute().inset_0().child(self.0.clone()))
        }
    }

    let output =
        PathBuf::from(std::env::var_os("PEBREL_OPACITY_QA_DIR").expect("set QA directory"));
    let config = PathBuf::from(std::env::var_os("PEBREL_CONFIG_DIR").expect("set isolated config"));
    assert!(config.starts_with(&output), "keep the native fixture settings in its QA directory");
    let ready = output.join("ready.json");
    assert!(!ready.exists(), "use a fresh QA directory");
    let ready_after_run = ready.clone();
    let width = std::env::var("PEBREL_OPACITY_QA_WIDTH")
        .unwrap_or_else(|_| "1100".into())
        .parse::<f32>()
        .expect("numeric width");
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        let runtime = RuntimeSettings::load();
        cx.set_global(crate::gpui_shell::config::Settings::load_with_runtime(
            runtime.theme,
            runtime,
        ));
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(-10000.0), px(-10000.0)),
                        size(px(width), px(1200.0)),
                    ))),
                    focus: false,
                    show: true,
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Pebrel Settings opacity QA");
                    let pane = cx.new(|cx| {
                        let mut pane = SettingsPane::new(window, cx);
                        pane.active_section = 1;
                        pane.appearance_section = Some(AppearanceSection::Interface);
                        pane
                    });
                    let view = cx.new(|_| Preview(pane));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
        crate::gpui_shell::wallpaper::refresh(cx);
        cx.spawn(async move |cx| {
            for _ in 0..100 {
                if cx.update(|cx| crate::gpui_shell::wallpaper::wallpaper_available_for_preview(cx))
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(50)).await;
            }
            assert!(
                cx.update(|cx| crate::gpui_shell::wallpaper::wallpaper_available_for_preview(cx))
            );
            cx.update_window(handle.into(), |_, window, cx| {
                let _ = window.draw(cx);
            })
            .unwrap();
            std::fs::write(
                &ready,
                serde_json::to_vec(&serde_json::json!({ "pid": std::process::id() })).unwrap(),
            )
            .unwrap();
            for _ in 0..120 {
                if output.join("capture-complete").exists() {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(500)).await;
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    assert!(ready_after_run.exists(), "native Settings was not rendered");
}
