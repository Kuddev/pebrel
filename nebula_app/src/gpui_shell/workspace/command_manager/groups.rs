//! Command organization UI; persistence remains in `saved_commands`.

use super::*;
use crate::i18n::Message;
use crate::saved_commands::BUILTIN_GROUP_ID;
use gpui::{Anchor, AnyElement, DismissEvent, Point, anchored, deferred};
use gpui_component::menu::PopupMenuItem;

pub(in crate::gpui_shell::workspace) struct GroupMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

#[derive(Clone)]
pub(super) struct CommandDrag {
    id: String,
    name: String,
}

impl CommandDrag {
    pub(super) fn new(command: &crate::saved_commands::SavedCommand) -> Self {
        Self { id: command.id.clone(), name: command.name.clone() }
    }
}

impl Render for CommandDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(self.name.clone())
    }
}

impl NebulaWorkspace {
    fn command_groups(&self, cx: &App) -> Vec<(Option<String>, String)> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let mut groups = vec![(None, language.text(Message::CommandsUngrouped).to_owned())];
        groups.extend(
            self.saved_commands
                .groups()
                .iter()
                .map(|group| (Some(group.id.clone()), group.name.clone())),
        );
        groups.push((
            Some(BUILTIN_GROUP_ID.into()),
            language.text(Message::CommandsBuiltinGroup).to_owned(),
        ));
        groups
    }

    pub(super) fn visible_command_groups(
        &self,
        commands: &[crate::saved_commands::SavedCommand],
        cx: &App,
    ) -> Vec<(Option<String>, String)> {
        let searching = !self.command_manager_input.read(cx).value().trim().is_empty();
        self.command_groups(cx)
            .into_iter()
            .filter(|(group, _)| {
                !searching
                    || commands.iter().any(|command| {
                        self.saved_commands.group_for(&command.id) == group.as_deref()
                    })
            })
            .collect()
    }

    pub(super) fn sort_command_groups(
        &self,
        commands: &mut [crate::saved_commands::SavedCommand],
        cx: &App,
    ) {
        let groups = self.command_groups(cx);
        commands.sort_by_key(|command| {
            groups.iter().position(|(group, _)| {
                group.as_deref() == self.saved_commands.group_for(&command.id)
            })
        });
    }

    pub(super) fn command_scroll_index(
        &self,
        commands: &[crate::saved_commands::SavedCommand],
        cx: &App,
    ) -> usize {
        let Some(command) = commands.get(self.command_manager_selected) else { return 0 };
        let headers = self
            .visible_command_groups(commands, cx)
            .iter()
            .position(|(group, _)| group.as_deref() == self.saved_commands.group_for(&command.id))
            .unwrap_or(0)
            + 1;
        self.command_manager_selected + headers
    }

    fn finish_group_change(
        &mut self,
        result: std::io::Result<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let language = crate::gpui_shell::config::ui_language(cx);
        match result {
            Ok(()) => {
                self.command_manager_selected = 0;
                self.command_manager_scroll.scroll_to_item(0);
                cx.notify();
                true
            },
            Err(error) => {
                crate::gpui_shell::toast::toast(
                    window,
                    cx,
                    crate::display::ToastKind::Warning,
                    format!("{}: {error}", language.text(Message::CommandsGroupSaveFailed)),
                );
                false
            },
        }
    }

    fn move_command_to_group(
        &mut self,
        id: &str,
        group: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.saved_commands.move_to_group(id, group);
        self.finish_group_change(result, window, cx);
    }

    pub(super) fn render_command_group_header(
        &self,
        id: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = cx.theme().list_hover;
        let selected = cx.theme().list_active;
        let delete_id = id.clone().filter(|id| id != BUILTIN_GROUP_ID);
        let language = crate::gpui_shell::config::ui_language(cx);
        h_flex()
            .id(SharedString::from(format!(
                "command-group-{}",
                id.as_deref().unwrap_or("ungrouped")
            )))
            .debug_selector({
                let id = id.clone();
                move || format!("command-group-{}", id.as_deref().unwrap_or("ungrouped")).into()
            })
            .w_full()
            .h(px(GROUP_HEADER_HEIGHT))
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .rounded_md()
            .drag_over::<CommandDrag>(move |style, _, _, _| style.bg(selected))
            .on_drop(cx.listener(move |this, command: &CommandDrag, window, cx| {
                cx.stop_propagation();
                this.move_command_to_group(&command.id, id.as_deref(), window, cx);
            }))
            .child(Icon::new(IconName::Folder).xsmall())
            .child(div().flex_1().truncate().child(name))
            .when_some(delete_id, |row, id| {
                row.child(
                    Button::new(SharedString::from(format!("delete-command-group-{id}")))
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .tooltip(language.text(Message::CommandsDeleteGroup))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            let result = this.saved_commands.remove_group(&id);
                            this.finish_group_change(result, window, cx);
                        })),
                )
            })
            .hover(move |style| style.bg(hover))
            .into_any_element()
    }

    pub(super) fn open_command_group_menu(
        &mut self,
        command_id: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let current = self.saved_commands.group_for(&command_id).map(str::to_owned);
        let targets = self.command_groups(cx);
        let workspace = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            for (target, name) in targets {
                if target == current {
                    continue;
                }
                let label = if target.is_none() {
                    language.text(Message::CommandsRemoveFromGroup).to_owned()
                } else {
                    name
                };
                let workspace = workspace.clone();
                let command_id = command_id.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |this, cx| {
                            this.move_command_to_group(&command_id, target.as_deref(), window, cx)
                        });
                    }
                }));
            }
            menu
        });
        menu.focus_handle(cx).focus(window, cx);
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                this.command_group_menu = None;
                this.focus_command_manager_or_terminal(window, cx);
                cx.notify();
            });
        self.command_group_menu = Some(GroupMenu { menu, position, _subscription: subscription });
        cx.notify();
    }

    pub(super) fn render_command_group_menu(&self) -> Option<AnyElement> {
        let state = self.command_group_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(state.position)
                    .snap_to_window_with_margin(px(8.0))
                    .anchor(Anchor::TopLeft)
                    .child(state.menu.clone()),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(super) fn open_command_group_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(Message::CommandsGroupName))
        });
        let dialog_input = input.clone();
        let workspace = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let save_input = dialog_input.clone();
            let save_workspace = workspace.clone();
            let close_workspace = workspace.clone();
            center_modal_dialog(dialog, window, 220.0)
                .close_button(false)
                .title(language.text(Message::CommandsAddGroup))
                .child(Input::new(&dialog_input).w_full())
                .footer(
                    DialogFooter::new()
                        .child(div().flex_1())
                        .child(
                            DialogClose::new().child(
                                Button::new("command-group-cancel")
                                    .label(language.text(Message::CommonCancel)),
                            ),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("command-group-save")
                                    .primary()
                                    .label(language.text(Message::CommonSave)),
                            ),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let name = save_input.read(cx).value().to_string();
                    let Some(workspace) = save_workspace.upgrade() else { return true };
                    workspace.update(cx, |this, cx| {
                        let result = this.saved_commands.create_group(&name);
                        this.finish_group_change(result, window, cx)
                    })
                })
                .on_close(move |_, window, cx| {
                    if let Some(workspace) = close_workspace.upgrade() {
                        workspace.update(cx, |this, cx| {
                            this.focus_command_manager_or_terminal(window, cx)
                        });
                    }
                })
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }
}
