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
pub(super) enum ManagerRow {
    Command(crate::saved_commands::SavedCommand),
    Folder { id: String, name: String, count: usize },
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

    pub(super) fn command_manager_rows(&self, cx: &App) -> Vec<ManagerRow> {
        let input = self.command_manager_input.read(cx).value();
        let query = input.trim();
        if query.len() > MAX_SEARCH_BYTES {
            return Vec::new();
        }
        let mut rows = self
            .filtered_saved_commands(cx)
            .into_iter()
            .map(ManagerRow::Command)
            .collect::<Vec<_>>();
        if self.command_manager_group.is_none() {
            let available = self.available_saved_commands(cx);
            let mut search = nebula_completions::command_search::CommandQuery::new(query);
            for (id, name) in self.command_groups(cx) {
                let Some(id) = id else { continue };
                if !query.is_empty() && search.score_fields(&[&name]).is_none() {
                    continue;
                }
                let count = available
                    .iter()
                    .filter(|command| {
                        self.saved_commands.group_for(&command.id) == Some(id.as_str())
                    })
                    .count();
                rows.push(ManagerRow::Folder { id, name, count });
            }
        }
        rows
    }

    pub(super) fn enter_command_group(
        &mut self,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if id.as_ref().is_some_and(|id| {
            !self.command_groups(cx).iter().any(|(group, _)| group.as_ref() == Some(id))
        }) {
            return;
        }
        self.command_manager_group = id;
        self.command_group_menu = None;
        self.command_manager_selected = 0;
        self.command_manager_scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        self.command_manager_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn render_command_group_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let name = self
            .command_groups(cx)
            .into_iter()
            .find(|(group, _)| *group == self.command_manager_group)
            .map(|(_, name)| name)
            .unwrap_or_default();
        let hover = cx.theme().list_hover;
        h_flex()
            .id("saved-command-group-back")
            .debug_selector(|| "saved-command-group-back".into())
            .w_full()
            .h(px(GROUP_NAV_HEIGHT))
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .items_center()
            .text_sm()
            .cursor_pointer()
            .hover(move |row| row.bg(hover))
            .active(move |row| row.bg(hover))
            .role(gpui::Role::Button)
            .aria_label(language.text(Message::CommandsBackToRoot))
            .on_click(cx.listener(|this, _, window, cx| this.enter_command_group(None, window, cx)))
            .drag_over::<CommandDrag>(move |row, _, _, _| row.bg(hover))
            .on_drop(cx.listener(|this, command: &CommandDrag, window, cx| {
                cx.stop_propagation();
                this.move_command_to_group(&command.id, None, window, cx);
            }))
            .child(Icon::new(IconName::ChevronLeft).xsmall())
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::CommandsRoot)),
            )
            .child(
                Icon::new(IconName::ChevronRight).xsmall().text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().min_w_0().truncate().child(name))
            .into_any_element()
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
                self.command_manager_scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
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

    pub(super) fn render_command_folder(
        &self,
        index: usize,
        id: String,
        name: String,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = cx.theme().list_hover;
        let active = cx.theme().list_active;
        let muted = cx.theme().muted_foreground;
        let selected = index == self.command_manager_selected;
        let delete_id = (id != BUILTIN_GROUP_ID).then(|| id.clone());
        let click_id = id.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        h_flex()
            .id(SharedString::from(format!("command-group-{id}")))
            .debug_selector({
                let id = id.clone();
                move || format!("command-group-{id}")
            })
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .items_center()
            .gap(px(crate::display::ui::tokens::space::XS))
            .px_2()
            .pr(px(18.0))
            .rounded_md()
            .cursor_pointer()
            .role(gpui::Role::Button)
            .aria_label(name.clone())
            .when(selected, |row| row.bg(active))
            .when(!selected, |row| row.hover(move |row| row.bg(hover)))
            .active(move |row| row.bg(active))
            .drag_over::<CommandDrag>(move |row, _, _, _| row.bg(active))
            .on_drop(cx.listener(move |this, command: &CommandDrag, window, cx| {
                cx.stop_propagation();
                this.move_command_to_group(&command.id, Some(&id), window, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.enter_command_group(Some(click_id.clone()), window, cx);
            }))
            .child(
                div()
                    .debug_selector(move || format!("command-group-icon-{index}"))
                    .size(px(ROW_ICON_SLOT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::Folder).size(px(ROW_ICON_SIZE)).text_color(muted)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(crate::display::ui::tokens::space::XXS))
                    .child(
                        div()
                            .debug_selector(move || format!("command-group-label-{index}"))
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(
                        div().text_size(px(11.0)).text_color(muted).child(
                            language.format(
                                Message::CommandsGroupCount,
                                &[("count", &count.to_string())],
                            ),
                        ),
                    ),
            )
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
            .child(Icon::new(IconName::ChevronRight).xsmall().text_color(muted))
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
