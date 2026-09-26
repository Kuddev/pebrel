//! Opt-in real GPUI/WinRT lifecycle acceptance; no terminal, settings or user data.

use crate::gpui_shell::wallpaper::{
    VisualEffects, test_apply_window_effects as apply_window_effects, test_install_visual_effects,
};
use crate::platform::acrylic;
use gpui::{App, AppContext, AsyncApp, Context, Render, WindowHandle, WindowOptions};
use gpui::{IntoElement, Styled, Window, WindowBackgroundAppearance, div};
use nebula_settings::BlurModeName;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

struct EmptyWindow;
impl Render for EmptyWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn open(cx: &mut App) -> WindowHandle<EmptyWindow> {
    cx.open_window(
        WindowOptions {
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |_, cx| cx.new(|_| EmptyWindow),
    )
    .unwrap()
}

async fn settle(cx: &AsyncApp, expected: usize) -> Result<(), String> {
    cx.background_executor().timer(Duration::from_millis(180)).await;
    let actual = acrylic::test_snapshot().0;
    if actual != expected {
        return Err(format!("expected {expected} native backdrops, observed {actual}"));
    }
    Ok(())
}

async fn exercise(first: WindowHandle<EmptyWindow>, cx: &AsyncApp) -> Result<(), String> {
    settle(cx, 1).await?;
    let second = cx.update(|cx| {
        let window = open(cx);
        apply_window_effects(cx);
        window
    });
    settle(cx, 2).await?;
    for mode in [
        BlurModeName::Aero,
        BlurModeName::Acrylic,
        BlurModeName::Mica,
        BlurModeName::Acrylic,
        BlurModeName::MicaAlt,
        BlurModeName::Acrylic,
        BlurModeName::None,
        BlurModeName::Acrylic,
    ] {
        cx.update(|cx| {
            cx.global_mut::<VisualEffects>().blur = mode;
            apply_window_effects(cx);
        });
        settle(cx, if mode == BlurModeName::Acrylic { 2 } else { 0 }).await?;
    }
    let before = acrylic::test_snapshot();
    for value in 0..=20 {
        cx.update(|cx| {
            cx.global_mut::<VisualEffects>().opacity = value as f32 / 20.0;
            apply_window_effects(cx);
        });
    }
    settle(cx, 2).await?;
    if acrylic::test_snapshot() != before {
        return Err("opacity refresh recreated native resources".into());
    }
    cx.update(|cx| first.update(cx, |_, window, _| window.remove_window()))
        .map_err(|error| error.to_string())?;
    settle(cx, 1).await?;
    cx.update(|cx| second.update(cx, |_, window, _| window.refresh()))
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
#[ignore = "opens real GPUI windows; requires Windows 11 and Windows App Runtime 1.8"]
fn native_acrylic_gpui_material_switches_and_window_lifecycle() {
    let result = Rc::new(RefCell::new(None));
    let shared_result = result.clone();
    let timed_out = Rc::new(Cell::new(false));
    let watchdog = timed_out.clone();
    let guard = acrylic::RunGuard::default();
    gpui_platform::application().run(move |cx| {
        acrylic::init(cx);
        test_install_visual_effects(cx, 0.4, BlurModeName::Acrylic);
        let first = open(cx);
        apply_window_effects(cx);
        cx.spawn(async move |cx| {
            *shared_result.borrow_mut() = Some(exercise(first, cx).await);
            cx.update(|cx| cx.quit());
        })
        .detach();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(15)).await;
            watchdog.set(true);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    drop(guard);
    assert!(!timed_out.get(), "normal GPUI quit must finish before the watchdog");
    assert_eq!(result.borrow_mut().take(), Some(Ok(())));
    assert_eq!(acrylic::test_snapshot().0, 0);
    assert!(windows::System::DispatcherQueue::GetForCurrentThread().is_err());
}
