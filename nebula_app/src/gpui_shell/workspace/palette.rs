use super::*;

const PALETTE_SEARCH_HEIGHT: f32 = 48.0;

/// 命令面板 / Shell 选择弹窗里图标轨的逻辑边长。品牌贴图、Nerd Font 字形
/// 与组件库线性图标都排进同一个槽，行与行之间才不会有的缩进有的不缩进。
const PALETTE_ICON_PX: f32 = 22.0;

/// 非 shell 行（SSH 主机等）的字形字号。shell 行另有配平后的字号，见
/// `shell_glyph_size`。
const PALETTE_GLYPH_PX: f32 = 16.0;

/// shell 行字形的字号：与品牌贴图按同一视觉尺寸配平（`shell_detect` 的
/// 安全边距/墨迹比例是这条换算的唯一出处）。
fn shell_glyph_size() -> f32 {
    PALETTE_ICON_PX * crate::shell_detect::FALLBACK_ICON_SCALE
}

impl NebulaWorkspace {
    pub(super) fn run_selected_palette_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = self
            .filtered_palette_rows(cx)
            .get(self.command_palette_selected)
            .map(|row| row.action.clone());
        if let Some(action) = action {
            self.run_workspace_palette_action(action, window, cx);
        }
    }
    pub(super) fn run_workspace_palette_action(
        &mut self,
        action: WorkspacePaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            WorkspacePaletteAction::TerminalReading { commands } => {
                self.dismiss_palette_state();
                if let Some(view) =
                    self.tabs.get(self.active).and_then(WorkspaceTab::focused_view).cloned()
                {
                    view.update(cx, |view, cx| {
                        view.open_reading(commands, window, cx);
                    });
                }
                cx.notify();
            },
            WorkspacePaletteAction::Shared(action) => self.run_palette_action(action, window, cx),
            WorkspacePaletteAction::LayoutRecipes => {
                self.dismiss_palette_state();
                self.open_layout_recipes(window, cx);
            },
            WorkspacePaletteAction::FocusTab(tab) => {
                self.dismiss_palette_state();
                self.activate_tab(tab, window, cx);
                self.focus_active(window, cx);
                cx.notify();
            },
            WorkspacePaletteAction::FocusPane { tab, pane } => {
                self.dismiss_palette_state();
                self.activate_tab(tab, window, cx);
                self.focus_pane(tab, pane, window, cx);
                self.focus_active(window, cx);
                cx.notify();
            },
            WorkspacePaletteAction::OpenDirectory(path) => {
                self.dismiss_palette_state();
                self.add_terminal_at(Some(path), None, window, cx);
            },
            WorkspacePaletteAction::RunAiSession { command, cwd } => {
                self.dismiss_palette_state();
                self.add_terminal_at(cwd, Some(command), window, cx);
            },
            WorkspacePaletteAction::LaunchSshHost(host) => {
                self.dismiss_palette_state();
                self.add_ssh_terminal(host, window, cx);
            },
            WorkspacePaletteAction::LaunchShell(detected) => {
                self.launch_palette_shell(detected, window, cx);
            },
            WorkspacePaletteAction::LaunchProfile(profile) => {
                self.launch_palette_profile(profile, window, cx);
            },
        }
    }

    /// The catalog itself is owned by the old/shared command model. This
    /// presentation advertises only actions whose execution path is already
    /// wired in GPUI; unsupported rows remain in the shared catalog and appear
    /// automatically when their host service is connected.
    pub(super) fn palette_action_supported(
        action: &crate::display::command_palette::PaletteAction,
    ) -> bool {
        use crate::display::command_palette::PaletteAction;
        matches!(
            action,
            PaletteAction::NewTab
                | PaletteAction::NewWindow
                | PaletteAction::CopyCwd
                | PaletteAction::RevealCwd
                | PaletteAction::CloseTab
                | PaletteAction::NextTab
                | PaletteAction::PrevTab
                | PaletteAction::ToggleSidebar
                | PaletteAction::OpenSettings
                | PaletteAction::ToggleGhost
                | PaletteAction::CycleCompletionStyle
                | PaletteAction::ToggleFilesPanel
                | PaletteAction::OpenAiSessionPicker
                | PaletteAction::SelectTheme(_)
                | PaletteAction::SplitRight
                | PaletteAction::SplitDown
                | PaletteAction::ToggleGitPanel
                | PaletteAction::ExportWorkspace
        )
    }

    pub(super) fn palette_action_available(
        action: &crate::display::command_palette::PaletteAction,
        has_local_cwd: bool,
        has_cwd: bool,
    ) -> bool {
        use crate::display::command_palette::PaletteAction;

        Self::palette_action_supported(action)
            && (has_local_cwd || !matches!(action, PaletteAction::RevealCwd))
            && (has_cwd || !matches!(action, PaletteAction::CopyCwd))
    }

    pub(super) fn filtered_palette_rows(&self, cx: &App) -> Vec<WorkspacePaletteRow> {
        let query = self.command_palette_input.read(cx).value().to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        // Rendering must not stat `\\wsl.localhost\…` (it can start the guest or wait
        // on the redirector): a WSL pane offers the action and the handler maps it.
        let has_local_cwd =
            self.active_wsl_cwd(cx).is_some() || self.active_local_cwd(cx).is_some();
        let has_cwd = self
            .tabs
            .get(self.active)
            .and_then(WorkspaceTab::focused_view)
            .is_some_and(|view| view.read(cx).working_directory().is_some());
        let language = crate::gpui_shell::config::ui_language(cx);
        let rows = self.palette_override.clone().unwrap_or_else(|| {
            let mut rows: Vec<WorkspacePaletteRow> = crate::display::command_palette::catalog()
                .iter()
                .filter(|item| Self::palette_action_available(&item.action, has_local_cwd, has_cwd))
                .map(|item| {
                    let (group_order, group) =
                        crate::display::command_palette::command_group_metadata(
                            &item.action,
                            language,
                            has_local_cwd,
                            false,
                        );
                    WorkspacePaletteRow {
                        group_order,
                        group,
                        label: item.label.to_owned(),
                        hint: item.hint.to_owned(),
                        hint_style: WorkspacePaletteHintStyle::Shortcut,
                        search: item.search.to_owned(),
                        action: WorkspacePaletteAction::Shared(item.action.clone()),
                        icon: None,
                        icon_glyph: None,
                        icon_path: None,
                    }
                })
                .collect();
            rows.push(recipes::palette_row(language));
            if self.tabs.get(self.active).and_then(WorkspaceTab::focused_view).is_some() {
                for (commands, message) in [
                    (false, crate::i18n::Message::TerminalReadingOpen),
                    (true, crate::i18n::Message::TerminalReadingChoose),
                ] {
                    let label = language.text(message).to_owned();
                    rows.push(WorkspacePaletteRow {
                        group_order: 0,
                        group: language.text(crate::i18n::Message::TerminalReadingOpen).to_owned(),
                        search: format!("{label} terminal search find copy output shell"),
                        label,
                        hint: String::new(),
                        hint_style: WorkspacePaletteHintStyle::Metadata,
                        action: WorkspacePaletteAction::TerminalReading { commands },
                        icon: None,
                        icon_glyph: None,
                        icon_path: None,
                    });
                }
            }
            // 启动器混排（旧壳 ⌘K 裁定）：SSH 主机与命令同列，置顶/隐藏
            // 次序由共享 merge 权威裁定。
            rows.extend(shell_picker::ssh_palette_rows(
                &crate::gpui_shell::ssh_hosts::SshHostLists::load(),
                usize::MAX,
                language.pick("SSH 主机", "SSH HOSTS"),
            ));
            rows
        });
        let mut rows: Vec<_> = rows
            .into_iter()
            .filter(|row| {
                if let Some(filter) = self.quick_jump_filter
                    && !filter.matches(&row.action)
                {
                    return false;
                }
                if self.shell_picker_open {
                    let keep = match self.launcher_filter {
                        crate::display::command_palette::LauncherFilter::All => true,
                        crate::display::command_palette::LauncherFilter::Ssh => {
                            matches!(row.action, WorkspacePaletteAction::LaunchSshHost(_))
                        },
                        crate::display::command_palette::LauncherFilter::Shell => {
                            matches!(
                                row.action,
                                WorkspacePaletteAction::LaunchShell(_)
                                    | WorkspacePaletteAction::LaunchProfile(_)
                            )
                        },
                    };
                    if !keep {
                        return false;
                    }
                }
                words.is_empty()
                    || words.iter().all(|word| row.search.to_ascii_lowercase().contains(word))
            })
            .collect();
        rows.sort_by_key(|row| row.group_order);
        rows
    }

    pub(super) fn render_command_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::display::ui::tokens::{radius, space};

        let theme = cx.theme();
        let panel_bg = theme.popover;
        let surface_bg = theme.muted;
        let selected_bg = theme.list_active;
        let hover_bg = theme.list_hover;
        let accent = theme.primary;
        let ring = theme.ring;
        let border = theme.border;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let overlay = theme.overlay;
        let mono_family = theme.mono_font_family.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let palette_filters = if self.shell_picker_open {
            Some((
                WorkspacePaletteFilter::Launcher(self.launcher_filter),
                self.launcher_chip_counts()
                    .into_iter()
                    .map(|(filter, count)| (WorkspacePaletteFilter::Launcher(filter), count))
                    .collect::<Vec<_>>(),
            ))
        } else {
            self.quick_jump_filter.map(|selected| {
                (
                    WorkspacePaletteFilter::QuickJump(selected),
                    self.quick_jump_chip_counts()
                        .into_iter()
                        .map(|(filter, count)| (WorkspacePaletteFilter::QuickJump(filter), count))
                        .collect::<Vec<_>>(),
                )
            })
        };
        let items = self.filtered_palette_rows(cx);
        if self.command_palette_selected >= items.len() {
            self.command_palette_selected = items.len().saturating_sub(1);
        }
        let has_filter_bar = palette_filters.is_some();
        let item_count = items.len();
        let mut group_count = 0;
        let mut measured_group: Option<&str> = None;
        for item in &items {
            if measured_group != Some(item.group.as_str()) {
                measured_group = Some(item.group.as_str());
                group_count += 1;
            }
        }
        let content_node_count = item_count + group_count;
        let content_height = if item_count == 0 {
            0.0
        } else {
            item_count as f32 * PALETTE_ROW_HEIGHT
                + group_count as f32 * PALETTE_GROUP_HEADER_HEIGHT
                + content_node_count.saturating_sub(1) as f32 * PALETTE_ROW_GAP
                + 8.0
        };
        // 面板外框必须稳定：筛选结果变少时只留下空白，不能让居中的面板连同
        // 搜索框一起上下跳。Command Palette 没有筛选条，结果视口自然多占一行。
        let results_height = f32::from(window.viewport_size().height).min(PALETTE_PANEL_HEIGHT)
            - 2.0
            - PALETTE_SEARCH_HEIGHT
            - if has_filter_bar { PALETTE_FILTER_BAR_HEIGHT } else { 0.0 };
        let results_scrollable = content_height > results_height;
        // 同一份 picker 只要有一行带图标，就为全部行保留固定 icon rail；常规
        // 命令目录没有图标时整列消失，不会凭空多出一层缩进。
        let has_icon_rail = items.iter().any(|item| {
            item.icon.is_some() || item.icon_glyph.is_some() || item.icon_path.is_some()
        });

        let mut rows = Vec::new();
        let mut previous_group: Option<String> = None;
        for (ix, item) in items.into_iter().enumerate() {
            if previous_group.as_deref() != Some(item.group.as_str()) {
                previous_group = Some(item.group.clone());
                rows.push(
                    h_flex()
                        .h(px(PALETTE_GROUP_HEADER_HEIGHT))
                        .flex_shrink_0()
                        .px_3()
                        .when(results_scrollable, |header| {
                            header.pr(px(PALETTE_SCROLLBAR_CONTENT_GUTTER))
                        })
                        .gap(px(space::XS))
                        .items_end()
                        .pb(px(8.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(muted)
                                .child(item.group.clone()),
                        )
                        .into_any_element(),
                );
            }

            let icon_content = if let Some(image) = item.icon.clone() {
                Some(
                    gpui::StyledImage::object_fit(
                        img(image).size(px(PALETTE_ICON_PX)),
                        gpui::ObjectFit::Contain,
                    )
                    .into_any_element(),
                )
            } else if let Some(glyph) = item.icon_glyph {
                // shell 行没有品牌贴图时画的是同一个 id-keyed 字形，字号要
                // 跟品牌贴图配平；SSH 主机等行保持原来的字号。
                let glyph_px = if matches!(
                    &item.action,
                    WorkspacePaletteAction::LaunchShell(_)
                        | WorkspacePaletteAction::LaunchProfile(_)
                ) {
                    shell_glyph_size()
                } else {
                    PALETTE_GLYPH_PX
                };
                Some(
                    div()
                        .size(px(PALETTE_ICON_PX))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(crate::font_install::REQUIRED_FONT_FAMILY)
                        .text_size(px(glyph_px))
                        .text_color(foreground)
                        .child(crate::gpui_shell::widgets::terminal_glyph(
                            glyph.to_string(),
                            PALETTE_ICON_PX,
                        ))
                        .into_any_element(),
                )
            } else {
                item.icon_path.clone().map(|path| {
                    div()
                        .size(px(PALETTE_ICON_PX))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::default().path(path).small().text_color(foreground))
                        .into_any_element()
                })
            };
            let icon_slot = has_icon_rail.then(|| {
                let slot = div()
                    .size(px(PALETTE_ICON_PX))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center();
                match icon_content {
                    Some(icon) => slot.child(icon).into_any_element(),
                    None => slot.into_any_element(),
                }
            });

            let hint = if item.hint.is_empty() {
                None
            } else {
                match item.hint_style {
                    WorkspacePaletteHintStyle::Metadata => Some(
                        div()
                            .max_w(px(PALETTE_METADATA_MAX_WIDTH))
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(muted)
                            .child(item.hint.clone())
                            .into_any_element(),
                    ),
                    WorkspacePaletteHintStyle::Shortcut => {
                        let keycaps = item
                            .hint
                            .split('+')
                            .filter(|key| !key.is_empty())
                            .map(|key| {
                                h_flex()
                                    .h(px(crate::display::ui::keycap::KEY_H))
                                    .px(px(space::S / 2.0))
                                    .items_center()
                                    .rounded(px(radius::CHIP))
                                    .border_1()
                                    .border_color(border)
                                    .bg(surface_bg)
                                    .font_family(mono_family.clone())
                                    .text_size(px(11.0))
                                    .text_color(muted)
                                    .child(key.to_owned())
                            })
                            .collect::<Vec<_>>();
                        Some(
                            h_flex()
                                .max_w(px(PALETTE_METADATA_MAX_WIDTH))
                                .gap(px(space::XXS))
                                .children(keycaps)
                                .into_any_element(),
                        )
                    },
                }
            };

            let action = item.action.clone();
            let context_target = self
                .shell_picker_open
                .then(|| launcher_menu::LauncherTarget::from_action(&action))
                .flatten();
            let row_tooltip = item.label.clone();
            let selected = ix == self.command_palette_selected;
            let hover_group = SharedString::from(format!("command-palette-row-hover-{ix}"));
            let row_content = h_flex()
                .id(SharedString::from(format!("command-palette-row-{ix}")))
                .debug_selector(move || format!("command-palette-row-{ix}"))
                .group(hover_group.clone())
                .h(px(PALETTE_ROW_HEIGHT))
                .flex_shrink_0()
                .w_full()
                .px_2()
                .when(results_scrollable, |row| row.pr(px(PALETTE_SCROLLBAR_CONTENT_GUTTER)))
                .gap(px(space::XS))
                .items_center()
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(row_tooltip.clone()).build(window, cx)
                })
                .when(selected, |row| row.bg(selected_bg))
                .when(!selected, |row| row.group_hover(hover_group.clone(), |row| row.bg(hover_bg)))
                .when_some(icon_slot, |row, icon| row.child(icon))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .line_height(relative(1.0))
                        .text_color(foreground)
                        .child(item.label),
                )
                .when_some(hint, |row, hint| row.child(hint))
                .when_some(context_target, |row, target| {
                    row.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.command_palette_selected = ix;
                            this.open_launcher_context_menu(
                                target.clone(),
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.command_palette_selected = ix;
                    this.run_workspace_palette_action(action.clone(), window, cx);
                }));

            rows.push(row_content.into_any_element());
        }

        if rows.is_empty() {
            rows.push(
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap(px(space::XS))
                    .text_color(muted)
                    .child(Icon::new(IconName::Search).small())
                    .child(
                        div()
                            .text_size(px(12.0))
                            .child(language.pick("没有匹配结果", "No matching results")),
                    )
                    .into_any_element(),
            );
        }

        let focused = self.command_palette_input.read(cx).focus_handle(cx).is_focused(window);
        let focus_line = window.use_keyed_state("workspace-palette-focus-line", cx, |_, _| {
            crate::gpui_shell::motion::FocusUnderline::default()
        });
        let underline = focus_line.update(cx, |line, _| {
            line.set_focused(focused);
            line.underline(ring)
        });
        // 搜索直接成为面板顶部，不再套一层灰色输入卡片；分组由留白而非长横线分隔。
        let search_box = h_flex()
            .debug_selector(|| "workspace-palette-search".to_owned())
            .relative()
            .w_full()
            .h(px(PALETTE_SEARCH_HEIGHT))
            .flex_shrink_0()
            .px(px(16.0))
            .gap(px(10.0))
            .border_b_1()
            .border_color(border.opacity(0.65))
            .child(Icon::new(IconName::Search).size(px(18.0)).text_color(muted))
            .child(
                Input::new(&self.command_palette_input)
                    .flex_1()
                    .min_w_0()
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .cleanable(true)
                    .text_size(px(15.0)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(6.0))
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(border)
                    .bg(surface_bg.opacity(0.45))
                    .font_family(mono_family.clone())
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child("esc"),
            )
            .child(underline);

        let filter_bar = palette_filters.map(|(selected_filter, counts)| {
            let selected_index =
                counts.iter().position(|(filter, _)| *filter == selected_filter).unwrap_or(0);
            let chips = counts
                .into_iter()
                .map(|(filter, count)| {
                    let selected = selected_filter == filter;
                    h_flex()
                        .id(SharedString::from(format!("workspace-palette-filter-{filter:?}")))
                        .debug_selector(move || format!("workspace-palette-filter-{filter:?}"))
                        .h(px(26.0))
                        .px_2()
                        .gap(px(space::XXS))
                        .items_center()
                        .cursor_pointer()
                        .rounded_full()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .when(!selected, |chip| chip.hover(|chip| chip.bg(hover_bg)))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(if selected { accent } else { foreground })
                                .when(selected, |label| label.font_weight(FontWeight::MEDIUM))
                                .child(filter.label(language)),
                        )
                        .child(
                            div()
                                .font_family(mono_family.clone())
                                .text_size(px(11.0))
                                .text_color(if selected { accent } else { muted })
                                .child(count.to_string()),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_workspace_palette_filter(filter, window, cx);
                        }))
                })
                .collect::<Vec<_>>();
            h_flex()
                .h(px(PALETTE_FILTER_BAR_HEIGHT))
                .flex_shrink_0()
                .w_full()
                .px(px(12.0))
                .pt(px(8.0))
                .items_center()
                .child(crate::gpui_shell::widgets::segmented::SegmentedRow::new(
                    if self.shell_picker_open { "launcher-filters" } else { "quick-jump-filters" },
                    selected_index,
                    h_flex().gap_1().children(chips),
                    div()
                        .rounded_full()
                        .border_1()
                        .border_color(accent.opacity(0.3))
                        .bg(accent.opacity(0.08)),
                ))
                .into_any_element()
        });

        let scroll_handle = self.command_palette_scroll.clone();
        let result_list = v_flex()
            .id("workspace-palette-results-scroll")
            .size_full()
            .gap(px(PALETTE_ROW_GAP))
            .px(px(8.0))
            .pb(px(8.0))
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .children(rows);
        let results =
            div().relative().flex_1().min_h_0().overflow_hidden().child(result_list).when(
                results_scrollable,
                |results| {
                    results.child(
                        gpui_component::scroll::Scrollbar::vertical(&scroll_handle)
                            .scrollbar_show(gpui_component::scroll::ScrollbarShow::Always),
                    )
                },
            );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .key_context(PALETTE_KEY_CONTEXT)
            .bg(overlay)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_command_palette(window, cx);
                }),
            )
            .on_key_down(cx.listener(|this: &mut Self, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "up" => {
                        this.move_command_palette_selection(-1, cx);
                        cx.stop_propagation();
                    },
                    "down" => {
                        this.move_command_palette_selection(1, cx);
                        cx.stop_propagation();
                    },
                    "escape" => {
                        this.close_command_palette(window, cx);
                        cx.stop_propagation();
                    },
                    _ => {},
                }
            }))
            .child(
                v_flex()
                    .w(px(PALETTE_PANEL_WIDTH))
                    .max_w_full()
                    .h(px(PALETTE_PANEL_HEIGHT))
                    .max_h_full()
                    .rounded(px(16.0))
                    .border_1()
                    .border_color(border)
                    .bg(panel_bg)
                    .shadow_lg()
                    .overflow_hidden()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .child(search_box)
                    .children(filter_bar)
                    .child(results),
            )
            .into_any_element()
    }
}
