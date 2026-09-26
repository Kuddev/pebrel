//! Both sidebar edges use the same minimum-width detent and release-to-close
//! gesture. Pointer travel remains separate from the displayed panel width.

use super::*;

const DETENT_TRAVEL: f32 = 24.0;
const CLOSE_TRAVEL: f32 = 88.0;
const CLOSE_HYSTERESIS: f32 = 12.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct ResizeDrag {
    pub start_x: f32,
    pub start_width: f32,
    pub open_width: f32,
    pub close: bool,
}

impl ResizeDrag {
    pub fn new(x: f32, width: f32) -> Self {
        Self { start_x: x, start_width: width, open_width: width, close: false }
    }

    pub fn width(&mut self, raw: f32, minimum: f32, maximum: f32) -> f32 {
        let overshoot = (minimum - raw).max(0.0);
        self.close =
            overshoot >= if self.close { CLOSE_TRAVEL - CLOSE_HYSTERESIS } else { CLOSE_TRAVEL };
        if raw >= minimum {
            self.open_width = raw.clamp(minimum, maximum);
            return self.open_width;
        }
        // A real stop before a short, resisted movement gives the two actions
        // distinct pointer travel without snapping the sidebar shut mid-drag.
        minimum - ((overshoot - DETENT_TRAVEL).max(0.0) * 0.25).min(24.0)
    }
}

impl NebulaWorkspace {
    pub(super) fn subscribe_sidebar_resize_cancel(
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let owner_window = window.window_handle();
        let listener = cx.listener(move |view, event: &gpui::KeystrokeEvent, window, cx| {
            // Bound input actions run before element KeyDown handlers. A drag
            // must consume Escape first without taking the editor's focus.
            if window.window_handle() == owner_window
                && event.keystroke.key == "escape"
                && (view.cancel_details_panel_resize(cx) | view.cancel_left_sidebar_resize(cx))
            {
                cx.stop_propagation();
            }
        });
        cx.intercept_keystrokes(listener)
    }

    pub(super) fn render_left_sidebar_resize_handle(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self.sidebar_collapsed
            || self.settings_open
            || self.reader_focus_active(cx)
            || self.tabs_position == nebula_settings::TabsPositionName::Top
            || !crate::gpui_shell::config::panel_resize(cx)
        {
            return None;
        }
        Some(
            div()
                .id("sidebar-resize-handle")
                .debug_selector(|| "sidebar-resize-handle".to_owned())
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(self.sidebar_width + sidebar_resize_visual_offset(cx)
                    - SIDEBAR_RESIZE_HANDLE_WIDTH * 0.5))
                .w(px(SIDEBAR_RESIZE_HANDLE_WIDTH))
                .occlude()
                .cursor_col_resize()
                .group("left-sidebar-divider")
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(SIDEBAR_RESIZE_HANDLE_WIDTH * 0.5))
                        .w(px(1.0))
                        .bg(cx.theme().border)
                        .group_hover("left-sidebar-divider", |line| line.bg(cx.theme().ring)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, event: &gpui::MouseDownEvent, _, cx| {
                        gpui_component::GlobalState::suppress_text_selection(cx);
                        view.sidebar_resizing =
                            Some(ResizeDrag::new(f32::from(event.position.x), view.sidebar_width));
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn update_left_sidebar_resize(
        &mut self,
        event: &gpui::MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = &mut self.sidebar_resizing else { return };
        if event.pressed_button != Some(MouseButton::Left) || !window.is_window_active() {
            self.cancel_left_sidebar_resize(cx);
            return;
        }
        let raw = drag.start_width + f32::from(event.position.x) - drag.start_x;
        self.sidebar_width =
            drag.width(raw, nebula_settings::MIN_SIDEBAR_WIDTH, nebula_settings::MAX_SIDEBAR_WIDTH);
        cx.notify();
    }

    pub(super) fn finish_left_sidebar_resize(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.sidebar_resizing.take() else { return false };
        if drag.close {
            self.sidebar_closing_width = Some(self.sidebar_width);
        }
        self.sidebar_width = if drag.close { drag.start_width } else { drag.open_width };
        if drag.close {
            self.sidebar_collapsed = true;
            self.sidebar_fold_armed = true;
        } else if let Err(error) =
            nebula_settings::persist_keys(&[("sidebar_w", format!("{:.0}", self.sidebar_width))])
        {
            log::warn!("Could not persist sidebar width: {error}");
        }
        cx.notify();
        true
    }

    pub(super) fn cancel_left_sidebar_resize(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.sidebar_resizing.take() else { return false };
        self.sidebar_width = drag.start_width;
        cx.notify();
        true
    }

    pub(super) fn render_left_sidebar_resize_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.sidebar_resizing.map(|drag| {
            div()
                .absolute()
                .inset_0()
                .occlude()
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|view, event, window, cx| {
                    view.update_left_sidebar_resize(event, window, cx);
                    cx.stop_propagation();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|view, _, _, cx| {
                        view.finish_left_sidebar_resize(cx);
                    }),
                )
                .when(drag.close, |overlay| {
                    overlay.child(collapse_hint(true, self.sidebar_width, cx))
                })
                .into_any_element()
        })
    }
}

pub(super) fn collapse_hint(left: bool, width: f32, cx: &App) -> gpui::Div {
    div()
        .absolute()
        .top(px(64.0))
        .when(left, |hint| hint.left(px(width + 12.0)))
        .when(!left, |hint| hint.right(px(width + 12.0)))
        .px_3()
        .py_2()
        .rounded_md()
        .shadow_sm()
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .text_sm()
        .child(
            crate::gpui_shell::config::ui_language(cx)
                .text(crate::i18n::Message::EditorReleaseToCloseSidebar),
        )
}

#[cfg(test)]
mod tests {
    use super::ResizeDrag;

    #[test]
    fn minimum_detent_precedes_close_and_reversing_cancels_it() {
        let mut drag = ResizeDrag::new(500.0, 320.0);
        assert_eq!(drag.width(240.0, 240.0, 560.0), 240.0);
        assert_eq!(drag.width(222.0, 240.0, 560.0), 240.0);
        assert!(!drag.close);
        assert!(drag.width(145.0, 240.0, 560.0) > 210.0);
        assert!(drag.close);
        drag.width(200.0, 240.0, 560.0);
        assert!(!drag.close);
        assert_eq!(drag.start_width, 320.0);
        assert_eq!(drag.width(400.0, 240.0, 560.0), 400.0);
    }
}
