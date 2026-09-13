use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    MouseButton, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, Window, div, point, px,
};
use gpui_component::{
    ActiveTheme as _, Root, Theme, ThemeMode, WindowExt as _,
    text::{TextView, TextViewState},
};
use nebula_settings::ThemeName;

use super::{apply_skin_tokens, chrome_theme, wash};

fn apply_reader_theme(name: ThemeName, cx: &mut App) {
    let chrome = chrome_theme(name);
    let mode = if chrome.skin().is_light { ThemeMode::Light } else { ThemeMode::Dark };
    Theme::change(mode, None, cx);
    apply_skin_tokens(chrome, cx);
    // Match the product shell at full opacity without loading wallpaper,
    // preferences, or tray integrations into the isolated test process.
    let background = super::shell_color(chrome);
    let theme = Theme::global_mut(cx);
    theme.background = background;
    theme.tokens.background = background.into();
}

#[gpui::test]
fn text_selection_theme_preserves_rgb_and_caps_overlay_alpha(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        for name in ThemeName::BUILTIN {
            let chrome = chrome_theme(name);
            let original = wash(chrome.skin().accent_soft);
            apply_reader_theme(name, cx);

            let theme = cx.theme();
            let selection = theme.selection;
            assert_eq!(
                (selection.h, selection.s, selection.l),
                (original.h, original.s, original.l)
            );
            assert_eq!(selection.a, original.a.min(0.3), "{name:?}");
            assert!(selection.a > 0.0, "{name:?}");
            assert_eq!(theme.tokens.selection.color, selection, "{name:?}");
            assert_eq!(theme.tokens.selection.background, selection.into(), "{name:?}");
            // Solid selection surfaces for lists are not text overlays.
            assert_eq!(theme.list_active, original, "{name:?}");
        }
    });
}

const SELECTED_TEXT: &str = "alpha beta gamma delta";

struct ReaderSelectionFixture {
    text: Entity<TextViewState>,
    width: f32,
}

impl Render for ReaderSelectionFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("reader-selection-fixture")
            .debug_selector(|| "reader-selection-fixture".to_owned())
            .w(px(self.width))
            .h(px(180.0))
            .text_size(px(16.0))
            .text_color(cx.theme().foreground)
            .bg(cx.theme().background)
            .child(TextView::new(&self.text).selectable(true))
    }
}

#[gpui::test]
fn reader_drag_selection_keeps_text_and_copy_after_theme_switch(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let mut fixture = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ReaderSelectionFixture {
            text: cx.new(|cx| TextViewState::markdown(SELECTED_TEXT, cx)),
            width: 480.0,
        });
        fixture = Some(view.clone());
        Root::new(view, window, cx)
    });
    let fixture = fixture.unwrap();

    for (name, width) in [(ThemeName::MintLight, 480.0), (ThemeName::Nord, 160.0)] {
        cx.update(|window, cx| {
            window.clear_text_selection(cx);
            apply_reader_theme(name, cx);
            fixture.update(cx, |view, cx| {
                view.width = width;
                cx.notify();
            });
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        let bounds = cx.debug_bounds("reader-selection-fixture").unwrap();
        let start = bounds.origin + point(px(1.0), px(10.0));
        let end = bounds.bottom_right() - point(px(2.0), px(2.0));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(window.selected_text(cx).trim(), SELECTED_TEXT, "{name:?}");
            assert!(cx.theme().selection.a <= 0.3, "{name:?}");
        });
        assert_eq!(cx.debug_bounds("reader-selection-fixture").unwrap(), bounds);
        let copy = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
        cx.simulate_keystrokes(copy);
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();
        assert_eq!(copied.trim(), SELECTED_TEXT, "{name:?}");
    }
}

/// Native visual check for the same TextView used by the answer reader. Input
/// events are dispatched to this test window, never to another desktop window.
/// The probe does not touch the OS clipboard or start a terminal/AI session.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires a Windows desktop and PEBREL_SELECTION_QA_DIR for screenshots"]
fn native_reader_selection_preview() {
    use gpui::{
        Bounds, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, WindowBounds,
        WindowOptions, size,
    };
    use std::{path::PathBuf, time::Duration};

    let output = PathBuf::from(
        std::env::var_os("PEBREL_SELECTION_QA_DIR").expect("set QA output directory"),
    );
    let name = std::env::var("PEBREL_SELECTION_QA_THEME").unwrap_or_else(|_| "MintLight".into());
    let name = ThemeName::from_prompt_name(&name).expect("a built-in theme name");
    let width: f32 = std::env::var("PEBREL_SELECTION_QA_WIDTH")
        .unwrap_or_else(|_| "480".into())
        .parse()
        .expect("numeric width");
    std::fs::create_dir_all(&output).unwrap();
    let ready = output.join("ready.json");
    assert!(!ready.exists(), "use a fresh QA directory");
    let ready_after_run = ready.clone();

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        apply_reader_theme(name, cx);
        let selection = cx.theme().selection;
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(40.0), px(60.0)),
                        size(px(680.0), px(260.0)),
                    ))),
                    focus: false,
                    show: true,
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| ReaderSelectionFixture {
                        text: cx.new(|cx| TextViewState::markdown(SELECTED_TEXT, cx)),
                        width,
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let selected = cx
                .update_window(window.into(), |_, window, cx| {
                    let start = point(px(1.0), px(10.0));
                    let end = point(px(width - 2.0), px(178.0));
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            position: start,
                            button: MouseButton::Left,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: end,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            position: end,
                            button: MouseButton::Left,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }),
                        cx,
                    );
                    let _ = window.draw(cx);
                    window.selected_text(cx)
                })
                .unwrap();
            assert_eq!(selected.trim(), SELECTED_TEXT);
            cx.background_executor().timer(Duration::from_millis(250)).await;
            std::fs::write(&ready, serde_json::to_vec(&serde_json::json!({
                "pid": std::process::id(), "theme": name.prompt_name(),
                "width": width, "selected_text": selected.trim(), "selection_alpha": selection.a,
            })).unwrap()).unwrap();
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
    assert!(ready_after_run.exists(), "native probe did not reach selected state");
}
