//! Native appearance check using the real settings pane and Select control.
//! Keyboard input goes directly to this test window, never to the desktop.

use super::*;
use gpui::{Bounds, Keystroke, WindowBounds, WindowOptions, point};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
#[ignore = "requires a native desktop and an isolated PEBREL_NOTIFICATION_QA_DIR/config"]
fn native_notification_duration_menu_keeps_all_choices_visible() {
    let output = PathBuf::from(std::env::var_os("PEBREL_NOTIFICATION_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    let theme = std::env::var("PEBREL_NOTIFICATION_QA_THEME").unwrap_or_else(|_| "Nord".into());
    let theme = ThemeName::from_prompt_name(&theme).expect("built-in theme");
    let chinese =
        std::env::var("PEBREL_NOTIFICATION_QA_LANGUAGE").is_ok_and(|value| value == "zh-CN");
    let expected_language =
        if chinese { crate::display::UiLanguage::ZhCn } else { crate::display::UiLanguage::EnUs };
    let ready = output.join("ready.json");
    assert!(!ready.exists(), "use a fresh QA output directory");
    let result = Arc::new(Mutex::new(None));
    let after_run = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(theme));
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let mut pane = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(60.0), px(70.0)),
                            gpui::size(px(900.0), px(600.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| SettingsPane::new(window, cx));
                        view.update(cx, |pane, _| pane.active_section = 2);
                        pane = Some(view.clone());
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let pane = pane.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let opened = cx
                    .update_window(handle.into(), |_, window, cx| -> Result<(), String> {
                        if crate::gpui_shell::config::ui_language(cx) != expected_language
                            || crate::gpui_shell::theme::effective_theme_name(cx) != theme
                            || cx.theme().is_dark()
                                == crate::gpui_shell::theme::chrome_theme(theme).skin().is_light
                        {
                            return Err(
                                "the live theme/language differs from the requested fixture".into(),
                            );
                        }
                        let view = pane.read(cx);
                        if view.runtime.notification_duration
                            != nebula_settings::NotificationDuration::ThirtySeconds
                            || view.runtime.ai_toasts
                        {
                            return Err(
                                "the duration and delivery switch are not independently loaded"
                                    .into(),
                            );
                        }
                        let select = view
                            .select_of("notification_duration")
                            .ok_or("duration control missing")?;
                        if select.read(cx).selected_index(cx).map(|path| path.row) != Some(3) {
                            return Err(
                                "duration selection does not reflect the persisted value".into()
                            );
                        }
                        let focus = select.read(cx).focus_handle(cx);
                        // The Alerts group follows Completion. Scroll the real settings
                        // viewport before opening its menu; an offscreen anchor is not QA.
                        window.dispatch_event(
                            gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                                position: point(px(500.0), px(400.0)),
                                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-1600.0))),
                                touch_phase: gpui::TouchPhase::Moved,
                                modifiers: Default::default(),
                            }),
                            cx,
                        );
                        let _ = window.draw(cx);
                        focus.focus(window, cx);
                        let _ = window.draw(cx);
                        window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                        let _ = window.draw(cx);
                        Ok(())
                    })
                    .map_err(|error| error.to_string())
                    .and_then(|result| result);
                if opened.is_ok() {
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                    std::fs::write(
                        &ready,
                        serde_json::to_vec(&serde_json::json!({
                            "pid": std::process::id(), "theme": theme.prompt_name(),
                            "language": if chinese { "zh-CN" } else { "en-US" },
                            "selected_seconds": 30, "ai_toasts_enabled": false,
                        }))
                        .unwrap(),
                    )
                    .unwrap();
                    for _ in 0..100 {
                        if output.join("capture-complete").exists() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                    }
                }
                *result.lock().unwrap() = Some(opened);
                drop(pane);
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert_eq!(*after_run.lock().unwrap(), Some(Ok(())));
}
