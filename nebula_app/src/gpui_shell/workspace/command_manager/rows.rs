//! Visible command rows, shared by folder contents and global search.
use super::*;
use gpui::AnyElement;

impl NebulaWorkspace {
    pub(super) fn render_saved_command_row(
        &self,
        index: usize,
        command: crate::saved_commands::SavedCommand,
        list_scrollable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::display::ui::tokens::{radius, space};
        let theme = cx.theme();
        let selected_bg = theme.list_active;
        let hover_bg = theme.list_hover;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let border = theme.border;
        let accent = theme.primary;
        let mono_family = theme.mono_font_family.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let selected = index == self.command_manager_selected;
        let builtin = command.id.starts_with("builtin:");
        let hover_group = SharedString::from(format!("saved-command-row-hover-{index}"));
        let preview = command
            .command
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let mode_label = if builtin {
            language.text(crate::i18n::Message::CommandsBuiltinLabel)
        } else if command.append_enter {
            language.pick("运行", "Run")
        } else {
            language.pick("插入", "Insert")
        };
        let run_tooltip = if command.append_enter {
            language.pick("运行命令", "Run command")
        } else {
            language.pick("插入到当前终端", "Insert into current terminal")
        };
        let run_icon = command_run_icon(builtin, command.append_enter);
        let drag = CommandDrag::new(&command);
        let menu_id = command.id.clone();
        let menu_button_id = command.id.clone();
        let run_command = command.clone();
        let row_command = command.clone();
        let copy_command = command.clone();
        let edit_id = command.id.clone();
        let delete_id = command.id.clone();

        h_flex()
            .id(SharedString::from(format!("saved-command-row-{index}")))
            .debug_selector(move || format!("saved-command-row-{index}").into())
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_command_group_menu(menu_id.clone(), event.position, window, cx);
                }),
            )
            .group(hover_group.clone())
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .items_center()
            .gap(px(space::XS))
            .px_2()
            .when(list_scrollable, |row| row.pr(px(18.0)))
            .rounded(px(radius::CONTROL))
            .cursor_pointer()
            .when(selected, |row| row.bg(selected_bg))
            .when(!selected, |row| row.group_hover(hover_group.clone(), |row| row.bg(hover_bg)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_manager_selected = index;
                this.dispatch_saved_command(row_command.clone(), window, cx);
            }))
            .child(
                Button::new(SharedString::from(format!("saved-command-run-{index}")))
                    .debug_selector(move || format!("saved-command-run-{index}"))
                    .icon(Icon::new(run_icon).size(px(ROW_ICON_SIZE)))
                    .ghost()
                    .xsmall()
                    .size(px(ROW_ICON_SLOT))
                    .flex_shrink_0()
                    .tooltip(run_tooltip)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.command_manager_selected = index;
                        this.dispatch_saved_command(run_command.clone(), window, cx);
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(space::XXS))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .debug_selector(move || format!("saved-command-label-{index}"))
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(foreground)
                                    .child(command.name),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .rounded(px(radius::CHIP))
                                    .border_1()
                                    .border_color(border)
                                    .px_1()
                                    .text_size(px(10.0))
                                    .text_color(if selected { accent } else { muted })
                                    .child(mode_label),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .font_family(mono_family.clone())
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child(preview),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!(
                            "saved-command-group-menu-{index}"
                        )))
                        .icon(IconName::EllipsisVertical)
                        .ghost()
                        .xsmall()
                        .tooltip(language.text(crate::i18n::Message::CommandsMoveToGroup))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_command_group_menu(
                                    menu_button_id.clone(),
                                    window.mouse_position(),
                                    window,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        Button::new(SharedString::from(format!("saved-command-copy-{index}")))
                            .icon(IconName::Copy)
                            .ghost()
                            .xsmall()
                            .tooltip(language.pick("复制命令", "Copy command"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.copy_saved_command(&copy_command, window, cx);
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("saved-command-edit-{index}")))
                            .icon(custom_icon(crate::gpui_shell::assets::nav::PENCIL))
                            .ghost()
                            .xsmall()
                            .tooltip(if builtin {
                                language.text(crate::i18n::Message::CommandsSaveCopy)
                            } else {
                                language.pick("编辑命令", "Edit command")
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_saved_command_editor(Some(edit_id.clone()), window, cx);
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("saved-command-delete-{index}")))
                            .icon(custom_icon(crate::gpui_shell::assets::nav::TRASH))
                            .debug_selector(move || format!("saved-command-delete-{index}"))
                            .ghost()
                            .xsmall()
                            .tooltip(language.pick("删除命令", "Delete command"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_delete_saved_command_dialog(
                                    delete_id.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }
}
