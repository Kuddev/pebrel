//! `ai_agent_sidebar` 开启时的分组树侧栏。`sidebar.rs` 与上游 main 保持
//! 逐字节一致（合并上游时零冲突）；本文件持有全部分支侧栏渲染差异：
//! 行模型渲染（`task_groups::SidebarRow` → 元素树）、分区/组头、状态
//! 徽章文案与 agent 域过滤、分组树的 Ready 静息语义。运行时开关的
//! 分派点在 `sidebar.rs` 尾部的 `render_sidebar_dispatch`——那是该文件
//! 仅有的分支差异（追加 impl 块 + 两处内部调用点改走分派）。

use super::sidebar::sidebar_new_tab_control;
use super::task_groups::SidebarRow;
use super::*;
use crate::i18n::Message;

/// 折叠箭头的固定布局槽。图标是 SVG，不应借任一字体的 advance 决定留白。
/// 「最近任务」分区头复用同一槽宽，两条分区标题的箭头纵向对齐。
/// （`sidebar.rs` 另有上游的同名私有常量；此处独立持有一份，两处值
/// 必须同步改。）
pub(super) const TABS_DISCLOSURE_SLOT_W: f32 = 24.0;

fn activity_status_label(
    activity: SidebarActivity,
    language: &crate::display::UiLanguage,
) -> Option<&'static str> {
    match activity {
        SidebarActivity::Running => Some(language.text(Message::SidebarStatusRunning)),
        SidebarActivity::Paused => Some(language.text(Message::SidebarStatusPaused)),
        SidebarActivity::Done | SidebarActivity::Completed => {
            Some(language.text(Message::SidebarStatusReady))
        },
        SidebarActivity::WaitingInput => Some(language.text(Message::SidebarStatusWaitingInput)),
        SidebarActivity::Attention => Some(language.text(Message::SidebarStatusAsk)),
        SidebarActivity::CommandFailed => Some(language.text(Message::SidebarStatusFailed)),
        SidebarActivity::Failed => Some(language.text(Message::SidebarStatusStopped)),
        SidebarActivity::Idle => None,
    }
}

/// 文字与徽章共用同一语义色：形状（或动效）表达状态，颜色表达严重度，
/// 文字把语义补全到不用猜的程度。
fn activity_label_color(activity: SidebarActivity, theme: &gpui_component::Theme) -> gpui::Hsla {
    match activity {
        SidebarActivity::Running => theme.warning,
        SidebarActivity::Paused => theme.warning,
        SidebarActivity::Done => theme.primary,
        SidebarActivity::Completed => theme.success,
        SidebarActivity::CommandFailed => theme.danger,
        SidebarActivity::WaitingInput => theme.primary,
        SidebarActivity::Attention => theme.warning,
        SidebarActivity::Failed => theme.danger,
        SidebarActivity::Idle => theme.muted_foreground,
    }
}

/// agent 侧栏模式下状态徽章只属于 AI 会话（用户 10-14 裁定）：普通终端
/// 的常规状态回落 `Idle`（无点无文字，空闲短标照旧）；`Failed` 是 pane 级
/// 故障（SSH 断连/进程没了），挂掉的连接必须看得见，保留例外。只被分组
/// 条目行消费——顶栏走未过滤的 `TabPresentation::activity`，不受影响。
fn agent_scoped_activity(activity: SidebarActivity, is_agent: bool) -> SidebarActivity {
    if is_agent || activity == SidebarActivity::Failed { activity } else { SidebarActivity::Idle }
}

/// 事件 vs 状态：分组树模式下静息徽章的取舍（`tab_presentation` 按开关键
/// 择本函数与 `sidebar::resting_activity`；关 = 上游经典语义「完成是事件、
/// 当前 tab 不画」）。
///
/// - `Done`/`Completed` 在状态标签体系下读作「回合结束、Ready 待命」——这是
///   **状态**，当前 tab 也照画（用户 10-10 裁定：高亮行的 agent 状态不得回落
///   成 shell 短标「pwsh」）。
/// - 后台响铃补位到 `Done` 只对非当前 tab 成立。`has_bell` 本身激活即清
///   （见 `TabMeta::has_bell`），这里再挡一次是因为清除发生在激活事件里，同一帧
///   内读到的可能还是旧值。
/// - 其余都是**状态**（转圈、等输入、等授权、失败），描述「此刻仍然如此」，你看
///   不看它都还成立，所以当前 tab 也照画。响铃补位的 `Idle` 前提保证了它不会
///   盖掉这些。
pub(super) fn tree_resting_activity(
    activity: SidebarActivity,
    active: bool,
    has_bell: bool,
) -> SidebarActivity {
    match activity {
        SidebarActivity::Idle if !active && has_bell => SidebarActivity::Done,
        other => other,
    }
}

