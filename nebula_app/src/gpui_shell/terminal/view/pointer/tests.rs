use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui::{Entity, Focusable as _, Modifiers, Render, TestAppContext, VisualTestContext};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use std::cell::Cell;
use std::rc::Rc;

struct Probe {
    terminal: Entity<TerminalView>,
    input: Entity<InputState>,
    overlay: bool,
    requests: Rc<Cell<usize>>,
    _subscription: gpui::Subscription,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(div().h(px(40.0)).child(Input::new(&self.input)))
            .child(
                div()
                    .id("mouse-terminal")
                    .debug_selector(|| "mouse-terminal".to_owned())
                    .flex_1()
                    .child(self.terminal.clone()),
            )
            .when(self.overlay, |root| root.child(div().absolute().inset_0().occlude()))
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<Probe>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
        settings.focus_follows_mouse = false;
        cx.set_global(settings);
    });
    let mut output = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let terminal = cx.new(|cx| {
            TerminalView::new(
                1,
                (80, 24),
                super::super::TerminalLaunch::Local {
                    cwd: None,
                    // Exercise the real terminal element without creating a shell session.
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        Vec::new(),
                    )),
                    shell_name: None,
                },
                window,
                cx,
            )
        });
        let input = cx.new(|cx| InputState::new(window, cx));
        input.read(cx).focus_handle(cx).focus(window, cx);
        let view = cx.new(|cx| {
            let requests = Rc::new(Cell::new(0));
            let count = requests.clone();
            let subscription = cx.subscribe(&terminal, move |_, _, event, _| {
                if matches!(event, TerminalViewEvent::FocusRequested) {
                    count.set(count.get() + 1);
                }
            });
            Probe { terminal, input, overlay: false, requests, _subscription: subscription }
        });
        output = Some(view.clone());
        Root::new(view, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(700.0), px(500.0)));
    // TestPlatform creates inactive windows; real pointer focus requires the
    // OS activation event as well as an element focus handle.
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    window.update(|window, _| assert!(window.is_window_active()));
    (output.unwrap(), window)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn move_over(cx: &mut VisualTestContext, button: Option<MouseButton>) {
    let position = cx.debug_bounds("mouse-terminal").unwrap().center();
    cx.simulate_mouse_move(position, button, Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn hover_focus_requires_opt_in_and_emits_once(cx: &mut TestAppContext) {
    let (probe, mut cx) = open(cx);
    move_over(&mut cx, None);
    assert_eq!(probe.read_with(&cx, |probe, _| probe.requests.get()), 0);
    cx.update(|_, cx| cx.global_mut::<Settings>().focus_follows_mouse = true);
    move_over(&mut cx, None);
    cx.update(|window, cx| {
        assert!(probe.read(cx).terminal.read(cx).focus_handle.is_focused(window))
    });
    move_over(&mut cx, None);
    assert_eq!(probe.read_with(&cx, |probe, _| probe.requests.get()), 1);
    cx.update(|window, cx| {
        cx.global_mut::<Settings>().focus_follows_mouse = false;
        probe.read(cx).input.read(cx).focus_handle(cx).focus(window, cx);
    });
    move_over(&mut cx, None);
    assert_eq!(probe.read_with(&cx, |probe, _| probe.requests.get()), 1);
}

#[gpui::test]
fn hover_does_not_steal_focus_during_drag_overlay_dialog_or_inactive_window(
    cx: &mut TestAppContext,
) {
    let (probe, mut cx) = open(cx);
    cx.update(|_, cx| cx.global_mut::<Settings>().focus_follows_mouse = true);
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        move_over(&mut cx, Some(button));
    }
    probe.update(&mut cx, |probe, cx| {
        probe.overlay = true;
        cx.notify();
    });
    draw(&mut cx);
    move_over(&mut cx, None);
    probe.update(&mut cx, |probe, cx| {
        probe.overlay = false;
        cx.notify();
    });
    draw(&mut cx);
    cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog.title("focus fixture")));
    draw(&mut cx);
    move_over(&mut cx, None);
    cx.update(|window, cx| window.close_dialog(cx));
    draw(&mut cx);
    cx.deactivate_window();
    cx.update(|window, _| assert!(!window.is_window_active()));
    move_over(&mut cx, None);
    assert_eq!(probe.read_with(&cx, |probe, _| probe.requests.get()), 0);
}

