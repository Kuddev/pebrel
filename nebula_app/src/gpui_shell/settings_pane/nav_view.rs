//! 设置导航：组名 + 当前大类原地展开子项。
//!
//! 导航与正文共用底色。选中项用与分组卡同材质的底色，hover 一律中性；
//! 强调色用于选中项的图标和子项指示条。
//! 子项只在其大类处于选中状态时展开，同一时间最多一棵；搜索时收起全部子项
//! 并隐去组名，结果按命中顺序平铺。

use super::*;

/// 子项缩进：竖向引导线对齐大类图标的中心。
const CHILD_INDENT: f32 = 20.0;
const CHILD_GUIDE_X: f32 = 18.0;

impl SettingsPane {
    pub(super) fn render_nav(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui::IntoElement as _;
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let foreground = theme.foreground;
        let accent = theme.primary;
        let hover_bg = crate::gpui_shell::theme::neutral_hover(cx);
        let raised = crate::gpui_shell::theme::raised_fill(cx);
        let raised_shadow = crate::gpui_shell::theme::raised_shadow(cx);
        let guide = crate::gpui_shell::theme::settings_hairline(cx);
        let radius = crate::gpui_shell::theme::card_radius(cx);
        // 返回入口和分类共用紧凑菜单尺寸；字号与搜索框一致，不继承终端配置字号。
        let nav = v_flex()
            .w(px(SETTINGS_NAV_WIDTH))
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .bg(crate::gpui_shell::theme::settings_canvas_tint(cx))
            .rounded_l(radius)
            .px(px(8.0))
            .pt(px(12.0))
            .pb(px(8.0))
            .gap(px(2.0))
            .text_sm()
            .line_height(px(20.0))
            .child(
                div()
                    .id("settings-back")
                    .debug_selector(|| "settings-back".into())
                    .mb(px(10.0))
                    .h(px(SETTINGS_NAV_ROW_HEIGHT))
                    .flex_shrink_0()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |item| item.bg(hover_bg).text_color(foreground))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsPaneEvent::Close)))
                    .child(Icon::new(IconName::ArrowLeft).size(px(SETTINGS_NAV_ICON_SIZE)))
                    .child(language.pick("返回工作区", "Back to workspace")),
            )
            .child(self.render_nav_search(window, cx));

        let mut sections = v_flex()
            .id("settings-nav-scroll")
            .debug_selector(|| "settings-nav-scroll".into())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.nav_scroll)
            .pb(px(8.0))
            .gap(px(4.0));

        let searching = !self.settings_search_input.read(cx).value().trim().is_empty();
        let matching = self.matching_settings_sections(cx);
        let active_child = self.visible_nav_child(cx);
        let ordered: Vec<(Option<&'static str>, usize)> = if searching {
            matching.iter().map(|ix| (None, *ix)).collect()
        } else {
            NAV_GROUPS
                .iter()
                .flat_map(|(key, sections)| sections.iter().map(move |ix| (Some(*key), *ix)))
                .filter(|(_, ix)| matching.contains(ix))
                .collect()
        };
        let mut previous_group = None;
        for (group, ix) in ordered {
            if group.is_some() && group != previous_group {
                sections = sections.child(
                    div()
                        .flex_shrink_0()
                        .px(px(14.0))
                        .pt(px(if previous_group.is_some() { 16.0 } else { 4.0 }))
                        .pb(px(4.0))
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(faint)
                        .child(nav_group_label(group.unwrap_or_default(), language)),
                );
            }
            previous_group = group;
            let children = if searching { &[][..] } else { nav_children(ix) };
            let selected = ix == self.active_section;
            // 有子项的大类在展开后由子项承担“选中”，大类自身只加深文字。
            let raised_row = selected && children.is_empty();
            sections = sections.child(
                div()
                    .id(("settings-nav", ix))
                    .debug_selector(move || format!("settings-nav-{ix}"))
                    .px(px(10.0))
                    .h(px(SETTINGS_NAV_ROW_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded_md()
                    .cursor_pointer()
                    // 底色和墨色表达选中，避免中文导航因字重匹配而突然变粗。
                    .font_weight(gpui::FontWeight::NORMAL)
                    .when(raised_row, |item| {
                        item.bg(raised).shadow(raised_shadow.clone()).text_color(foreground)
                    })
                    .when(selected && !raised_row, |item| {
                        item.text_color(foreground).hover(|s| s.bg(hover_bg))
                    })
                    .when(!selected, |item| item.text_color(muted).hover(|s| s.bg(hover_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let children = nav_children(ix);
                        if this.active_section != ix || this.visible_nav_child(cx).is_none() {
                            this.active_nav_child = children.first().copied();
                        }
                        this.active_section = ix;
                        this.about_sponsor_open = false;
                        cx.notify();
                    }))
                    .child(
                        Icon::default()
                            .path(section_icon(ix))
                            .size(px(SETTINGS_NAV_ICON_SIZE))
                            .flex_shrink_0()
                            .text_color(if raised_row { accent } else { muted }),
                    )
                    .child(div().flex_1().min_w_0().child(section_label(ix, language)))
                    .when(!children.is_empty(), |item| {
                        item.child(
                            Icon::new(if selected { IconName::ChevronDown } else { IconName::ChevronRight })
                                .size(px(12.0))
                                .text_color(faint),
                        )
                    }),
            );
            if selected && !children.is_empty() {
                sections = sections.child(self.render_nav_children(
                    ix,
                    children,
                    active_child,
                    (foreground, muted, accent, hover_bg, guide),
                    cx,
                ));
            }
        }
        nav.child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(sections)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right(px(-8.0))
                        .bottom_0()
                        .w(px(16.0))
                        .child(
                            gpui_component::scroll::Scrollbar::vertical(&self.nav_scroll)
                                .scrollbar_show(gpui_component::scroll::ScrollbarShow::Always),
                        ),
                ),
        )
            .child(
                Button::new("settings-restore-defaults")
                    .debug_selector(|| "settings-restore-defaults".into())
                    .icon(IconName::Undo2)
                    .label(language.pick("恢复默认设置", "Restore defaults"))
                    .ghost()
                    .small()
                    .w_full()
                    .h(px(SETTINGS_NAV_ROW_HEIGHT))
                    .flex_shrink_0()
                    .mt(px(8.0))
                    .justify_start()
                    .px_3()
                    .text_color(muted)
                    .tooltip(language.pick("恢复设置与快捷键；保留 SSH 主机、凭据和历史，并备份原设置。", "Restores settings and shortcuts. Keeps SSH hosts, credentials and history, and backs up current settings."))
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_reset_all_settings(window, cx))),
            )
            .into_any_element()
    }

    fn render_nav_children(
        &self,
        section: usize,
        children: &'static [NavChild],
        active: Option<NavChild>,
        (foreground, muted, accent, hover_bg, guide): (Hsla, Hsla, Hsla, Hsla, Hsla),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let mut list = v_flex()
            .relative()
            .flex_shrink_0()
            .pl(px(CHILD_INDENT))
            .py(px(2.0))
            .gap(px(2.0))
            .child(
                div()
                    .absolute()
                    .left(px(CHILD_GUIDE_X))
                    .top(px(4.0))
                    .bottom(px(4.0))
                    .w(px(1.0))
                    .bg(guide),
            );
        for (order, child) in children.iter().copied().enumerate() {
            let selected = active == Some(child);
            list = list.child(
                div()
                    .id(("settings-nav-child", section * 16 + order))
                    .debug_selector(move || format!("settings-nav-child-{section}-{order}"))
                    .relative()
                    .h(px(SETTINGS_NAV_CHILD_HEIGHT))
                    .flex_shrink_0()
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(if selected { foreground } else { muted })
                    .when(selected, |item| {
                        item.bg(hover_bg).child(
                            div()
                                .absolute()
                                .left(px(CHILD_GUIDE_X - CHILD_INDENT - 0.5))
                                .top(px(7.0))
                                .bottom(px(7.0))
                                .w(px(2.0))
                                .rounded(px(1.0))
                                .bg(accent),
                        )
                    })
                    .when(!selected, |item| item.hover(|s| s.bg(hover_bg).text_color(foreground)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active_section = section;
                        this.active_nav_child = Some(child);
                        cx.notify();
                    }))
                    .child(child.label(language)),
            );
        }
        list
    }
}
