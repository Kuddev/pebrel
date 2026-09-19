use super::*;
use gpui::{TestAppContext, VisualTestContext};

struct EditorProbe {
    input: Entity<InputState>,
}

impl Render for EditorProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(420.0))
            .debug_selector(|| "saved-command-editor-input".into())
            .child(command_editor_input(&self.input, cx))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn command_editor_retains_height_and_keeps_typing_visible(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let mut input = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let state = cx.new(|cx| InputState::new(window, cx).multi_line(true).soft_wrap(true));
        state.update(cx, |state, cx| state.focus(window, cx));
        input = Some(state.clone());
        let probe = cx.new(|_| EditorProbe { input: state });
        Root::new(probe, window, cx)
    });
    let input = input.unwrap();
    draw(cx);
    let initial = cx.debug_bounds("saved-command-editor-input").unwrap();
    assert_eq!(initial.size.height, px(COMMAND_INPUT_HEIGHT));
    for _ in 0..120 {
        cx.simulate_input("d");
        draw(cx);
        assert_eq!(cx.debug_bounds("saved-command-editor-input").unwrap(), initial);
        input.read_with(cx, |state, _| {
            assert_eq!(state.cursor(), state.value().len());
            assert_eq!(
                state.scroll_offset().y,
                px(0.0),
                "a few wrapped lines fit without vertical jumping"
            );
        });
    }
    cx.simulate_input("\n第一行\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行\n第八行");
    draw(cx);
    input.read_with(cx, |state, _| {
        assert!(state.value().contains("\n第一行\n第二行"));
        assert!(state.scroll_offset().y < px(0.0));
        assert_eq!(state.cursor(), state.value().len());
    });
    assert_eq!(cx.debug_bounds("saved-command-editor-input").unwrap(), initial);
}