fn link_modifiers() -> Modifiers {
    match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => Modifiers { platform: true, ..Modifiers::default() },
        _ => Modifiers { control: true, ..Modifiers::default() },
    }
}

fn link_fixture(
    cx: &mut TestAppContext,
    text: &[u8],
) -> (Entity<TerminalView>, VisualTestContext, std::sync::mpsc::Receiver<Msg>) {
    cx.update(crate::gpui_shell::math_view::register);
    let (probe, mut cx) = open(cx);
    let terminal = probe.read_with(&cx, |probe, _| probe.terminal.clone());
    let receiver = terminal.update(&mut cx, |view, cx| {
        let (session, receiver) = session::test_session();
        view.session = Some(session);
        view.error = None;
        view.exited = None;
        view.copy_on_select = false;
        // Use the production matcher and dispatcher with a deterministic action;
        // tests must not launch a user's browser or editor.
        let mut config = UiConfig::default();
        Arc::make_mut(&mut config.hints.enabled[0]).action =
            crate::config::ui_config::HintAction::Action(
                crate::config::ui_config::HintInternalAction::Copy,
            );
        view.hint_config = Arc::new(config);
        super::super::startup_tests::feed(view, text);
        cx.notify();
        receiver
    });
    draw(&mut cx);
    cx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("before".into())));
    (terminal, cx, receiver)
}

fn cell(view: &Entity<TerminalView>, cx: &VisualTestContext, col: usize) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        point(
            view.origin.x + view.cell_width * (col as f32 + 0.5),
            view.origin.y + view.line_height * 0.5,
        )
    })
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

#[gpui::test]
fn link_gesture_opens_regex_files_and_osc8_with_or_without_mouse_reporting(
    cx: &mut TestAppContext,
) {
    for (output, target) in [
        ("https://example.com", "https://example.com"),
        ("file:///tmp/pebrel-notes.md", "file:///tmp/pebrel-notes.md"),
        ("[notes](./notes.md)", "[notes](./notes.md)"),
        ("\x1b]8;;https://example.com/osc8\x1b\\site\x1b]8;;\x1b\\", "https://example.com/osc8"),
    ] {
        for mouse_mode in [false, true] {
            let output = if mouse_mode {
                format!("\x1b[?1003h\x1b[?1006h{output}")
            } else {
                output.to_owned()
            };
            let (view, mut window, receiver) = link_fixture(cx, output.as_bytes());
            let position = cell(&view, &window, 1);
            window.simulate_mouse_move(position, None, link_modifiers());
            assert!(view.read_with(&window, |view, _| view.link_hover.is_some()));
            window.simulate_mouse_down(position, MouseButton::Left, link_modifiers());
            assert_eq!(clipboard(&mut window).as_deref(), Some("before"));
            assert!(view.read_with(&window, |view, _| view.pending_link_open));
            // Releasing Command/Ctrl first must not send an orphan mouse-up to the TUI.
            window.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
            assert_eq!(clipboard(&mut window).as_deref(), Some(target));
            assert!(!receiver.try_iter().any(|event| matches!(event, Msg::Input(_))));
        }
    }
}

#[gpui::test]
fn plain_click_and_wrong_modifier_do_not_open_links(cx: &mut TestAppContext) {
    let (view, mut window, _) = link_fixture(cx, b"https://example.com");
    let position = cell(&view, &window, 1);
    let wrong = if link_modifiers().platform {
        Modifiers { control: true, ..Modifiers::default() }
    } else {
        Modifiers { platform: true, ..Modifiers::default() }
    };
    for modifiers in [Modifiers::default(), wrong] {
        window.simulate_mouse_move(position, None, modifiers);
        window.simulate_mouse_down(position, MouseButton::Left, modifiers);
        window.simulate_mouse_up(position, MouseButton::Left, modifiers);
        assert_eq!(clipboard(&mut window).as_deref(), Some("before"));
    }
}

