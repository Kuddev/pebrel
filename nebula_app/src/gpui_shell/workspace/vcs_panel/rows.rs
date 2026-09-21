//! Construct only the Git/SVN rows requested by the virtual list.
use super::list_model::{RowOps, VcsRow};
use super::*;

impl NebulaWorkspace {
    pub(super) fn render_vcs_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        #[cfg(test)]
        self.vcs_list.rendered.push(visible_index);
        let Some(row) = self.vcs_list.row(visible_index).cloned() else {
            return div().into_any_element();
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hover = theme.list_hover;
        let selected_bg = theme.list_active;
        let symbol: SharedString = crate::font_install::REQUIRED_FONT_FAMILY.into();
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
                .child(message.map(|m| language.text(m)).unwrap_or("修改"))
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
                            .text_size(px(9.5))
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
                            .child(div().truncate().text_xs().text_color(muted).child(meta)),
                    )
                    .into_any_element()
            },
            VcsRow::Change { section_id, index, status, relative_path, ops } => {
                let is_git = ops != RowOps::Svn;
                let status = &status;
                let relative_path = &relative_path;
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
                            .font_family(symbol.clone())
                            .text_sm()
                            .text_color(status_color)
                            .child(status.to_string()),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .items_center()
                            .child(div().flex_shrink_0().text_sm().child(file_name.clone()))
                            .when(!parent.is_empty(), |line| {
                                line.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(parent.clone()),
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
                                            "确认还原"
                                        } else {
                                            language.text(Message::VcsConfirmDiscard)
                                        })
                                        .danger()
                                        .xsmall()
                                } else {
                                    button.icon(IconName::Undo2).ghost().xsmall().tooltip(
                                        if svn_revert {
                                            "还原 SVN 改动"
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
                                        if svn_revert {
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
                                    this.side_panel.git_stage_path(&stage_path);
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(svn_add, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "svn-add-{section_id}-{index}"
                            )))
                            .icon(IconName::Plus)
                            .ghost()
                            .xsmall()
                            .tooltip("添加到 SVN")
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
                    .when(svn_resolve, |row| {
                        row.child(
                            Button::new(SharedString::from(format!(
                                "svn-resolve-{section_id}-{index}"
                            )))
                            .label("解决")
                            .ghost()
                            .xsmall()
                            .tooltip("保留当前内容并标记冲突已解决")
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
                    .when(ops == RowOps::Svn, |row| {
                        // 这一行的完整 SVN 操作集（日志、blame、锁、
                        // 忽略、改名、删除、冲突、属性）都在这个菜单里，
                        // 行内只多一个 ⋯ 位。
                        row.child(Self::svn_row_menu(&menu_target, &menu_path, index, section_id))
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
                                    this.side_panel.git_unstage_path(&unstage_path);
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(ops == RowOps::Conflict && is_git, |row| {
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
                        if ops == RowOps::Conflict && is_git {
                            this.open_git_merge_tab(merge_open_path.clone(), window, cx);
                        } else if !svn_diff || !this.side_panel.svn_diff_path(&diff_path) {
                            // Git 与未版本化 SVN 文件仍走现有文档路由。
                            this.open_document_path(open_path.clone(), window, cx);
                        }
                    }))
                    .into_any_element()
            },
        };
        div().w_full().pb_1().child(element).into_any_element()
    }
}
