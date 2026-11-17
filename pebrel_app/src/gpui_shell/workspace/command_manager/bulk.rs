//! Selection and explicit deletion scopes for the command manager.

use super::*;
use crate::i18n::Message;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

#[derive(Default)]
pub(in crate::gpui_shell::workspace) struct CommandSelection {
    pub cursor: usize,
    pub selecting: bool,
    pub ids: std::collections::HashSet<String>,
}

#[derive(Clone)]
pub(super) enum Deletion {
    Commands(Vec<String>),
    All,
    Builtins,
    Folder(String),
}

impl NebulaWorkspace {
    pub(super) fn toggle_command_selection(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.command_manager_selection.ids.remove(id) {
            self.command_manager_selection.ids.insert(id.to_owned());
        }
        cx.notify();
    }

    fn set_command_selection_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_manager_selection.selecting = !self.command_manager_selection.selecting;
        self.command_manager_selection.ids.clear();
        self.command_manager_input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn render_command_bulk_controls(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let selecting = self.command_manager_selection.selecting;
        let count = self.command_manager_selection.ids.len();
        let owner = cx.entity().downgrade();
        h_flex()
            .w_full()
            .h(px(36.0))
            .px_2()
            .gap_1()
            .items_center()
            .flex_shrink_0()
            .child(
                Button::new("command-select-mode")
                    .h(px(32.0))
                    .debug_selector(|| "command-select-mode".into())
                    .label(language.text(if selecting {
                        Message::CommonCancel
                    } else {
                        Message::CommandsSelect
                    }))
                    .small()
                    .ghost()
                    .selected(selecting)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            this.set_command_selection_mode(window, cx);
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_command_selection_mode(window, cx)
                    })),
            )
            .when(selecting, |bar| {
                bar.child(
                    Button::new("command-select-visible")
                        .h(px(32.0))
                        .debug_selector(|| "command-select-visible".into())
                        .small()
                        .ghost()
                        .label(language.text(Message::CommandsSelectVisible))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                this.select_visible_commands(cx);
                            }
                        }))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_visible_commands(cx);
                        })),
                )
                .child(
                    Button::new("command-delete-selected")
                        .h(px(32.0))
                        .debug_selector(|| "command-delete-selected".into())
                        .small()
                        .ghost()
                        .disabled(count == 0)
                        .label(language.format(
                            Message::CommandsDeleteSelected,
                            &[("count", &count.to_string())],
                        ))
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.stop_propagation();
                                this.delete_selected_commands(window, cx);
                            }
                        }))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.delete_selected_commands(window, cx);
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("command-manage-menu")
                    .h(px(32.0))
                    .debug_selector(|| "command-manage-menu".into())
                    .small()
                    .ghost()
                    .label(language.text(Message::CommandsManage))
                    .dropdown_caret(true)
                    .dropdown_menu(move |menu, _, _| {
                        let clear_owner = owner.clone();
                        let restore_owner = owner.clone();
                        menu.item(
                            PopupMenuItem::new(language.text(Message::CommandsClearAll)).on_click(
                                move |_, window, cx| {
                                    let _ = clear_owner.update(cx, |this, cx| {
                                        this.open_command_deletion(Deletion::All, window, cx);
                                    });
                                },
                            ),
                        )
                        .item(
                            PopupMenuItem::new(language.text(Message::CommandsRestoreBuiltins))
                                .on_click(move |_, window, cx| {
                                    let _ = restore_owner.update(cx, |this, cx| {
                                        let result = this.saved_commands.restore_builtin_commands();
                                        this.finish_command_batch(result, window, cx);
                                    });
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    fn select_visible_commands(&mut self, cx: &mut Context<Self>) {
        self.command_manager_selection.ids =
            self.filtered_saved_commands(cx).into_iter().map(|command| command.id).collect();
        cx.notify();
    }

    fn delete_selected_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.command_manager_selection.ids.iter().cloned().collect();
        self.open_command_deletion(Deletion::Commands(ids), window, cx);
    }

    fn finish_command_batch(
        &mut self,
        result: std::io::Result<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let language = crate::gpui_shell::config::ui_language(cx);
        match result {
            Ok(count) => {
                self.command_manager_selection.ids.clear();
                self.command_manager_selection.cursor = 0;
                self.command_manager_scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
                crate::gpui_shell::toast::toast(
                    window,
                    cx,
                    crate::display::ToastKind::Success,
                    language
                        .format(Message::CommandsBatchChanged, &[("count", &count.to_string())]),
                );
                cx.notify();
                true
            },
            Err(error) => {
                crate::gpui_shell::toast::toast(
                    window,
                    cx,
                    crate::display::ToastKind::Warning,
                    language.format(Message::CommandsBatchFailed, &[("error", &error.to_string())]),
                );
                false
            },
        }
    }

    pub(super) fn open_command_deletion(
        &mut self,
        deletion: Deletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let description = match &deletion {
            Deletion::Commands(ids) => {
                if ids.is_empty() {
                    return;
                }
                language.format(
                    Message::CommandsDeleteCountConfirm,
                    &[("count", &ids.len().to_string())],
                )
            },
            Deletion::All => language.text(Message::CommandsClearAllConfirm).to_owned(),
            Deletion::Builtins => language.text(Message::CommandsClearBuiltinsConfirm).to_owned(),
            Deletion::Folder(_) => language.text(Message::CommandsDeleteFolderConfirm).to_owned(),
        };
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let delete_owner = owner.clone();
            let close_owner = owner.clone();
            let deletion = deletion.clone();
            let footer = DialogFooter::new()
                .child(div().flex_1())
                .child(
                    DialogClose::new().child(
                        Button::new("saved-command-delete-cancel")
                            .debug_selector(|| "saved-command-delete-cancel".into())
                            .label(language.text(Message::CommonCancel)),
                    ),
                )
                .child(
                    DialogAction::new().child(
                        Button::new("saved-command-delete-confirm")
                            .debug_selector(|| "saved-command-delete-confirm".into())
                            .label(language.text(Message::CommonDelete))
                            .danger(),
                    ),
                );
            center_modal_dialog(dialog, window, DELETE_DIALOG_HEIGHT)
                .close_button(false)
                .overlay_closable(true)
                .title(language.text(Message::CommandsDeleteTitle))
                .child(div().text_sm().child(description.clone()))
                .footer(footer)
                .on_ok(move |_, window, cx| {
                    let Some(owner) = delete_owner.upgrade() else {
                        return true;
                    };
                    owner.update(cx, |this, cx| {
                        let result = match &deletion {
                            Deletion::Commands(ids) => this.saved_commands.remove_many(ids),
                            Deletion::All => this.saved_commands.clear_commands(),
                            Deletion::Builtins => this.saved_commands.clear_builtin_commands(),
                            Deletion::Folder(id) => {
                                this.saved_commands.remove_group(id).map(|()| 1)
                            },
                        };
                        this.finish_command_batch(result, window, cx)
                    })
                })
                .on_close(move |_, window, cx| {
                    if let Some(owner) = close_owner.upgrade() {
                        owner.update(cx, |this, cx| {
                            this.focus_command_manager_or_terminal(window, cx)
                        });
                    }
                })
        });
    }
}
