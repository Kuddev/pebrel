use super::*;

/// Prepaint records the actual pane column before any titlebar paint runs. Keep
/// this cell for the workspace lifetime, so resizing and panel animations do not
/// allocate a new shared slot or duplicate the body's layout calculations.
#[derive(Clone, Default)]
pub(super) struct TitleBarBackground(Rc<std::cell::Cell<Option<Bounds<Pixels>>>>);

impl TitleBarBackground {
    pub(super) fn record_pane(&self, bounds: Bounds<Pixels>) {
        self.0.set(Some(bounds));
    }

    fn element(&self) -> gpui::Canvas<()> {
        let pane_bounds = self.0.clone();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let card = crate::gpui_shell::theme::PaneCardStyle::current(cx);
                let shell = cx.theme().background;
                let Some(pane) = pane_bounds.get().filter(|_| card.radius == 0.0) else {
                    window.paint_quad(fill(bounds, shell));
                    return;
                };
                let title_left = f32::from(bounds.origin.x);
                let title_right = title_left + f32::from(bounds.size.width);
                let pane_left = f32::from(pane.origin.x).clamp(title_left, title_right);
                let pane_right = f32::from(pane.right()).clamp(pane_left, title_right);
                let content = crate::gpui_shell::theme::card_content_bg(cx);

                // Adjacent bands receive alpha once. The actual pane bounds also
                // account for a hidden sidebar and an open file tree.
                for (left, right, color) in [
                    (title_left, pane_left, shell),
                    (pane_left, pane_right, content),
                    (pane_right, title_right, shell),
                ] {
                    if right > left {
                        window.paint_quad(fill(
                            Bounds::new(
                                gpui::point(px(left), bounds.origin.y),
                                size(px(right - left), bounds.size.height),
                            ),
                            color,
                        ));
                    }
                }
            },
        )
        .absolute()
        .inset_0()
    }
}

/// Paint tab and file-tree seams after the terminal, including the titlebar span.
/// Both use the same pixel snapping and theme color; the right seam stays inside
/// the terminal edge so the subsequently painted drawer cannot cover it.
pub(super) fn paint_pane_dividers(
    bounds: Bounds<Pixels>,
    file_tree: bool,
    window: &mut Window,
    cx: &App,
) {
    let card = crate::gpui_shell::theme::PaneCardStyle::current(cx);
    let color = crate::gpui_shell::theme::card_divider_color(cx);
    if let Some(line) = pane_card_divider_bounds(bounds, card.divider, window.scale_factor()) {
        window.paint_quad(fill(line, color));
    }
    if file_tree {
        let mut edge = bounds;
        edge.origin.x += edge.size.width;
        if let Some(mut line) = pane_card_divider_bounds(edge, card.divider, window.scale_factor())
        {
            line.origin.x -= line.size.width;
            window.paint_quad(fill(line, color));
        }
    }
}

fn title_bar_frame() -> gpui::Div {
    div().relative().flex_shrink_0().on_mouse_down(MouseButton::Left, |_, _, cx| {
        // Window dragging owns this press; it must not anchor the document selection.
        gpui_component::global_state::GlobalState::suppress_text_selection(cx);
    })
}

impl NebulaWorkspace {
    pub(super) fn render_window_title_bar(
        &self,
        settings_active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let top_tabs = self.tabs_position == nebula_settings::TabsPositionName::Top;
        let native_layout = crate::platform::window_chrome::layout(window);
        let bar = TitleBar::new()
            // Leave 8px above and below the existing 32px controls.
            .h(px(48.0))
            .when(!settings_active, |bar| bar.bg(gpui::transparent_black()).border_b_0())
            .when(settings_active, |bar| {
                bar.border_b_1().border_color(crate::gpui_shell::theme::settings_hairline(cx))
            })
            .when(top_tabs && native_layout.is_none(), |bar| {
                bar.pl(px(top_tabs::TOP_TAB_LEFT_INSET))
            })
            .when(top_tabs, |bar| {
                bar.child(self.render_top_title_bar(
                    settings_active,
                    window,
                    cx,
                ))
            })
            .when(!top_tabs, |bar| {
                bar.child(self.render_sidebar_title_bar(
                    settings_active,
                    cx,
                ))
            });

        // AppKit owns both the control group and its geometry. Read the live
        // frames so system layout, resize and full-screen transitions agree.
        let bar = if let Some((height, inset)) = native_layout {
            bar.h(px(height)).pl(px(inset))
        } else {
            bar
        };

        title_bar_frame()
            .when(!settings_active, |title| title.child(self.titlebar_background.element()))
            .child(bar)
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, point};
    use gpui_component::{
        Root, WindowExt,
        text::{TextView, TextViewState},
    };

    struct DocumentWindow(Entity<TextViewState>);

    impl Render for DocumentWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            v_flex().size_full().child(title_bar_frame().child(TitleBar::new().h(px(48.0)))).child(
                div()
                    .debug_selector(|| "titlebar-selection-document".to_owned())
                    .child(TextView::new(&self.0).selectable(true)),
            )
        }
    }

    #[gpui::test]
    fn title_bar_drag_does_not_anchor_document_selection(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let text = cx.new(|cx| TextViewState::markdown("Document text remains selectable", cx));
            let view = cx.new(|_| DocumentWindow(text));
            Root::new(view, window, cx)
        });
        let text = cx.debug_bounds("titlebar-selection-document").unwrap();
        cx.simulate_mouse_down(point(px(140.0), px(24.0)), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(text.center(), Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_move(text.center(), None, Modifiers::default());
        assert!(cx.update(|window, cx| window.selected_text(cx).is_empty()));

        // The same reader must still support ordinary text dragging afterwards.
        let start = point(text.left() + px(1.0), text.top() + px(10.0));
        let end = point(start.x + px(80.0), start.y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        assert!(!cx.update(|window, cx| window.selected_text(cx).is_empty()));
    }
}
