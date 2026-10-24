//! Browse-only order controls; the store owns membership and neighbor selection.

use super::*;
use crate::i18n::Message;
use crate::saved_commands::{BUILTIN_GROUP_ID, MoveDirection};
use gpui::{AnyElement, FocusHandle};

#[derive(Clone)]
pub(super) enum OrderTarget {
    Command(String),
    Group(String),
}

impl NebulaWorkspace {
    fn command_order_available(&self, cx: &App) -> bool {
        !self.command_manager_selection.selecting
            && self.command_manager_input.read(cx).value().trim().is_empty()
    }

    pub(super) fn render_command_order_buttons(
        &self,
        target: OrderTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.command_order_available(cx) {
            return None;
        }
        let (kind, id) = match &target {
            OrderTarget::Command(id) if !id.starts_with("builtin:") => ("command", id),
            OrderTarget::Group(id) if id != BUILTIN_GROUP_ID => ("group", id),
            _ => return None,
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let focus = window
            .use_keyed_state(SharedString::from(format!("{kind}-order-focus-{id}")), cx, |_, cx| {
                cx.focus_handle().tab_stop(false)
            })
            .read(cx)
            .clone();
        let focus_bg = cx.theme().list_hover;
        let mut controls = h_flex()
            .id(SharedString::from(format!("{kind}-order-controls-{id}")))
            .track_focus(&focus)
            .flex_shrink_0()
            .gap_1()
            .rounded_md()
            .focus_visible(move |controls| controls.bg(focus_bg))
            .on_key_down(|event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                }
            });
        for (direction, name, icon, message) in [
            (MoveDirection::Up, "up", IconName::ArrowUp, Message::CommandsMoveUp),
            (MoveDirection::Down, "down", IconName::ArrowDown, Message::CommandsMoveDown),
        ] {
            let enabled = match &target {
                OrderTarget::Command(id) => self.saved_commands.can_reorder_command(id, direction),
                OrderTarget::Group(id) => self.saved_commands.can_reorder_group(id, direction),
            };
            let selector = format!("{kind}-move-{name}-{id}");
            let keyboard_target = target.clone();
            let click_target = target.clone();
            let keyboard_focus = focus.clone();
            let click_focus = focus.clone();
            controls = controls.child(
                Button::new(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .icon(Icon::new(icon).size(px(ROW_ICON_SIZE)))
                    .ghost()
                    .xsmall()
                    .size(px(32.0))
                    .aria_label(language.text(message))
                    .tooltip(language.text(message))
                    .disabled(!enabled)
                    .when(enabled, |control| {
                        control.on_key_down(cx.listener(
                            move |this, event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.stop_propagation();
                                    this.reorder_command_target(
                                        &keyboard_target,
                                        direction,
                                        &keyboard_focus,
                                        window,
                                        cx,
                                    );
                                }
                            },
                        ))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.reorder_command_target(
                            &click_target,
                            direction,
                            &click_focus,
                            window,
                            cx,
                        );
                    })),
            );
        }
        Some(controls.into_any_element())
    }

    fn reorder_command_target(
        &mut self,
        target: &OrderTarget,
        direction: MoveDirection,
        focus: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.command_order_available(cx) {
            return;
        }
        let ordering_focused = focus.contains_focused(window, cx);
        let result = match target {
            OrderTarget::Command(id) => self.saved_commands.reorder_command(id, direction),
            OrderTarget::Group(id) => self.saved_commands.reorder_group(id, direction),
        };
        if let Err(error) = result {
            let language = crate::gpui_shell::config::ui_language(cx);
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                language.format(Message::CommandsOrderSaveFailed, &[("error", &error.to_string())]),
            );
            return;
        }
        let rows = self.command_manager_rows(cx);
        let selected = rows.iter().position(|row| match (target, row) {
            (OrderTarget::Command(id), ManagerRow::Command(command)) => id == &command.id,
            (OrderTarget::Group(id), ManagerRow::Folder { id: group, .. }) => id == group,
            _ => false,
        });
        if let Some(index) = selected {
            self.command_manager_selection.cursor = index;
            self.command_manager_scroll.scroll_to_item(index, gpui::ScrollStrategy::Nearest);
        } else {
            // The moved row may now be outside this folder after a concurrent edit.
            // No selection keeps a repeated Enter from dispatching an unrelated row.
            self.command_manager_selection.cursor = rows.len();
        }
        let can_continue = match target {
            OrderTarget::Command(id) => self.saved_commands.can_reorder_command(id, direction),
            OrderTarget::Group(id) => self.saved_commands.can_reorder_group(id, direction),
        };
        if ordering_focused && selected.is_none() {
            self.command_manager_input.update(cx, |input, cx| input.focus(window, cx));
        } else if ordering_focused && !can_continue {
            // Disabled buttons leave the focus tree. Keep focus in their control
            // group: Escape/Tab still work and repeated Enter/Space cannot run a row.
            focus.focus(window, cx);
        }
        cx.notify();
    }
}
