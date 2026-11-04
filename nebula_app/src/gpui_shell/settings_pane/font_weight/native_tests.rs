//! Opt-in screenshots of the real font weight control, without desktop input injection.

use super::*;
use gpui::{Bounds, Keystroke, WindowBounds, WindowOptions, point};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct PreviewHost(gpui::Entity<SettingsPane>);

impl Render for PreviewHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        // The real workspace owns this surface; SettingsPane is transparent.
        div()
            .size_full()
            .bg(crate::gpui_shell::theme::settings_content_bg(cx))
            .child(self.0.clone())
    }
}

#[test]
#[ignore = "requires a native desktop and isolated PEBREL_FONT_WEIGHT_QA_DIR/config"]
fn native_font_weight_menu() {
    let output =
        PathBuf::from(std::env::var_os("PEBREL_FONT_WEIGHT_QA_DIR").expect("QA directory"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("ready.json").exists(), "use a fresh QA directory");
    let width = std::env::var("PEBREL_FONT_WEIGHT_QA_WIDTH")
        .map(|value| value.parse::<f32>().expect("QA width must be a number"))
        .unwrap_or(1100.0);
    assert!(width.is_finite() && width >= 700.0, "QA width must be at least 700");
    let outcome = Arc::new(Mutex::new(None));
    let result = outcome.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        gpui_component::init(cx);
        crate::gpui_shell::typography::init(cx);
        let runtime = RuntimeSettings::load();
        cx.set_global(crate::gpui_shell::config::Settings::load_with_runtime(runtime.theme, runtime));
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let mut pane = None;
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(40.0), px(40.0)), gpui::size(px(width), px(900.0)),
                ))),
                focus: false,
                show: true,
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| SettingsPane::new(window, cx));
                view.update(cx, |pane, _| pane.active_section = 1);
                pane = Some(view.clone());
                let host = cx.new(|_| PreviewHost(view));
                cx.new(|cx| gpui_component::Root::new(host, window, cx))
            },
        ).unwrap();
        let pane = pane.unwrap();
        cx.spawn(async move |cx| {
            let opened: Result<(), String> = async {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            cx.update_window(handle.into(), |_, window, cx| -> Result<(), String> {
                let _ = window.draw(cx);
                let selector = "settings-select-font_weight";
                let bounds = window.debug_bounds_for_selector(selector).ok_or("font weight was not rendered")?;
                let viewport = window.viewport_size();
                if bounds.top() < px(0.0) || bounds.bottom() > viewport.height {
                    window.dispatch_event(gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                        position: point(viewport.width / 2.0, viewport.height / 2.0),
                        delta: gpui::ScrollDelta::Pixels(point(px(0.0), viewport.height / 2.0 - bounds.center().y)),
                        touch_phase: gpui::TouchPhase::Moved,
                        modifiers: Default::default(),
                    }), cx);
                    let _ = window.draw(cx);
                }
                let bounds = window.debug_bounds_for_selector(selector).ok_or("font weight disappeared after scrolling")?;
                if bounds.top() < px(0.0) || bounds.bottom() > viewport.height
                    || bounds.left() < px(0.0) || bounds.right() > viewport.width
                {
                    return Err("font weight selector is outside the visible viewport".into());
                }
                let view = pane.read(cx);
                let select = view.select_of("font_weight").ok_or("missing font weight selector")?;
                let row = select.read(cx).selected_index(cx).ok_or("missing selected weight")?.row;
                if nebula_settings::TerminalFontWeight::VALUES[row] != view.runtime.font_weight.settings_value() {
                    return Err("visible selection differs from saved weight".into());
                }
                std::fs::write(output.join("ready.json"), serde_json::to_vec(&serde_json::json!({
                    "pid": std::process::id(),
                    "weight": view.runtime.font_weight.settings_value(),
                    "label": select.read(cx).selected_value().map(|label| label.to_string()),
                    "scale_factor": window.scale_factor(),
                    "viewport": { "width": f32::from(viewport.width), "height": f32::from(viewport.height) },
                    "selector_bounds": {
                        "x": f32::from(bounds.left()), "y": f32::from(bounds.top()),
                        "width": f32::from(bounds.size.width), "height": f32::from(bounds.size.height),
                    },
                })).unwrap()).map_err(|error| error.to_string())
            }).map_err(|error| error.to_string())??;
                for _ in 0..300 {
                    if output.join("open-menu").exists() { break; }
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                }
                if !output.join("open-menu").exists() {
                    return Err("timed out waiting for the menu capture request".into());
                }
                cx.update_window(handle.into(), |_, window, cx| -> Result<(), String> {
                    let select = pane.read(cx).select_of("font_weight").unwrap().clone();
                    let trigger = select.read(cx).focus_handle(cx);
                    trigger.focus(window, cx);
                    window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                    let _ = window.draw(cx);
                    // Select's public Focusable returns the list handle only while open.
                    let list = select.read(cx).focus_handle(cx);
                    if list == trigger || !list.is_focused(window) {
                        return Err("font weight menu did not open and receive keyboard focus".into());
                    }
                    Ok(())
                }).map_err(|error| error.to_string())??;
                cx.background_executor().timer(Duration::from_millis(250)).await;
                std::fs::write(output.join("menu-ready.json"), "{}").map_err(|error| error.to_string())?;
                for _ in 0..300 {
                    if output.join("capture-complete").exists() { break; }
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                }
                if !output.join("capture-complete").exists() {
                    return Err("timed out waiting for screenshots".into());
                }
                Ok(())
            }.await;
            *result.lock().unwrap() = Some(opened);
            drop(pane);
            let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
            cx.update(|cx| cx.quit());
        }).detach();
    });
    assert_eq!(*outcome.lock().unwrap(), Some(Ok(())));
}