impl NebulaWorkspace {
    /// 分组模式的新终端继承 SSH 项目；经典模式保留原本的本地启动策略。
    pub(super) fn add_grouped_ssh_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if crate::gpui_shell::config::ai_agent_sidebar(cx)
            && let Some((destination, cwd)) = self.focused_ssh_origin(cx)
        {
            self.add_ssh_terminal_at(destination, cwd, window, cx);
            true
        } else {
            false
        }
    }

    /// SSH 聚焦任务的「新建终端」来源（用户 10-13 裁定）：同主机 + 当前
    /// 远端任务目录。非 SSH 聚焦任务返回 `None` 走本地路径；远端 cwd 尚
    /// 未上报时仍继承主机（`None` 目录落远端默认目录），与
    /// [`Self::duplicate_tab`] 的 SSH 分支同一 `remote_cwd` 口径。
    pub(super) fn focused_ssh_origin(&self, cx: &gpui::App) -> Option<(String, Option<String>)> {
        let view = self.tabs.get(self.active).and_then(super::WorkspaceTab::focused_view)?;
        let view = view.read(cx);
        Some((view.ssh_destination.clone()?, view.remote_cwd()))
    }
    pub(super) fn render_sidebar_tree(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row_pitch = self.tab_row_pitch();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let active_bg = theme.sidebar_accent;
        let active_fg = theme.sidebar_accent_foreground;
        let hover_bg = theme.list_hover;
        let dark = theme.is_dark();
        // 标题走 UI sans，路径/状态短标才用 Normal 等宽；有图标时才占图标槽。
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let tab_close_visible = settings.map(|settings| settings.tab_close_visible).unwrap_or(true);
        let chrome_family = theme.font_family.clone();
        let metadata_family = theme.mono_font_family.clone();
        let symbol_family: SharedString = crate::font_install::REQUIRED_FONT_FAMILY.into();
        // 界面字号独立于终端缩放。
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        // 分组条目行高与副行带：行高由密度+界面字号派生（两行副行槽固定
        // 预留，见 `task_groups::sidebar_entry_height` 的取舍说明）。
        let metrics = self.row_metrics(cx);
        let entry_h = task_groups::sidebar_entry_height(&metrics);
        let subline_line_h = task_groups::subline_line_height(&metrics);
        let subline_font_px = task_groups::subline_font_px(&metrics);
        let subline_h = 2.0 * subline_line_h + 2.0;
        let language = crate::gpui_shell::config::ui_language(cx);
        let new_session = language.text(Message::SidebarNewSession);

        // 受约束拖拽的渲染参数：激活后被拖行骑指针位移，落点槽位由位移换算。
        let drag = self
            .tab_drag
            .as_ref()
            .filter(|d| d.active && d.axis == TabDragAxis::Vertical)
            .map(|d| (d.source, Self::drag_slot(d, self.tabs.len()), d.offset));
        let items_running = std::cell::Cell::new(false);

        // 统一行模型（两个分区头 + 组头 + 条目）与变节距窗口：render 路径
        // 只做内存推导。
        let rows = self.sidebar_list_rows(cx);
        let (heights, offsets) = self.rows_geometry(&rows, cx);
        let row_window = self.rows_window(&rows, &heights);
        let bar = self.tabs_overlay_bar(&heights, &offsets, &row_window);
        // 目录树线：坐标相对可见窗口首行；视口未量到时按无限高画（首帧
        // 全显，窗口化不会裁掉线段）。
        let guides = task_groups::tree_guides(
            &rows,
            &offsets,
            offsets.get(row_window.scroll).copied().unwrap_or(0.0),
            if self.tabs_viewport_h >= 1.0 { self.tabs_viewport_h } else { f32::INFINITY },
            &metrics,
        );
        // 行的确定宽度：侧栏宽 − 侧栏 p_2 两边 − 列表右侧滚动条留白。
        // 与下面 `label_avail` 同一份减法口径，两者不能各算一套。
        let row_w = (self.sidebar_width - 16.0 - tab_scroll::TAB_SCROLL_GUTTER).max(1.0);
        let items = rows
            .iter()
            .enumerate()
            .filter(|(row_ix, _)| {
                task_groups::row_index_visible(*row_ix, row_window.scroll, row_window.show)
            })
            .map(|(row_ix, row)| match row {
                SidebarRow::TabsSectionHeader { count, collapsed } => self
                    .render_tabs_section_header(row_w, *count, *collapsed, cx)
                    .into_any_element(),
                SidebarRow::HostHeader { key, name, tabs, collapsed } => self
                    .render_ssh_host_header(row_w, row_ix, key, name, *tabs, *collapsed, cx)
                    .into_any_element(),
                SidebarRow::GroupHeader { key, name, branch, tabs, collapsed } => self
                    .render_group_header(row_w, row_ix, key, name, branch, *tabs, *collapsed, cx)
                    .into_any_element(),
                SidebarRow::Tab { ix } => {
                let ix = *ix;
                let active = ix == self.active;
                let TabPresentation {
                    title: _,
                    headline,
                    tooltip,
                    is_settings,
                    activity,
                    logo_image,
                    logo_pending,
                    program_glyph,
                    shell_tag,
                    color: tab_color,
                    renaming,
                    pane_count,
                    local_administrator,
                    is_agent_session,
                } = self.tab_presentation(ix, cx, dark);
                let hover_group: SharedString = format!("sidebar-tab-hover-{ix}").into();
                let has_program_glyph = program_glyph.is_some();
                // 状态徽章只属于 AI 会话：普通终端的常规状态（转圈/完成/
                // 等输入等）一律不画，只有 pane 级故障（SSH 断连/进程没了，
                // ✕ Stopped）例外——挂掉的连接不能看不见。被滤成 `Idle` 的
                // tab 状态槽自然回落 shell 短标。
                let activity = agent_scoped_activity(activity, is_agent_session);
                // 状态槽宽度：文字徽章按「文字 + 图标 + 间隔」预留（UI 文字
                // 走 chrome sans）；无文字时仍按 shell 短标测量（路径类短标
                // 走 metadata 等宽）。两套互斥，Idle 不画文字。
                let status_label = activity_status_label(activity, &language);
                let status_width = match status_label {
                    Some(label) => Self::shell_status_width(
                        window,
                        Some(&SharedString::from(label)),
                        &chrome_family,
                        label_px * SIDEBAR_TAG_SCALE,
                        TAB_STATUS_SLOT_W,
                        row_w * 0.45,
                    ) + TAB_LABEL_ICON_W + 4.0,
                    None => Self::shell_status_width(
                        window,
                        shell_tag.as_ref(),
                        &metadata_family,
                        label_px * SIDEBAR_TAG_SCALE,
                        TAB_STATUS_SLOT_W,
                        row_w * 0.45,
                    ),
                }
                .min(row_w * 0.45);
                let cross_window_drag = self.cross_window_drag_payload(ix, cx);
                // 用户明确设置过的标签色：行左侧一条竖光条（上下各留 7、宽 2.5，
                // 与旧壳 strip 同源）。默认标签不占这层视觉层级。
                //
                // 光条贴住行左缘，不沿用旧壳的左内缩 4：旧壳内容起点是 14px，这里
                // 是 `px_2` = 0.5rem，rem 跟随界面字号（默认 14 → 7px，最小 10 →
                // 5px）。内缩会让光条贴上图标；贴边时间隙就是内边距减光条宽，随
                // 字号一起缩放。
                let strip = tab_color.map(|color| gpui::Rgba {
                    r: color.r as f32 / 255.0,
                    g: color.g as f32 / 255.0,
                    b: color.b as f32 / 255.0,
                    a: 1.0,
                });
                // 副行「摘要 · 时刻」：任务标题与 tooltip 同一过滤
                // （`task_summary`），普通 shell 无任务标题时回落「新会话」。
                let (task_summary_row, spawned_at_row) = match self.tabs[ix].focused_view() {
                    Some(view) => {
                        let view = view.read(cx);
                        (view.task_summary().map(str::to_owned), Some(view.spawned_at()))
                    },
                    None => (None, None),
                };
                let subline: SharedString = task_groups::task_subline_label(
                    task_summary_row.as_deref(),
                    spawned_at_row,
                    new_session,
                )
                .into();
                let status_color = if active { active_fg } else { muted };
                let resting_status: Option<gpui::AnyElement> = match activity {
                    SidebarActivity::Running => {
                        items_running.set(true);
                        let (track, head) =
                            crate::gpui_shell::theme::sidebar_spinner_colors(cx, active);
                        Some(Self::spinner(self.spinner_phase, track, head).into_any_element())
                    },
                    SidebarActivity::Paused => Some(
                        Icon::new(IconName::Pause)
                            .xsmall()
                            .text_color(theme.warning)
                            .into_any_element(),
                    ),
                    // 回合完成、等下一条指令：旧壳蓝点语义——不转圈，留一个
                    // 「有结果没看」的痕迹。
                    SidebarActivity::Done => Some(
                        div().size(px(6.0)).rounded_full().bg(theme.primary).into_any_element(),
                    ),
                    // 刚完成：先闪一个对勾做确认，`COMPLETION_FLASH` 之后沉降
                    // 成上面那个圆点（同一件事的两个阶段，不是两种语义）。
                    SidebarActivity::Completed => Some(
                        Icon::new(IconName::Check)
                            .xsmall()
                            .text_color(theme.success)
                            .into_any_element(),
                    ),
                    // 上一条命令非 0 退出：⚠ 三角就是给失败的，形状本身在所有
                    // 终端/编辑器里都这么读。pane 级故障用下面的 ✕，两者不同。
                    SidebarActivity::CommandFailed => Some(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(theme.danger)
                            .into_any_element(),
                    ),
                    // 停在 [y/n]/口令/Press ENTER 上：也要你动手，但这是常态，
                    // 用 Attention 的警示形状去喊会把真正要紧的那个喊废。
                    SidebarActivity::WaitingInput => Some(
                        Self::waiting_hand(symbol_family.clone(), label_px, theme.primary)
                            .into_any_element(),
                    ),
                    // 停在授权/提问上：同样是「有人在等你」，所以跟
                    // `WaitingInput` 用同一个手掌——形状表达语义。差别走颜色轴：
                    // 它会把整个回合卡住，值得 warning。⚠ 三角留给失败，拿它表
                    // 示「问你一句」会跟所有终端/编辑器的既有读法冲突。
                    SidebarActivity::Attention => Some(
                        Self::waiting_hand(symbol_family.clone(), label_px, theme.warning)
                            .into_any_element(),
                    ),
                    SidebarActivity::Failed => Some(
                        Icon::new(IconName::CircleX)
                            .xsmall()
                            .text_color(theme.danger)
                            .into_any_element(),
                    ),
                    SidebarActivity::Idle => shell_tag.map(|tag| {
                        Self::shell_status_label(
                            tag,
                            metadata_family.clone(),
                            label_px * SIDEBAR_TAG_SCALE,
                            status_color,
                        )
                        .into_any_element()
                    }),
                };
                // 三类行位移（旧壳 tab_drag_draw_y 的语义）：被拖行骑指针，
                // 源与落点之间的行向反方向让一个槽位，其余不动。存储顺序在
                // 拖拽期间不变，释放时一次性提交。
                let (dragged, _shift) = match drag {
                    Some((src, _, _)) if ix == src => (true, 0.0),
                    Some((src, tgt, _)) if src < tgt && ix > src && ix <= tgt => {
                        (false, -row_pitch)
                    },
                    Some((src, tgt, _)) if src > tgt && ix >= tgt && ix < src => {
                        (false, row_pitch)
                    },
                    _ => (false, 0.0),
                };
                let row = h_flex()
                .id(("sidebar-tab", ix))
                .debug_selector(move || format!("sidebar-tab-{ix}"))
                .when_some(tooltip, |row, text| row.tooltip(move |window, cx| {
                    tab_presentation::tooltip(text.clone(), window, cx)
                }))
                .group(hover_group.clone())
                .relative()
                // 旧壳 `layout.tabs[i]` 的命中矩形覆盖整条可见行。
                //
                // 这里必须是**显式像素宽**，不能用 `w_full`：行在
                // `tab_scroll::wrap_rows_scroll_list` 的 overflow_hidden 容器
                // 里，百分比宽度到不了这一层，会回落成 shrink-to-fit——表现
                // 就是行宽跟着文件名长短变，带图标的行还整体右移一个图标宽
                // （用户 08-19 报的侧栏 tab 宽度乱跳）。双击重命名"看起来正常"
                // 只是因为 Input 恰好把容器撑满，不是宽度真的对了。
                .w(px(row_w))
                // 内容再宽也不许把行撑开：截断后的标题若比测量值宽一两像素，
                // 撑开的行会重新引入上面那个症状。
                .overflow_hidden()
                .gap_2()
                // 左侧让位给目录树肘形线（图标列从
                // [`task_groups::tree_icon_left`] 起，SSH 主机下的条目同
                // 档不缩进），右侧仍是 8px 旧壳内边距。
                .pl(px(task_groups::tree_icon_left()))
                .pr(px(8.0))
                .h(px(entry_h))
                // 底部留出副行带：第一行内容在剩余高度里垂直居中，副行走
                // absolute 槽位（见行链上的 subline child）。
                .pb(px(subline_h))
                .items_center()
                // 旧壳 pill 圆角 = UI_CORNER_RADIUS_LOGICAL(8)，rounded_md(6)
                // 偏小一圈，选中水洗的轮廓形状会不一样。
                .rounded(px(crate::display::UI_CORNER_RADIUS_LOGICAL))
                // GPUI 默认文本样式可能把侧栏整行带到中等/粗体；旧壳
                // tab chrome 使用终端 Regular face，所有子文本从这里继承常规字重。
                .font_weight(FontWeight::NORMAL)
                .cursor_pointer()
                .when(active, |item| item.bg(active_bg).text_color(active_fg))
                .when(!active && !dragged, |item| {
                    item.text_color(muted).hover(|style| style.bg(hover_bg))
                })
                .when(!active && dragged, |item| item.text_color(muted).bg(hover_bg))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_tab(ix, window, cx);
                }))
                // 分组视图不做组内拖拽重排（组由 cwd 派生，跨组拖拽没有落点
                // 语义），但保留待命装填：gpui 的 click 合成链路依赖视图节点
                // 在 bubble 阶段的 mouse_down listener（div.rs pending_mouse_
                // down 的记录条件），去掉它条目 on_click 会整体失效——见
                // `sidebar_groups` 布局测试的点击断言。拖拽永不激活（移动阈值
                // 由 update_tab_drag 驱动，本视图不喂给它轴位移）。
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.tab_drag = Some(TabDrag {
                            source: ix,
                            cross_window: this.cross_window_drag_payload(ix, cx),
                            cross_window_target: None,
                            press_x: f32::from(event.position.x),
                            press_y: f32::from(event.position.y),
                            axis: TabDragAxis::Vertical,
                            pitch: row_pitch,
                            offset: 0.0,
                            active: false,
                            dock: None,
                        });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.request_close_tab(ix, window, cx);
                    }),
                )
                .on_double_click(cx.listener(move |this, _, window, cx| {
                    // 旧壳 `ChromeHit::Tab` + DoubleClick → BeginRename。
                    cx.stop_propagation();
                    this.begin_rename(ix, window, cx);
                }))
                .when_some(strip, |row, color| {
                    row.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(7.0))
                            .w(px(2.5))
                            .h(px(entry_h - 14.0))
                            .rounded_full()
                            .bg(color),
                    )
                })
                // 权限属于整个会话，先于程序身份呈现；**agent 会话期让位**
                // ——行首只留 agent 品牌图标（用户 10-10 裁定）。品牌存在 =
                // `logo_image` 已就位或仍在 `logo_pending` 装载中。
                .when(local_administrator && logo_image.is_none() && !logo_pending, |row| {
                    row.child(tab_presentation::administrator_badge(
                        format!("sidebar-admin-{ix}").into(),
                        cx,
                    ))
                })
                // 行首图标的优先级：**先身份、后形态**。AI 品牌图 / 程序字位
                // 表达「这个 tab 里在跑什么」，它必须跟随聚焦 pane；2×2
                // 分屏标记只在没有身份可显示时补位。
                // 反过来（分屏就一律画 grid）会让 claude 分屏之后标签上再也
                // 看不出跑着 claude——用户 08-23 报的「侧边 tab 不跟随激活
                // tab 变化」就是这个。数量由尾部胶囊表达，不必和图标抢槽位。
                .when(is_settings, |row| {
                    row.child(
                        div().w(px(TAB_LABEL_ICON_W)).flex_shrink_0().flex().justify_center().child(
                            Icon::new(IconName::Settings)
                                .small()
                                .text_color(if active { active_fg } else { muted }),
                        ),
                    )
                })
                .when_some(logo_image.clone(), |row, image| {
                    row.child(
                        div().w(px(TAB_LABEL_ICON_W)).flex_shrink_0().flex().items_center().justify_center().child(
                            img(image).size(px(TAB_LABEL_ICON_SIZE)).object_fit(ObjectFit::Contain),
                        ),
                    )
                })
                .when_some(program_glyph, |row, glyph| {
                    row.child(
                        div()
                            .w(px(if logo_pending { TAB_LABEL_ICON_SIZE } else { TAB_LABEL_ICON_W }))
                            .flex_shrink_0()
                            .font_family(symbol_family.clone())
                            .text_size(px(label_px))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(if active { active_fg } else { muted })
                            .child(glyph),
                    )
                })
                .when(
                    pane_count > 1 && !is_settings && logo_image.is_none() && !has_program_glyph,
                    |row| {
                        row.child(
                            div()
                                .w(px(TAB_LABEL_ICON_W))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(pane_header::split_glyph(
                                    label_px * 0.78,
                                    if active { active_fg } else { muted },
                                )),
                        )
                    },
                )
                // GPUI truncates the actual shaped title within its flex column.
                .child(match renaming {
                    Some(input) => div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.cancel_rename(window, cx);
                            }
                        }))
                        .child(
                            Input::new(&input)
                                .w_full()
                                .text_size(px(label_px))
                                .font_family(chrome_family.clone()),
                        )
                        .into_any_element(),
                    None => div()
                        .flex_1()
                        .min_w_0()
                        .font_family(chrome_family.clone())
                        .text_size(px(label_px))
                        .font_weight(FontWeight::NORMAL)
                        .truncate()
                        // Agent 会话期第一行显示品牌名（`headline`）；目录名
                        // 身份仍由 `title` 供 tooltip/顶栏/runtime API。
                        .child(headline)
                        .into_any_element(),
                })
                .when(pane_count > 1, |row| {
                    row.child(
                        div()
                            .id(("sidebar-pane-count", ix))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.cycle_pane_focus(ix, window, cx);
                            }))
                            .child(pane_header::split_badge(
                                pane_count,
                                label_px,
                                if active { active_fg } else { muted },
                                if active { active_bg } else { theme.muted },
                            )),
                    )
                })
                .child(
                    Self::tab_status_slot(status_width)
                        .when_some(resting_status, |slot, status| {
                            slot.child(
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .justify_end()
                                    .items_center()
                                    .gap_1()
                                    .group_hover(hover_group.clone(), |item| item.invisible())
                                    // 徽章左侧的同语义文字描述（Running/Ready…）。
                                    .when_some(status_label, |row, label| {
                                        row.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .font_family(chrome_family.clone())
                                                .text_size(px(label_px * SIDEBAR_TAG_SCALE))
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(activity_label_color(activity, theme))
                                                .child(label),
                                        )
                                    })
                                    .child(status),
                            )
                        })
                        .child(
                            // 关闭按钮跟状态徽章共用同一个居中槽位：位置由
                            // flex 给出，不再硬写 top 偏移。
                            h_flex()
                                .absolute()
                                .inset_0()
                                .justify_end()
                                .items_center()
                                .invisible()
                                .group_hover(hover_group, |slot| slot.visible())
                                .when(tab_close_visible, |slot| {
                                        slot.child(
                                            Button::new(("close-tab", ix))
                                                .icon(IconName::Close)
                                                .ghost()
                                                .xsmall()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.request_close_tab(ix, window, cx);
                                                    },
                                                )),
                                        )
                                    }),
                        ),
                )
                // 副行槽位：absolute 底带，不参与第一行 flex 排布（行相对
                // 定位，宽从图标槽右侧起，避开状态徽章与关闭按钮所在尾带）。
                // 自动换行、最多两行、末行省略——`line_clamp` + `text_ellipsis`
                // + Definite 宽走 gpui 的 truncate_wrapped_line；不能用
                // `.truncate()`（whitespace_nowrap 会关掉换行）。
                .child(
                    div()
                        .absolute()
                        // 与第一行任务名文字左缘对齐（见 tree_text_left）。
                        .left(px(task_groups::tree_text_left()))
                        .bottom(px(2.0))
                        .w(px((row_w - task_groups::tree_text_left() - 52.0).max(40.0)))
                        .h(px(2.0 * subline_line_h))
                        .line_height(px(subline_line_h))
                        .line_clamp(2)
                        .text_ellipsis()
                        .font_family(chrome_family.clone())
                        .text_size(px(subline_font_px))
                        .text_color(if active { active_fg } else { muted })
                        .child(subline),
                )
                // 右键只记锚点：菜单由 workspace 根上唯一一份宿主画。挂
                // `.context_menu()` 会让每个标签行都渲染同一个 PopupMenu，
                // 阴影按标签数叠厚（见 `tab_menu.rs` 模块头）。
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_tab_context_menu(ix, event.position, window, cx);
                    }),
                )
                .when_some(cross_window_drag, |row, payload| {
                    row.on_drag(payload, |payload, _, _, cx| {
                        NebulaWorkspace::cross_window_drag_preview(payload, cx)
                    })
                });
                row.into_any_element()
                }
            })
            .collect::<Vec<_>>();

        let sidebar = v_flex()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            // workspace 根保持透明，侧栏自己只铺一层壳色；否则终端卡会
            // 叠到根底色上，把 Acrylic 的目标透明度二次增浓。
            .bg(theme.background)
            // 上边距为零：侧栏 / 终端卡 / 右侧抽屉三列顶边一律贴 chrome 下沿
            // （用户 08-26 裁定「左侧 tab 抬到和文件树顶部一致」）。左右和底部
            // 仍是旧壳的 8px。
            .px_2()
            .pb_2()
            .gap(px(tab_scroll::tab_row_gap(self.density)))
            // 待命阶段（未过阈值）的指针跟踪；激活后由根部罩层独占接管。
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_tab_drag(event, window, cx);
            }))
            .child(self.wrap_rows_scroll_list(items, guides, bar, cx));
        self.spinner_visible.set(items_running.get());
        if items_running.get() {
            self.arm_activity_spinner_frame(window, cx);
        }
        sidebar
    }

    /// 「当前任务」分区头行：折叠箭头 + 标题 + 计数 + 悬停 +/⋯ 动作钮。
    /// 统一列表的首行；行宽显式像素（同条目行的教训，容器 overflow_hidden
    /// 下 w_full 会回落 shrink-to-fit）。
    fn render_tabs_section_header(
        &self,
        row_w: f32,
        count: usize,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hover_bg = theme.list_hover;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        let chrome_family = theme.mono_font_family.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let header_group: SharedString = "sidebar-tabs-header-hover".into();
        let count_text: SharedString = count.to_string().into();
        let row_height = tab_scroll::tab_row_height(self.density);
        h_flex()
            .id("sidebar-tabs-toggle")
            .debug_selector(|| "sidebar-tabs-toggle".to_owned())
            .group(header_group.clone())
            .w(px(row_w))
            .h(px(row_height))
            .pb_1()
            // 旧壳标题文字从 panel_x + 16px 起；箭头槽自带缩进，这里再补
            // 8px，箭头不会贴住左边缘。
            .pl_2()
            .items_center()
            .cursor_pointer()
            .flex_shrink_0()
            // 命中整条分区头：右侧空白区同样可以折叠，而不是只有箭头、
            // 标题和数量这一小段能点。右侧 +/⋯ 自己停止冒泡。
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_tree_tabs_section(cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, _| {}),
            )
            .child(
                // 箭头、标题和计数仍作为一个排版段。
                h_flex()
                    .h_full()
                    .items_center()
                    .font_family(chrome_family.clone())
                    .pr_1()
                    .child(
                        // 折叠三角用组件库线性 Chevron（lucide 细线），和右侧
                        // +/⋯ 的 SVG 同一套语言。
                        h_flex()
                            .w(px(TABS_DISCLOSURE_SLOT_W))
                            .h_full()
                            .flex_shrink_0()
                            .items_center()
                            .child(
                                Icon::new(if collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .xsmall()
                                .text_color(muted),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(muted)
                            .child(language.text(Message::SidebarCurrentTasks)),
                    )
                    .child(
                        // chip 尺寸贴旧壳公式：h = max(cell_h×0.82×1.18, 11)
                        // ≈ 18-19px（不是 22），1 位数宽 max(adv+8, h×1.25)。
                        // 数字 ink_faint。
                        h_flex()
                            .ml_2()
                            .h(px((label_px * 1.22 * SIDEBAR_TITLE_SCALE * 1.18).max(11.0)))
                            .min_w(px(label_px * 0.62 + 8.0))
                            .px_2()
                            .justify_center()
                            .items_center()
                            .rounded_full()
                            .bg(theme.muted)
                            .text_size(px(label_px * SIDEBAR_TITLE_SCALE))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(faint)
                            .child(count_text),
                    ),
            )
            // 数量徽标右侧的水平 hairline：填满标题行剩余宽度（悬停动作钮
            // 出现时线在钮前收尾）。
            .child(
                div()
                    .debug_selector(|| "sidebar-tabs-hairline".to_owned())
                    .flex_1()
                    .ml_2()
                    .h(px(1.0))
                    .bg(crate::gpui_shell::theme::card_divider_color(cx)),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(2.0))
                    .child(
                        // 直接开设置里的默认 shell，不经过选择器；三点才是
                        // NewTabMenu。
                        sidebar_new_tab_control(
                            header_group,
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.add_terminal(window, cx);
                            }),
                        )
                        .rounded_md()
                        .text_color(muted)
                        .hover(|button| button.bg(hover_bg).text_color(theme.foreground))
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new("新建终端 (Ctrl+Shift+T)")
                                .build(window, cx)
                        })
                        .child(Icon::new(IconName::Plus).with_size(px(SIDEBAR_HEADER_ICON))),
                    )
                    .child(
                        h_flex()
                            .id("sidebar-tabs-menu")
                            .w(px(SIDEBAR_MENU_W))
                            .h(px(SIDEBAR_PLUS_SIZE))
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(|button| button.bg(hover_bg).text_color(theme.foreground))
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new("新建终端 (Ctrl+K)")
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_shell_palette(window, cx);
                            }))
                            .child(
                                Icon::new(IconName::EllipsisVertical)
                                    .with_size(px(SIDEBAR_HEADER_ICON)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 与旧壳 `nebula_tabs_section_open` 同义：点标题整行折叠/展开。
    /// 折叠即时生效（与项目组折叠一致）——行数变化后就地钳制滚动窗口。
    fn toggle_tree_tabs_section(&mut self, cx: &mut Context<Self>) {
        self.tabs_section_collapsed = !self.tabs_section_collapsed;
        // 行移位，菜单锚定的那一行不在原处了。
        self.tab_menu = None;
        self.clamp_rows_after_group_toggle(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_states_stay_on_the_active_tab_as_ready() {
        // 状态标签体系下 Done/Completed 读作「回合结束、Ready 待命」——高亮
        // 行也照画，不得回落成 shell 短标（用户 10-10 报：Ready 被 pwsh 盖掉）。
        assert_eq!(
            tree_resting_activity(SidebarActivity::Done, true, false),
            SidebarActivity::Done
        );
        assert_eq!(
            tree_resting_activity(SidebarActivity::Done, false, false),
            SidebarActivity::Done
        );
    }

    #[test]
    fn ongoing_states_show_on_the_active_tab_too() {
        // 状态描述「此刻仍然如此」，看不看都成立——当前 tab 一样要画。
        for state in [
            SidebarActivity::Running,
            SidebarActivity::Paused,
            SidebarActivity::WaitingInput,
            SidebarActivity::Attention,
            SidebarActivity::CommandFailed,
            SidebarActivity::Failed,
        ] {
            assert_eq!(tree_resting_activity(state, true, false), state);
            assert_eq!(tree_resting_activity(state, false, false), state);
            // 响铃补位只从 Idle 起跳，不能盖掉任何进行中的状态。
            assert_eq!(tree_resting_activity(state, false, true), state);
        }
    }

    /// 对勾是「完成」的第一阶段，与圆点同语义；命令失败不是事件——看一眼
    /// 不等于处理完了，所以它留在当前 tab 上。
    #[test]
    fn completion_flash_follows_the_same_rule_as_the_dot() {
        assert_eq!(
            tree_resting_activity(SidebarActivity::Completed, true, false),
            SidebarActivity::Completed
        );
        assert_eq!(
            tree_resting_activity(SidebarActivity::Completed, false, false),
            SidebarActivity::Completed
        );
        assert_eq!(
            tree_resting_activity(SidebarActivity::CommandFailed, true, false),
            SidebarActivity::CommandFailed
        );
    }

    #[test]
    fn background_bell_stands_in_for_a_finish_mark_but_only_while_unattended() {
        assert_eq!(
            tree_resting_activity(SidebarActivity::Idle, false, true),
            SidebarActivity::Done
        );
        assert_eq!(tree_resting_activity(SidebarActivity::Idle, true, true), SidebarActivity::Idle);
        assert_eq!(
            tree_resting_activity(SidebarActivity::Idle, false, false),
            SidebarActivity::Idle
        );
    }

    #[test]
    fn agent_scoped_activity_hides_plain_terminal_states_but_keeps_failures() {
        // 普通终端的常规状态一律回落 Idle（无点无文字，空闲短标照旧）。
        for state in [
            SidebarActivity::Running,
            SidebarActivity::Paused,
            SidebarActivity::Done,
            SidebarActivity::Completed,
            SidebarActivity::CommandFailed,
            SidebarActivity::WaitingInput,
            SidebarActivity::Attention,
        ] {
            assert_eq!(agent_scoped_activity(state, false), SidebarActivity::Idle, "{state:?}");
        }
        // pane 级故障例外：挂掉的连接必须看得见。
        assert_eq!(agent_scoped_activity(SidebarActivity::Failed, false), SidebarActivity::Failed);
        // AI 会话状态原样。
        for state in [
            SidebarActivity::Running,
            SidebarActivity::Done,
            SidebarActivity::Failed,
            SidebarActivity::Idle,
        ] {
            assert_eq!(agent_scoped_activity(state, true), state, "{state:?}");
        }
    }
}
