//! 用户保存命令的右上角紧凑弹窗。
//!
//! 顶栏闪电列表只负责打开管理器；行内动作才表达执行、复制、编辑和删除。
//! 弹窗覆盖在终端之上而不参与主布局，避免为了短时管理命令永久压缩 PTY。

use super::*;
mod groups;
mod rows;
pub(super) use groups::GroupMenu;
use groups::{CommandDrag, ManagerRow};

const PANEL_MAX_WIDTH: f32 = 430.0;
const PANEL_MAX_HEIGHT: f32 = 360.0;
const PANEL_EMPTY_HEIGHT: f32 = 196.0;
// Search padding, list padding, two footer rows and the panel's two borders.
const PANEL_FIXED_HEIGHT: f32 = 146.0;
const GROUP_NAV_HEIGHT: f32 = 36.0;
const PANEL_MARGIN: f32 = 8.0;
// 覆盖层从自绘标题栏下沿开始；固定组件依赖当前将该区域定义为 34px。
const WINDOW_TITLE_BAR_HEIGHT: f32 = 34.0;
const PANEL_FOOTER_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 62.0;
const ROW_ICON_SIZE: f32 = 16.0;
const ROW_ICON_SLOT: f32 = 24.0;
const EDITOR_DIALOG_HEIGHT: f32 = 430.0;
const COMMAND_INPUT_HEIGHT: f32 = 150.0;
const DELETE_DIALOG_HEIGHT: f32 = 230.0;
const MAX_SEARCH_BYTES: usize = 2 * 1024;
const COMMAND_MANAGER_KEY_CONTEXT: &str = "NebulaSavedCommands";

fn command_manager_icon() -> Icon {
    Icon::new(Icon::empty()).path(crate::gpui_shell::assets::nav::COMMAND_MANAGER)
}

fn custom_icon(path: &'static str) -> Icon {
    Icon::new(Icon::empty()).path(path)
}

fn command_run_icon(builtin: bool, append_enter: bool) -> IconName {
    if builtin || append_enter { IconName::Play } else { IconName::SquareTerminal }
}

fn command_editor_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state)
        .w_full()
        .h(px(COMMAND_INPUT_HEIGHT))
        .font_family(cx.theme().mono_font_family.clone())
}