#[gpui::test]
fn link_drag_cannot_retarget_or_leave_a_pending_open(cx: &mut TestAppContext) {
    let (view, mut window, _) = link_fixture(cx, b"https://one.test https://two.test");
    let start = cell(&view, &window, 1);
    for end in [cell(&view, &window, 20), point(px(10.0), px(10.0))] {
        window.simulate_mouse_down(start, MouseButton::Left, link_modifiers());
        window.simulate_mouse_move(end, Some(MouseButton::Left), link_modifiers());
        window.simulate_mouse_up(end, MouseButton::Left, link_modifiers());
        assert_eq!(clipboard(&mut window).as_deref(), Some("before"));
        assert!(!view.read_with(&window, |view, _| view.pending_link_open));
    }
}

#[gpui::test]
fn mouse_reporting_still_receives_ordinary_and_non_link_clicks(cx: &mut TestAppContext) {
    let (view, mut window, receiver) =
        link_fixture(cx, b"\x1b[?1000h\x1b[?1006hhttps://example.com plain");
    for (col, modifiers) in [(1, Modifiers::default()), (22, link_modifiers())] {
        let position = cell(&view, &window, col);
        window.simulate_mouse_down(position, MouseButton::Left, modifiers);
        window.simulate_mouse_up(position, MouseButton::Left, modifiers);
        let reports: Vec<_> = receiver
            .try_iter()
            .filter_map(|event| match event {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .collect();
        assert_eq!(reports.len(), 2);
        assert!(reports[0].ends_with(b"M"));
        assert!(reports[1].ends_with(b"m"));
        assert_eq!(clipboard(&mut window).as_deref(), Some("before"));
    }
}

#[gpui::test]
fn link_hover_uses_current_language_and_platform(cx: &mut TestAppContext) {
    use crate::i18n::UiLanguage;
    let (view, mut window, _) = link_fixture(cx, b"https://example.com");
    let position = cell(&view, &window, 1);
    let modifier = if link_modifiers().platform { "Command" } else { "Ctrl" };
    for (language, suffix) in [
        (UiLanguage::ZhCn, "+左键跳转"),
        (UiLanguage::EnUs, "+left click to open"),
        (UiLanguage::KoKr, "+왼쪽 클릭으로 열기"),
    ] {
        window.update(|_, cx| cx.global_mut::<Settings>().ui_language = language);
        window.simulate_mouse_move(position, None, Modifiers::default());
        let preview =
            view.read_with(&window, |view, _| view.link_hover.as_ref().unwrap().preview.clone());
        assert!(preview.ends_with(&format!(" · {modifier}{suffix}")), "{preview}");
        draw(&mut window);
    }
}

#[gpui::test]
fn explicit_link_gesture_respects_disabled_hints_and_required_modifiers(cx: &mut TestAppContext) {
    let (view, mut window, _) = link_fixture(cx, b"\x1b[?1000hhttps://example.com");
    let position = cell(&view, &window, 1);
    for enabled in [false, true] {
        view.update(&mut window, |view, _| {
            let config = Arc::make_mut(&mut view.hint_config);
            let hint = Arc::make_mut(&mut config.hints.enabled[0]);
            let mouse = hint.mouse.as_mut().unwrap();
            mouse.enabled = enabled;
            mouse.mods.0 = winit::keyboard::ModifiersState::SHIFT;
        });
        window.simulate_mouse_down(position, MouseButton::Left, link_modifiers());
        window.simulate_mouse_up(position, MouseButton::Left, link_modifiers());
        assert_eq!(clipboard(&mut window).as_deref(), Some("before"));
    }
    let modifiers = Modifiers { shift: true, ..link_modifiers() };
    window.simulate_mouse_down(position, MouseButton::Left, modifiers);
    window.simulate_mouse_up(position, MouseButton::Left, modifiers);
    assert_eq!(clipboard(&mut window).as_deref(), Some("https://example.com"));
}
