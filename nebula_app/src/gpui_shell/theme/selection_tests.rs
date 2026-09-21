use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    MouseButton, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, Window, div, point, px,
};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Root, Theme, ThemeMode, WindowExt as _,
    text::{TextView, TextViewState},
};
use nebula_settings::ThemeName;

use super::{ResolvedTheme, apply_skin_tokens, wash};

fn apply_reader_theme(name: ThemeName, cx: &mut App) {
    let chrome = ResolvedTheme::builtin(name, None);
    let mode = if chrome.skin().is_light { ThemeMode::Light } else { ThemeMode::Dark };
    Theme::change(mode, None, cx);
    apply_skin_tokens(&chrome, cx);
    // Match the product shell at full opacity without loading wallpaper,
    // preferences, or tray integrations into the isolated test process.
    let background = super::shell_color(chrome.chrome_palette());
    let theme = Theme::global_mut(cx);
    theme.background = background;
    theme.tokens.background = background.into();
}

#[gpui::test]
fn text_selection_and_search_matches_stay_distinct_from_the_document(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        for name in ThemeName::BUILTIN {
            let chrome = ResolvedTheme::builtin(name, None);
            let original = wash(chrome.skin().accent_soft);
            apply_reader_theme(name, cx);

            let theme = cx.theme();
            let selection = theme.selection;
            assert!(selection.a > 0.0 && selection.a <= 0.3, "{name:?}");
            let document = theme.highlight_theme.style.editor_background.unwrap();
            let active_line = theme.highlight_theme.style.editor_active_line.unwrap();
            assert!(active_line.a <= 0.08, "the caret row must not hide the wallpaper");
            assert!(theme.highlight_theme.style.editor_gutter_background.is_none());
            // Input paints inactive matches after desaturating the selection;
            // active matches receive a second layer. Check the visible result
            // both on the page and on the caret row, including light themes.
            for base in [document, document.blend(active_line), theme.background] {
                let selected = base.blend(selection);
                let matched = base.blend(selection.saturation(0.1));
                for overlay in [selected, matched] {
                    assert!(contrast(base, overlay) >= 1.2, "{name:?}: highlight blends into page");
                }
                assert!(contrast(matched, matched.blend(selection)) > 1.05, "{name:?}");
            }
            assert_eq!(theme.tokens.selection.color, selection, "{name:?}");
            assert_eq!(theme.tokens.selection.background, selection.into(), "{name:?}");
            // Solid selection surfaces for lists are not text overlays.
            assert_eq!(theme.list_active, original, "{name:?}");
        }
    });
}

fn contrast(left: gpui::Hsla, right: gpui::Hsla) -> f64 {
    let rgb = |color: gpui::Hsla| {
        let color: gpui::Rgba = color.into();
        crate::display::color::Rgb::new(
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8,
        )
    };
    rgb(left).contrast(*rgb(right))
}

#[gpui::test]
fn editor_surfaces_follow_a_custom_document_palette(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        for name in [ThemeName::MintLight, ThemeName::Nord] {
            let mut definition = nebula_settings::ThemeDefinition::from_builtin(name);
            definition.terminal.background =
                if name == ThemeName::Nord { [24, 32, 45] } else { [245, 248, 242] };
            let expected = definition.terminal.background;
            let chrome = ResolvedTheme::custom(definition, None, None);
            Theme::change(
                if chrome.is_light() { ThemeMode::Light } else { ThemeMode::Dark },
                None,
                cx,
            );
            apply_skin_tokens(&chrome, cx);
            let style = &cx.theme().highlight_theme.style;
            assert_eq!(
                style.editor_background,
                Some(super::to_hsla(expected[0], expected[1], expected[2]))
            );
            assert!(style.editor_gutter_background.is_none());
            let background = style.editor_background.unwrap();
            assert!(contrast(background, background.blend(cx.theme().selection)) >= 1.2);
        }
    });
}

const SELECTED_TEXT: &str = "alpha beta gamma delta";

#[gpui::test]
fn source_editor_keeps_find_and_copy_usable_when_the_theme_changes(cx: &mut TestAppContext) {
    use crate::gpui_shell::file_editor::TextFileView;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.txt");
    let source = "alpha beta\nalpha gamma\n";
    std::fs::write(&path, source).unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::math_view::register(cx);
        apply_reader_theme(ThemeName::MintLight, cx);
    });
    let mut file = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| TextFileView::new(path, window, cx));
        file = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let content = cx.debug_bounds("file-source-content").unwrap();
    cx.simulate_click(content.origin + point(px(100.0), px(20.0)), Modifiers::default());
    let modifier = if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{modifier}-f"));
    cx.simulate_input("alpha");
    cx.run_until_parked();
    cx.update(|window, cx| {
        apply_reader_theme(ThemeName::Nord, cx);
        window.refresh();
        let _ = window.draw(cx);
    });
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    cx.simulate_click(content.origin + point(px(100.0), px(20.0)), Modifiers::default());
    cx.simulate_keystrokes(&format!("{modifier}-a {modifier}-c"));
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);
    assert!(!file.unwrap().read_with(cx, |view, _| view.is_dirty()));
}

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
        let copy = if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
            "cmd-c"
        } else {
            "ctrl-c"
        };
        cx.simulate_keystrokes(copy);
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();
        assert_eq!(copied.trim(), SELECTED_TEXT, "{name:?}");
    }
}

/// Native visual check for the same TextView used by the answer reader. Input
/// events are dispatched to this test window, never to another desktop window.
/// The probe does not touch the OS clipboard or start a terminal/AI session.
#[test]
#[ignore = "requires a Windows desktop and PEBREL_SELECTION_QA_DIR for screenshots"]
fn native_reader_selection_preview() {
    assert_eq!(
        crate::platform::Platform::current(),
        crate::platform::Platform::Windows,
        "this native desktop probe requires Windows",
    );
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
