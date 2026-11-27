//! Native visual acceptance of the production lifecycle settings page.

use super::*;
use gpui::{Bounds, WindowBounds, WindowOptions, point};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
#[ignore = "requires a native desktop and isolated PEBREL_RESIDENCY_QA_DIR/config"]
fn native_private_residency_settings() {
    let output = PathBuf::from(std::env::var_os("PEBREL_RESIDENCY_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    let explicit = std::env::var("PEBREL_RESIDENCY_QA_EXPLICIT").as_deref() == Ok("1");
    crate::platform::startup::instance::initialize(explicit);
    assert!(crate::platform::startup::instance::is_private());
    let theme = std::env::var("PEBREL_RESIDENCY_QA_THEME").unwrap_or_else(|_| "Nord".into());
    let theme = ThemeName::from_prompt_name(&theme).expect("built-in theme");
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
                            gpui::size(px(880.0), px(720.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| SettingsPane::new(window, cx));
                        view.update(cx, |pane, _| pane.active_section = 8);
                        pane = Some(view.clone());
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let pane = pane.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let check = cx
                    .update_window(handle.into(), |_, window, cx| -> Result<(), String> {
                        if crate::platform::startup::instance::can_keep_session()
                            || !pane.read(cx).runtime.keep_session
                        {
                            return Err(
                                "fixture must preserve an enabled preference in a private instance"
                                    .into(),
                            );
                        }
                        let _ = window.draw(cx);
                        let bounds = window
                            .debug_bounds_for_selector("nebula-switch-keep_session")
                            .ok_or("residency switch was not laid out")?;
                        if bounds.top() < px(0.0) || bounds.bottom() > window.viewport_size().height
                        {
                            return Err("residency switch is outside the settings viewport".into());
                        }
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: bounds.center(),
                                pressed_button: None,
                                modifiers: Default::default(),
                            }),
                            cx,
                        );
                        let _ = window.draw(cx);
                        std::fs::write(&ready, serde_json::to_vec(&serde_json::json!({
                        "pid": std::process::id(), "theme": theme.prompt_name(),
                        "explicit": explicit, "can_keep_session": false,
                        "saved_keep_session": true,
                        "language": format!("{:?}", crate::gpui_shell::config::ui_language(cx)),
                        "switch_bounds": format!("{bounds:?}"),
                    })).unwrap()).map_err(|error| error.to_string())?;
                        Ok(())
                    })
                    .map_err(|error| error.to_string())
                    .and_then(|value| value);
                let mut captured = false;
                if check.is_ok() {
                    for _ in 0..300 {
                        if output.join("capture-complete").exists() {
                            captured = true;
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                    }
                }
                *result.lock().unwrap() = Some(check.and_then(|()| {
                    if captured { Ok(()) } else { Err("native capture was not verified".into()) }
                }));
                drop(pane);
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert_eq!(*after_run.lock().unwrap(), Some(Ok(())));
}
