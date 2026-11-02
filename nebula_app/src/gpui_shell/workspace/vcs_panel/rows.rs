//! Construct only the Git/SVN rows requested by the virtual list.
use super::list_model::{RowOps, VcsRow};
use super::*;

impl NebulaWorkspace {
    pub(in crate::gpui_shell::workspace) fn render_vcs_list(
        &self,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        let scroll = self.vcs_list.scroll.clone();
        let owner = cx.entity().downgrade();
        v_flex()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .child(
                gpui::list(scroll.clone(), move |index, window, cx| {
                    owner
                        .update(cx, |workspace, cx| workspace.render_vcs_row(index, window, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .w(px(16.0))
                    .child(gpui_component::scroll::Scrollbar::vertical(&scroll)),
            )
            .into_any_element()
    }

    pub(super) fn render_vcs_row(
        &mut self,
        visible_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        #[cfg(test)]
        self.vcs_list.rendered.push(visible_index);
        let Some(row) = self.vcs_list.row(visible_index).cloned() else {
            return div().into_any_element();
        };
        let history = matches!(&row, VcsRow::History { .. });
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hover = theme.list_hover;
        let selected_bg = theme.list_active;
        let lane_purple = gpui::Hsla {
            h: (theme.primary.h + 0.20) % 1.0,
            s: theme.primary.s.max(0.42),
            l: theme.primary.l,
            a: theme.primary.a,
        };
        let root = self.vcs_list.root().cloned();
        let selected = self.side_panel.selected.clone();
        let menu_target = cx.entity().downgrade();
        let element = match row {
            VcsRow::Heading { message, count } => h_flex()
                .h(px(26.0))
                .px_2()
                .items_center()
                .text_xs()
                .text_color(muted)
                .child(language.text(message.unwrap_or(Message::VcsChanges)))
                .child(div().ml_2().child(count.to_string()))
                .into_any_element(),
            VcsRow::Empty(message) => div()
                .py_2()
                .px_2()
                .text_sm()
                .text_color(muted)
                .child(language.text(message))
                .into_any_element(),
            VcsRow::History { index, commit, graph } => {
                let (lane_width, lane_spacing) = self.vcs_list.lane_layout();
                let graph_cell = git_lane_canvas(
                    graph,
                    lane_width,
                    lane_spacing,
                    [theme.primary, theme.success, lane_purple, theme.warning],
                    theme.popover,
                    &commit,
                );
                let refs = git_ref_labels(&commit.decorations)
                    .into_iter()
                    .take(2)
                    .map(|git_ref| {
                        let color = match git_ref.kind {
                            GitRefKind::Head | GitRefKind::Local => theme.primary,
                            GitRefKind::Remote => lane_purple,
                            GitRefKind::Tag => theme.warning,
                        };
                        div()
                            .h(px(15.0))
                            .max_w(px(72.0))
                            .flex_shrink_0()
                            .px(px(4.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(color.opacity(0.38))
                            .bg(color.opacity(0.12))
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(theme.font_size - px(3.0))
                            .text_color(color)
                            .child(git_ref.label)
                            .into_any_element()
                    })
                    .collect::<Vec<_>>();
                let meta = format!(
                    "{} · {} · {}",
                    commit.author,
                    git_relative_time(commit.timestamp, language),
                    commit.short_hash
                );
                h_flex()
                    .id(SharedString::from(format!("git-history-{index}")))
                    .debug_selector(move || format!("git-history-{index}"))
                    .w_full()
                    .h(px(46.0))
                    .px_2()
                    .items_start()
                    .hover(|row| row.bg(hover))
                    .child(graph_cell)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .justify_center()
                            .gap_1()
                            .child(h_flex().min_w_0().gap(px(4.0)).children(refs).child(
                                div().min_w_0().truncate().text_sm().child(commit.subject.clone()),
                            ))
                            .child(
                                div()
                                    .truncate()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(theme.font_size - px(2.5))
                                    .text_color(muted)
                                    .child(meta),
                            ),
                    )
                    .into_any_element()
            },
            VcsRow::Change { section_id, index, status, relative_path, ops } => {
                let is_git = ops != RowOps::Svn;
                let status = &status;
                let relative_path = &relative_path;
                // 远端快照：写操作改走远端 exec、双击开远端文档、本地专属
                // 入口（merge 编辑器、SVN 菜单/对话框）不画。行选中（本地
                // 比较 key，自洽）保持可用。
                let remote = self.vcs_list.remote;
                let remote_doc =
                    remote.then(|| self.remote_git.document_target(relative_path)).flatten();
                // 本地专属动作的剩余门控（远端形态全部为 false）。
                let local_only = !remote;
                let discard_confirm = self.vcs_discard_confirm.clone();
                let path = root
                    .as_ref()
                    .map(|root| root.join(relative_path))
                    .unwrap_or_else(|| std::path::PathBuf::from(relative_path));
                let selected_row = selected.as_ref() == Some(&path);
                let status_color = match status {
                    'A' | '?' => theme.success,
                    'D' | '!' => theme.danger,
                    'C' | 'U' => theme.danger,
                    _ => theme.warning,
                };
                // 路径拆分显示：文件名主体 + 灰色父目录。
                let (file_name, parent) = match relative_path.rfind('/') {
                    Some(pos) => {
                        (relative_path[pos + 1..].to_owned(), relative_path[..pos].to_owned())
                    },
                    None => (relative_path.clone(), String::new()),
                };
                let row_group = SharedString::from(format!("vcs-row-actions-{section_id}-{index}"));
                let open_path = path.clone();
                let stage_path = relative_path.clone();
                let svn_add_path = relative_path.clone();
                let unstage_path = relative_path.clone();
                let discard_path = relative_path.clone();
                let resolve_path = relative_path.clone();
                let diff_path = relative_path.clone();
                let menu_path = relative_path.clone();
                let merge_button_path = relative_path.clone();
                let merge_open_path = relative_path.clone();
                let discard_armed = discard_confirm.as_deref() == Some(relative_path.as_str());
                let git_discard = ops == RowOps::Unstaged && *status != '?';
                let svn_revert = ops == RowOps::Svn && !matches!(*status, '?' | 'C');
                let can_discard = git_discard || svn_revert;
                let svn_add = ops == RowOps::Svn && *status == '?';
                let svn_resolve = ops == RowOps::Svn && *status == 'C';
                let svn_diff = ops == RowOps::Svn && !matches!(*status, '?' | '!');
                h_flex()
                    .id(SharedString::from(format!(
                        "git-tree-row-{section_id}-{index}-{relative_path}"
                    )))
                    .debug_selector(|| format!("git-tree-row-{section_id}-{index}-{relative_path}"))
                    .group(row_group.clone())
                    .h(px(30.0))
                    .w_full()
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_md()
                    .when(selected_row, |row| row.bg(selected_bg))
                    .hover(|row| row.bg(hover))
                    .child(
                        div()
                            .w(px(14.0))
                            .flex_shrink_0()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(theme.font_size - px(2.0))
                            .text_center()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(status_color)
                            .child(status.to_string()),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .items_center()
                            .child(
                                // 文件名与父目录各自截断（用户 10-23）：真被
                                // 截断才悬浮展示全文——探针在 layout 时比对
                                // 全文固有宽与实际可用宽，tooltip 延迟到期
                                // 才读取；未截断返回空视图，等效没有提示。
                                // 行内按钮的提示不受影响（独立元素）。
                                Self::truncation_reveal(
                                    &format!("vcs-file-name-{section_id}-{index}"),
                                    file_name.clone(),
                                    window.text_style().font(),
                                    theme.font_size - px(0.5),
                                    "vcs-file-tip",
                                )
                                .font_weight(FontWeight::NORMAL),
                            )
                            .when(!parent.is_empty(), |line| {
                                line.child(
                                    Self::truncation_reveal(
                                        &format!("vcs-parent-{section_id}-{index}"),
                                        parent.clone(),
                                        gpui::Font {
                                            family: theme.mono_font_family.clone(),
                                            ..window.text_style().font()
                                        },
                                        theme.font_size - px(2.5),
                                        "vcs-path-tip",
                                    )
                                    .font_family(theme.mono_font_family.clone())
                                    .text_color(muted),
                                )
                            }),
                    )
                    .when(can_discard, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "vcs-discard-{section_id}-{index}"
                            )))
                            .map(|button| {
                                if discard_armed {
                                    button
                                        .label(if svn_revert {
                                            language.text(Message::VcsSvnConfirmRevert)
                                        } else {
                                            language.text(Message::VcsConfirmDiscard)
                                        })
                                        .danger()
                                        .xsmall()
                                } else {
                                    button.icon(IconName::Undo2).ghost().xsmall().tooltip(
                                        if svn_revert {
                                            language.text(Message::VcsSvnRevert)
                                        } else {
                                            language.text(Message::VcsDiscard)
                                        },
                                    )
                                }
                            })
                            .when(!discard_armed, |button| {
                                button
                                    .invisible()
                                    .group_hover(row_group.clone(), |button| button.visible())
                            })
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if this.vcs_discard_confirm.as_deref()
                                        == Some(discard_path.as_str())
                                    {
                                        this.vcs_discard_confirm = None;
                                        if this.vcs_list.remote {
                                            this.remote_git_discard(&discard_path, cx);
                                        } else if svn_revert {
                                            this.side_panel.svn_revert_path(&discard_path);
                                        } else {
                                            this.side_panel.git_discard_path(&discard_path);
                                        }
                                    } else {
                                        this.vcs_discard_confirm = Some(discard_path.clone());
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(ops == RowOps::Unstaged && is_git, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "vcs-stage-{section_id}-{index}"
                            )))
                            .icon(IconName::Plus)
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::VcsStage))
                            .invisible()
                            .group_hover(row_group.clone(), |button| button.visible())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.vcs_discard_confirm = None;
                                    if this.vcs_list.remote {
                                        this.remote_git_stage(&stage_path, cx);
                                    } else {
                                        this.side_panel.git_stage_path(&stage_path);
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(local_only && svn_add, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "svn-add-{section_id}-{index}"
                            )))
                            .icon(IconName::Plus)
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::VcsSvnAddFile))
                            .invisible()
                            .group_hover(row_group.clone(), |button| button.visible())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.vcs_discard_confirm = None;
                                    this.side_panel.svn_add_path(&svn_add_path);
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(local_only && svn_resolve, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "svn-resolve-{section_id}-{index}"
                            )))
                            .label(language.text(Message::VcsSvnResolve))
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::VcsSvnMarkResolved))
                            .invisible()
                            .group_hover(row_group.clone(), |button| button.visible())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.vcs_discard_confirm = None;
                                    this.side_panel.svn_resolve_path(&resolve_path);
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(local_only && ops == RowOps::Svn, |row| {
                        // 这一行的完整 SVN 操作集（日志、blame、锁、
                        // 忽略、改名、删除、冲突、属性）都在这个菜单里，
                        // 行内只多一个 ⋯ 位。
                        row.child(Self::svn_row_menu(
                            &menu_target,
                            &menu_path,
                            index,
                            section_id,
                            language,
                        ))
                    })
                    .when(ops == RowOps::Staged, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "vcs-unstage-{section_id}-{index}"
                            )))
                            .icon(IconName::Minus)
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::VcsUnstage))
                            .invisible()
                            .group_hover(row_group.clone(), |button| button.visible())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.vcs_discard_confirm = None;
                                    if this.vcs_list.remote {
                                        this.remote_git_unstage(&unstage_path, cx);
                                    } else {
                                        this.side_panel.git_unstage_path(&unstage_path);
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(local_only && ops == RowOps::Conflict && is_git, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "git-resolve-{section_id}-{index}"
                            )))
                            .icon(
                                Icon::new(Icon::empty())
                                    .path(crate::gpui_shell::assets::nav::VCS_CONFLICT),
                            )
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::VcsResolveInMergeEditor))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_git_merge_tab(merge_button_path.clone(), window, cx);
                                },
                            )),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.side_panel.selected = Some(path.clone());
                        cx.notify();
                    }))
                    .on_double_click(cx.listener(move |this, _, window, cx| {
                        if let Some((destination, path)) = &remote_doc {
                            // 只读形态（远端）：打开远端文档，不碰本地文件系统。
                            this.open_remote_document(
                                destination.clone(),
                                path.clone(),
                                window,
                                cx,
                            );
                        } else if ops == RowOps::Conflict && is_git {
                            this.open_git_merge_tab(merge_open_path.clone(), window, cx);
                        } else if !svn_diff || !this.side_panel.svn_diff_path(&diff_path) {
                            // Git 与未版本化 SVN 文件仍走现有文档路由。
                            this.open_document_path(open_path.clone(), window, cx);
                        }
                    }))
                    .into_any_element()
            },
        };
        // 历史行必须首尾相接；通用的 4px 行底留白会把线路切成一段段。
        div().w_full().when(!history, |row| row.pb_1()).child(element).into_any_element()
    }

    /// 「真被截断才悬浮展示全文」的文本元素：`truncate()` 的 div 内嵌截断
    /// 探针（layout 时拿实际可用宽比对全文固有宽），悬浮提示在延迟到期
    /// 才读探针结果——未截断时返回空视图，等效没有提示（用户 10-23
    /// 需求）。`font`/`font_size` 必须与该 div 最终渲染用的完全一致，探针
    /// 才量得准；行内其余样式（字重、颜色、字体族）由调用方继续追加。
    fn truncation_reveal(
        element_key: &str,
        text: String,
        font: gpui::Font,
        font_size: gpui::Pixels,
        tip_selector: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        let (truncated, probe) =
            tab_presentation::truncation_probe(text.clone().into(), font, font_size);
        let tip_text: SharedString = text.clone().into();
        let selector_source = element_key.to_owned();
        div()
            .id(SharedString::from(element_key.to_owned()))
            .debug_selector(move || selector_source.clone())
            .min_w_0()
            .truncate()
            .text_size(font_size)
            .tooltip(move |window, cx| {
                if truncated.get() {
                    tab_presentation::tooltip_with_selector(
                        tip_text.clone(),
                        tip_selector,
                        window,
                        cx,
                    )
                } else {
                    // 空视图什么都不画；这里的行没有行级 tooltip，不会占位
                    // 压制别的提示（侧栏分支 chip 才需要 build 时不挂）。
                    cx.new(|_| gpui::Empty).into()
                }
            })
            .child(text)
            .child(probe)
    }
}