/// 多行文本直接逐行送进 PTY 时，前台程序可能把第二行当成自己的 stdin。
/// 立即执行因此折成一条 shell 语句；仅插入模式保留用户原文供手动编辑。
fn dispatch_text(command: &crate::saved_commands::SavedCommand) -> String {
    if !command.append_enter || (!command.command.contains('\r') && !command.command.contains('\n'))
    {
        return command.command.clone();
    }
    command
        .command
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

impl NebulaWorkspace {
    pub(super) fn on_command_manager_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.command_manager_selected = 0;
                self.command_manager_scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                cx.notify();
            },
            InputEvent::PressEnter { .. } => self.run_selected_saved_command(window, cx),
            _ => {},
        }
    }

    fn available_saved_commands(&self, cx: &App) -> Vec<crate::saved_commands::SavedCommand> {
        use crate::saved_commands::builtins::CommandPlatform;
        let remote = self
            .tabs
            .get(self.active)
            .and_then(WorkspaceTab::focused_view)
            .is_some_and(|view| view.read(cx).is_remote_session());
        let platform = if remote {
            CommandPlatform::Posix
        } else if cfg!(windows) {
            CommandPlatform::Windows
        } else if cfg!(target_os = "macos") {
            CommandPlatform::Mac
        } else {
            CommandPlatform::Posix
        };
        let mut commands = self.saved_commands.commands().to_vec();
        commands.extend(
            self.saved_commands
                .builtin_commands(crate::gpui_shell::config::ui_language(cx), platform),
        );
        self.sort_command_groups(&mut commands, cx);
        commands
    }

    fn filtered_saved_commands(&self, cx: &App) -> Vec<crate::saved_commands::SavedCommand> {
        let value = self.command_manager_input.read(cx).value();
        let query = value.trim();
        if query.len() > MAX_SEARCH_BYTES {
            return Vec::new();
        }
        let commands = self
            .available_saved_commands(cx)
            .into_iter()
            .filter(|command| {
                if let Some(group) = self.command_manager_group.as_deref() {
                    self.saved_commands.group_for(&command.id) == Some(group)
                } else {
                    !query.is_empty() || self.saved_commands.group_for(&command.id).is_none()
                }
            })
            .collect::<Vec<_>>();
        if query.is_empty() {
            return commands;
        }
        let mut query = nebula_completions::command_search::CommandQuery::new(query);
        let mut matches = commands
            .into_iter()
            .enumerate()
            .filter_map(|(index, command)| {
                query
                    .score_fields(&[&command.name, &command.command])
                    .map(|score| (score, index, command))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(a, ai, _), (b, bi, _)| b.cmp(a).then(ai.cmp(bi)));
        let mut commands = matches.into_iter().map(|(_, _, command)| command).collect::<Vec<_>>();
        self.sort_command_groups(&mut commands, cx);
        commands
    }

    pub(super) fn toggle_command_manager(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.command_manager_open {
            self.close_command_manager(window, cx);
            return;
        }
        self.dismiss_palette_state();
        if let Err(error) = self.saved_commands.reload() {
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                format!("无法读取已保存命令：{error}"),
            );
        }
        self.command_manager_open = true;
        self.command_manager_group = None;
        self.command_manager_selected = 0;
        self.command_manager_scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        self.command_manager_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close_command_manager(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.command_manager_open {
            return;
        }
        self.command_manager_open = false;
        self.command_group_menu = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn focus_command_manager_or_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.command_manager_open {
            self.command_manager_input.update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.focus_active(window, cx);
        }
    }

    fn move_saved_command_selection(&mut self, delta: isize, cx: &mut Context<'_, Self>) {
        if !self.command_manager_open {
            return;
        }
        let len = self.command_manager_rows(cx).len();
        self.command_manager_selected = if len == 0 {
            0
        } else {
            (self.command_manager_selected as isize + delta).rem_euclid(len as isize) as usize
        };
        self.command_manager_scroll
            .scroll_to_item(self.command_manager_selected, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    pub(super) fn run_selected_saved_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match self.command_manager_rows(cx).get(self.command_manager_selected).cloned() {
            Some(ManagerRow::Command(command)) => self.dispatch_saved_command(command, window, cx),
            Some(ManagerRow::Folder { id, .. }) => self.enter_command_group(Some(id), window, cx),
            None => {},
        }
    }

    fn dispatch_saved_command(
        &mut self,
        command: crate::saved_commands::SavedCommand,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let view = self.tabs.get(self.active).and_then(WorkspaceTab::focused_view).cloned();
        let Some(view) = view else {
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                "当前标签不是可用的终端",
            );
            return;
        };

        let submit = command.append_enter;
        let text = dispatch_text(&command);
        match view.update(cx, |view, cx| view.runtime_prompt(text, submit, cx)) {
            Ok(()) => {
                self.command_manager_open = false;
                self.focus_active(window, cx);
                cx.notify();
            },
            Err(error) => crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                format!("无法发送命令：{}", error.message),
            ),
        }
    }

    fn copy_saved_command(
        &mut self,
        command: &crate::saved_commands::SavedCommand,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(command.command.clone()));
        crate::gpui_shell::toast::toast(window, cx, crate::display::ToastKind::Info, "命令已复制");
    }

    fn open_saved_command_editor(
        &mut self,
        edit_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let current = edit_id.as_deref().and_then(|id| {
            self.available_saved_commands(cx).into_iter().find(|command| command.id == id)
        });
        if edit_id.is_some() && current.is_none() {
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                "这条命令已不存在",
            );
            return;
        }

        let edit_id = edit_id.filter(|id| !id.starts_with("builtin:"));
        let language = crate::gpui_shell::config::ui_language(cx);
        let initial_name = current.as_ref().map(|command| command.name.clone()).unwrap_or_default();
        let initial_command =
            current.as_ref().map(|command| command.command.clone()).unwrap_or_default();
        let append_enter = Rc::new(std::cell::Cell::new(
            current.as_ref().is_none_or(|command| command.append_enter),
        ));
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.pick("例如：启动开发服务", "For example: Start dev server"))
        });
        name_input.update(cx, |input, cx| input.set_value(initial_name, window, cx));
        let command_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .soft_wrap(true)
                .placeholder(language.pick("输入 Shell 命令", "Enter a shell command"))
        });
        command_input.update(cx, |input, cx| input.set_value(initial_command, window, cx));

        let workspace = cx.entity().downgrade();
        let dialog_workspace = workspace.clone();
        let dialog_name = name_input.clone();
        let dialog_command = command_input.clone();
        let dialog_append = append_enter.clone();
        let title = if edit_id.is_some() {
            language.pick("编辑命令", "Edit Command")
        } else {
            language.pick("新增命令", "Add Command")
        };
        let save_label = language.pick("保存", "Save");
        let cancel_label = language.pick("取消", "Cancel");

        window.open_dialog(cx, move |dialog, window, cx| {
            let checkbox_state = dialog_append.clone();
            let name = dialog_name.clone();
            let command = dialog_command.clone();
            let save_name = name.clone();
            let save_command = command.clone();
            let save_append = dialog_append.clone();
            let save_workspace = dialog_workspace.clone();
            let save_id = edit_id.clone();
            let close_workspace = dialog_workspace.clone();
            let body = v_flex()
                .w_full()
                .gap_3()
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .child(div().text_sm().font_semibold().child(language.pick("名称", "Name")))
                        .child(Input::new(&name).w_full()),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .child(
                            div().text_sm().font_semibold().child(language.pick("命令", "Command")),
                        )
                        .child(command_editor_input(&command, cx)),
                )
                .child(
                    gpui_component::checkbox::Checkbox::new("saved-command-append-enter")
                        .checked(checkbox_state.get())
                        .label(
                            language.pick(
                                "插入后立即按 Enter 执行",
                                "Press Enter and run after inserting",
                            ),
                        )
                        .on_click(move |checked, window, _| {
                            checkbox_state.set(*checked);
                            window.refresh();
                        }),
                )
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                    language.pick(
                        "关闭时只把命令放入当前终端，适合运行前补参数。",
                        "When off, the command is inserted for editing.",
                    ),
                ));
            let footer = DialogFooter::new()
                .child(div().flex_1())
                .child(
                    DialogClose::new()
                        .child(Button::new("saved-command-cancel").label(cancel_label)),
                )
                .child(
                    DialogAction::new()
                        .child(Button::new("saved-command-save").label(save_label).primary()),
                );

            center_modal_dialog(dialog, window, EDITOR_DIALOG_HEIGHT)
                .close_button(false)
                .overlay_closable(true)
                .title(div().text_lg().font_semibold().child(title))
                .footer(footer)
                .child(body)
                .on_ok(move |_, window, cx| {
                    let name = save_name.read(cx).value().to_string();
                    let command = save_command.read(cx).value().to_string();
                    let append_enter = save_append.get();
                    let Some(workspace) = save_workspace.upgrade() else {
                        return true;
                    };
                    let result = workspace.update(cx, |workspace, cx| {
                        let result = match save_id.as_deref() {
                            Some(id) => {
                                workspace.saved_commands.update(id, &name, &command, append_enter)
                            },
                            None => workspace
                                .saved_commands
                                .insert_in_group(
                                    &name,
                                    &command,
                                    append_enter,
                                    workspace.command_manager_group.as_deref(),
                                )
                                .map(|_| ()),
                        };
                        if result.is_ok() {
                            cx.notify();
                        }
                        result
                    });
                    match result {
                        Ok(()) => {
                            crate::gpui_shell::toast::toast(
                                window,
                                cx,
                                crate::display::ToastKind::Success,
                                language.pick("命令已保存", "Command saved"),
                            );
                            true
                        },
                        Err(error) => {
                            crate::gpui_shell::toast::toast(
                                window,
                                cx,
                                crate::display::ToastKind::Warning,
                                format!("{}: {error}", language.pick("保存失败", "Save failed")),
                            );
                            false
                        },
                    }
                })
                .on_close(move |_, window, cx| {
                    if let Some(workspace) = close_workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| {
                            workspace.focus_command_manager_or_terminal(window, cx);
                        });
                    }
                })
        });
        name_input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn open_delete_saved_command_dialog(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(command) =
            self.available_saved_commands(cx).into_iter().find(|command| command.id == id)
        else {
            return;
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let workspace = cx.entity().downgrade();
        let dialog_workspace = workspace.clone();
        let command_name = command.name.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let delete_workspace = dialog_workspace.clone();
            let close_workspace = dialog_workspace.clone();
            let delete_id = id.clone();
            let body = v_flex()
                .w_full()
                .gap_2()
                .child(div().text_sm().child(format!(
                    "{}“{}”？",
                    language.pick("确定删除命令 ", "Delete command "),
                    command_name
                )))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(language.pick("删除后无法撤销。", "This action cannot be undone.")),
                );
            let footer = DialogFooter::new()
                .child(div().flex_1())
                .child(
                    DialogClose::new().child(
                        Button::new("saved-command-delete-cancel")
                            .debug_selector(|| "saved-command-delete-cancel".into())
                            .label(language.pick("取消", "Cancel")),
                    ),
                )
                .child(
                    DialogAction::new().child(
                        Button::new("saved-command-delete-confirm")
                            .debug_selector(|| "saved-command-delete-confirm".into())
                            .label(language.pick("删除", "Delete"))
                            .danger(),
                    ),
                );
            center_modal_dialog(dialog, window, DELETE_DIALOG_HEIGHT)
                .close_button(false)
                .overlay_closable(true)
                .title(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(language.pick("删除已保存命令", "Delete Saved Command")),
                )
                .footer(footer)
                .child(body)
                .on_ok(move |_, window, cx| {
                    let Some(workspace) = delete_workspace.upgrade() else {
                        return true;
                    };
                    let result = workspace.update(cx, |workspace, cx| {
                        let result = workspace.saved_commands.remove(&delete_id);
                        if result.is_ok() {
                            let len = workspace.filtered_saved_commands(cx).len();
                            workspace.command_manager_selected =
                                workspace.command_manager_selected.min(len.saturating_sub(1));
                            cx.notify();
                        }
                        result
                    });
                    match result {
                        Ok(()) => true,
                        Err(error) => {
                            crate::gpui_shell::toast::toast(
                                window,
                                cx,
                                crate::display::ToastKind::Warning,
                                format!("{}: {error}", language.pick("删除失败", "Delete failed")),
                            );
                            false
                        },
                    }
                })
                .on_close(move |_, window, cx| {
                    if let Some(workspace) = close_workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| {
                            workspace.focus_command_manager_or_terminal(window, cx);
                        });
                    }
                })
        });
    }

    pub(super) fn render_command_manager(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        use crate::display::ui::tokens::{control, radius};

        let theme = cx.theme();
        let panel_bg = theme.popover;
        let surface_bg = theme.muted;
        let hover_bg = theme.list_hover;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let border = theme.border;
        let language = crate::gpui_shell::config::ui_language(cx);
        let viewport = window.viewport_size();
        let panel_width =
            PANEL_MAX_WIDTH.min((f32::from(viewport.width) - PANEL_MARGIN * 2.0).max(0.0));
        let available_height =
            (f32::from(viewport.height) - WINDOW_TITLE_BAR_HEIGHT - PANEL_MARGIN * 2.0).max(0.0);

        let rows = self.command_manager_rows(cx);
        self.command_manager_selected =
            self.command_manager_selected.min(rows.len().saturating_sub(1));
        let in_group = self.command_manager_group.is_some();
        let fixed_height = PANEL_FIXED_HEIGHT
            + if in_group { GROUP_NAV_HEIGHT - PANEL_FOOTER_HEIGHT } else { 0.0 };
        let content_height = rows.len() as f32 * ROW_HEIGHT;
        let desired_height =
            if rows.is_empty() { PANEL_EMPTY_HEIGHT } else { fixed_height + content_height };
        let panel_height = desired_height.min(PANEL_MAX_HEIGHT).min(available_height);
        let list_scrollable = content_height > (panel_height - fixed_height).max(0.0);

        let search_box = h_flex()
            .w_full()
            .h(px(control::MIN_HIT_TARGET))
            .flex_shrink_0()
            .rounded(px(radius::CONTROL))
            .border_1()
            .border_color(border)
            .bg(surface_bg)
            .overflow_hidden()
            .child(
                Input::new(&self.command_manager_input)
                    .w_full()
                    .appearance(false)
                    .focus_bordered(false)
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).xsmall().text_color(muted))
                    .text_size(px(13.0)),
            );

        let list_content = if rows.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(muted)
                .child(command_manager_icon().with_size(px(28.0)))
                .child(
                    div()
                        .text_sm()
                        .text_color(foreground)
                        .child(language.pick("没有匹配的命令", "No matching commands")),
                )
                .into_any_element()
        } else {
            let scroll_handle = self.command_manager_scroll.clone();
            let list = gpui::uniform_list(
                "saved-command-results-scroll",
                rows.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| match rows[index].clone() {
                            ManagerRow::Command(command) => {
                                this.render_saved_command_row(index, command, list_scrollable, cx)
                            },
                            ManagerRow::Folder { id, name, count } => {
                                this.render_command_folder(index, id, name, count, cx)
                            },
                        })
                        .collect()
                }),
            )
            .size_full()
            .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
            .track_scroll(&scroll_handle);
            div()
                .relative()
                .size_full()
                .min_h_0()
                .overflow_hidden()
                .child(list)
                .when(list_scrollable, |list| {
                    list.child(
                        div().absolute().top_0().right_0().bottom_0().w(px(16.0)).child(
                            gpui_component::scroll::Scrollbar::vertical(&scroll_handle)
                                .scrollbar_show(gpui_component::scroll::ScrollbarShow::Always),
                        ),
                    )
                })
                .into_any_element()
        };

        div()
            .absolute()
            .inset_0()
            .occlude()
            .key_context(COMMAND_MANAGER_KEY_CONTEXT)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_command_manager(window, cx);
                }),
            )
            .on_key_down(cx.listener(|this: &mut Self, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "up" => {
                        this.move_saved_command_selection(-1, cx);
                        cx.stop_propagation();
                    },
                    "down" => {
                        this.move_saved_command_selection(1, cx);
                        cx.stop_propagation();
                    },
                    "escape" => {
                        if this.command_manager_group.is_some() {
                            this.enter_command_group(None, window, cx);
                        } else {
                            this.close_command_manager(window, cx);
                        }
                        cx.stop_propagation();
                    },
                    "left" | "backspace"
                        if this.command_manager_group.is_some()
                            && this.command_manager_input.read(cx).value().is_empty() =>
                    {
                        this.enter_command_group(None, window, cx);
                        cx.stop_propagation();
                    },
                    _ => {},
                }
            }))
            .child(
                v_flex()
                    .absolute()
                    .top(px(WINDOW_TITLE_BAR_HEIGHT + PANEL_MARGIN))
                    .right(px(PANEL_MARGIN))
                    .w(px(panel_width))
                    .h(px(panel_height))
                    .rounded(px(radius::OVERLAY))
                    .border_1()
                    .border_color(border)
                    .bg(panel_bg)
                    .shadow_lg()
                    .overflow_hidden()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(div().w_full().flex_shrink_0().p_2().child(search_box))
                    .when(in_group, |panel| panel.child(self.render_command_group_navigation(cx)))
                    .child(div().flex_1().min_h_0().px_2().pb_2().child(list_content))
                    .child(
                        h_flex()
                            .id("saved-command-add")
                            .debug_selector(|| "saved-command-add".into())
                            .focusable()
                            .tab_stop(true)
                            .role(gpui::Role::Button)
                            .w_full()
                            .h(px(PANEL_FOOTER_HEIGHT))
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .border_t_1()
                            .border_color(border)
                            .text_sm()
                            .text_color(muted)
                            .cursor_pointer()
                            .hover(move |row| row.bg(hover_bg).text_color(foreground))
                            .active(move |row| row.bg(hover_bg))
                            .focus_visible(move |row| row.bg(hover_bg).text_color(foreground))
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.stop_propagation();
                                    this.open_saved_command_editor(None, window, cx);
                                }
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_saved_command_editor(None, window, cx);
                            }))
                            .child(
                                div()
                                    .id("saved-command-add-icon")
                                    .debug_selector(|| "saved-command-add-icon".into())
                                    .child(Icon::new(IconName::Plus).xsmall()),
                            )
                            .child(language.text(crate::i18n::Message::CommandsAddCommand)),
                    )
                    .when(!in_group, |panel| {
                        panel.child(
                            h_flex()
                                .id("saved-command-add-group")
                                .debug_selector(|| "saved-command-add-group".into())
                                .focusable()
                                .tab_stop(true)
                                .role(gpui::Role::Button)
                                .w_full()
                                .h(px(PANEL_FOOTER_HEIGHT))
                                .flex_shrink_0()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .text_sm()
                                .text_color(muted)
                                .cursor_pointer()
                                .hover(move |row| row.bg(hover_bg).text_color(foreground))
                                .active(move |row| row.bg(hover_bg))
                                .focus_visible(move |row| row.bg(hover_bg).text_color(foreground))
                                .on_key_down(cx.listener(
                                    |this, event: &KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            cx.stop_propagation();
                                            this.open_command_group_editor(window, cx);
                                        }
                                    },
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.open_command_group_editor(window, cx);
                                }))
                                .child(
                                    div()
                                        .id("saved-command-add-group-icon")
                                        .debug_selector(|| "saved-command-add-group-icon".into())
                                        .child(Icon::new(IconName::Folder).xsmall()),
                                )
                                .child(language.text(crate::i18n::Message::CommandsAddGroup)),
                        )
                    }),
            )
            .children(self.render_command_group_menu())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_keep_the_play_icon_without_changing_insert_behavior() {
        assert!(matches!(command_run_icon(true, false), IconName::Play));
        assert!(matches!(command_run_icon(false, true), IconName::Play));
        assert!(matches!(command_run_icon(false, false), IconName::SquareTerminal));
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod input_tests;

#[cfg(all(test, feature = "gpui-test-support"))]
mod group_tests;
