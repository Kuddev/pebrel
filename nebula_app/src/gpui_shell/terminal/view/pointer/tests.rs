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
