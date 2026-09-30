use std::{cell::Cell, rc::Rc};

use gpui::{AppContext as _, Context, Modifiers, Render, TestAppContext, point};
use gpui_component::{IconName, Root, Theme, h_flex};

use super::*;

struct ControlProbe(Rc<Cell<usize>>);

impl Render for ControlProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let action = self.0.clone();
        let tool = self.0.clone();
        let disabled = self.0.clone();
        h_flex()
            .gap(px(8.0))
            .child(
                NebulaButton::new("comfort-action")
                    .label("查看详情 / Details")
                    .on_click(move |_, _, _| action.set(action.get() + 1)),
            )
            .child(
                toolbar_button("comfort-tool", IconName::Settings)
                    .debug_selector(|| "comfort-tool".to_owned())
                    .on_click(move |_, _, _| tool.set(tool.get() + 1)),
            )
            .child(
                NebulaButton::new("comfort-disabled")
                    .label("Disabled")
                    .disabled(true)
                    .on_click(move |_, _, _| disabled.set(disabled.get() + 1)),
            )
    }
}

#[gpui::test]
fn desktop_controls_keep_padding_clickable_with_small_ui_fonts(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let clicks = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| ControlProbe(clicks.clone()));
        Root::new(view, window, cx)
    });
    cx.simulate_resize(gpui::size(px(1000.0), px(200.0)));
    for (font, height) in [(10.0, 32.0), (14.0, 32.0), (24.0, 48.0)] {
        cx.update(|window, cx| {
            Theme::global_mut(cx).font_size = px(font);
            window.refresh();
            let _ = window.draw(cx);
        });
        let action = cx.debug_bounds("nebula-btn-comfort-action").expect("text button");
        let tool = cx.debug_bounds("comfort-tool").expect("toolbar button");
        let disabled = cx.debug_bounds("nebula-btn-comfort-disabled").expect("disabled button");
        assert!(action.size.height >= px(height));
        assert_eq!(tool.size, gpui::size(px(32.0), px(32.0)));
        let before = clicks.get();
        // These corners are padding, not glyphs: the whole surface must activate.
        for bounds in [action, tool, disabled] {
            cx.simulate_click(
                point(bounds.origin.x + px(2.0), bounds.bottom() - px(2.0)),
                Modifiers::default(),
            );
        }
        assert_eq!(clicks.get(), before + 2, "disabled padding must not activate");
    }
}
