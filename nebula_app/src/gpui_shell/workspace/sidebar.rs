use super::sidebar_groups::SidebarTabRow;
use super::*;
use crate::gpui_shell::widgets::toolbar_button;
use crate::i18n::Message;

/// 状态图形与 hover 关闭按钮共用的方形槽，中心不随图形宽度改变。
const TAB_STATUS_ICON_SIDE: f32 = 24.0;
const TAB_STATUS_SYMBOL_PX: f32 = 14.0;

/// 侧栏新建入口的生产命中区；鼠标测试直接复用，避免只验证一个近似按钮。
pub(super) fn sidebar_new_tab_control(
    header_group: SharedString,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id("sidebar-new-tab")
        .size(px(SIDEBAR_HEADER_ACTION))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .invisible()
        .group_hover(header_group, |button| button.visible())
        // Header controls own the entire gesture, including mouse-down.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
}

/// 事件 vs 状态：一个 tab 此刻该显示哪个静息徽章。
///
/// - `Done` 是**事件**——「回合完成了，你不在场」。你正看着这个 tab 时它没有
///   信息量了，还画一个点等于让用户去点掉自己刚亲眼看完的事，所以当前 tab
///   一律降回 `Idle`。
/// - 后台响铃补位到 `Done` 同理，只对非当前 tab 成立。`has_bell` 本身激活即清
///   （见 `TabMeta::has_bell`），这里再挡一次是因为清除发生在激活事件里，同一帧
///   内读到的可能还是旧值。
/// - 其余都是**状态**（转圈、等输入、等授权、失败），描述「此刻仍然如此」，你看
///   不看它都还成立，所以当前 tab 也照画。响铃补位的 `Idle` 前提保证了它不会
///   盖掉这些。
pub(super) fn resting_activity(
    activity: SidebarActivity,
    active: bool,
    has_bell: bool,
) -> SidebarActivity {
    match activity {
        SidebarActivity::Done | SidebarActivity::Completed if active => SidebarActivity::Idle,
        SidebarActivity::Idle if !active && has_bell => SidebarActivity::Done,
        other => other,
    }
}

impl NebulaWorkspace {
    pub(super) fn shell_status_label(
        tag: SharedString,
        family: SharedString,
        size_px: f32,
        color: gpui::Hsla,
    ) -> gpui::Div {
        // Short labels retain their width so the status lane can center them.
        // Long labels truncate inside the same lane.
        div()
            .max_w_full()
            .min_w_0()
            .truncate()
            .font_family(family)
            .text_size(px(size_px))
            .font_weight(FontWeight::NORMAL)
            .text_color(color)
            .child(tag)
    }

    pub(super) fn tab_status_slot(width: f32) -> gpui::Div {
        div().relative().w(px(width)).h_full().flex_shrink_0()
    }

    pub(super) fn tab_status_icon(icon: impl IntoElement) -> gpui::Div {
        h_flex()
            .size(px(TAB_STATUS_ICON_SIDE))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .child(icon)
    }

    pub(super) fn tab_status_lane() -> gpui::Div {
        h_flex().absolute().inset_0().justify_center().items_center()
    }

    pub(super) fn tab_activity_badge(
        &self,
        activity: SidebarActivity,
        active: bool,
        cx: &Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let theme = cx.theme();
        let symbol = |name, color| {
            Icon::new(name).size(px(TAB_STATUS_SYMBOL_PX)).text_color(color).into_any_element()
        };
        Some(match activity {
            SidebarActivity::Idle => return None,
            SidebarActivity::Running => {
                let (track, head) = crate::gpui_shell::theme::sidebar_spinner_colors(cx, active);
                Self::spinner(self.spinner_phase, track, head, cx.weak_entity()).into_any_element()
            },
            SidebarActivity::Paused => symbol(IconName::Pause, theme.warning),
            SidebarActivity::Done => {
                div().size(px(7.0)).rounded_full().bg(theme.primary).into_any_element()
            },
            SidebarActivity::Completed => symbol(IconName::Check, theme.success),
            SidebarActivity::CommandFailed => symbol(IconName::TriangleAlert, theme.danger),
            SidebarActivity::WaitingInput => {
                Self::waiting_hand(13.0, theme.primary).into_any_element()
            },
            SidebarActivity::Attention => {
                Self::waiting_hand(13.0, theme.warning).into_any_element()
            },
            SidebarActivity::Failed => symbol(IconName::CircleX, theme.danger),
        })
    }

    /// 静态细轨道加 30% 的旋转弧。SVG 的缓存身份只含路径和像素尺寸，
    /// 每帧复用 alpha mask，仅更新矩阵；不重新生成路径或逐点混色。
    pub(super) fn spinner(
        phase: f32,
        track: gpui::Rgba,
        head: gpui::Rgba,
        workspace: gpui::WeakEntity<Self>,
    ) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                // 两处 status 容器的 hover 都用 invisible()：GPUI 的 Hidden
                // 分支跳过子元素 paint，根本不会走到这里。折叠/滚动则由
                // 当前 content mask 裁定；不要改成仍会调用 paint 的 opacity(0)。
                if bounds.intersect(&window.content_mask().bounds).is_empty() {
                    return;
                }
                let _ = workspace.update(cx, |workspace, cx| {
                    workspace.spinner_visible.set(true);
                    workspace.arm_activity_spinner_frame(window, cx);
                });
                // SVG mask 按宽度等比栅格化；吸附后仍须保持物理像素正方形，
                // 否则 16×17 的边界会让实际 sprite 中心与旋转中心错开。
                let side = window.pixel_snap(bounds.size.width.min(bounds.size.height));
                let origin =
                    window.pixel_snap_point(bounds.center() - gpui::point(side / 2.0, side / 2.0));
                let bounds = Bounds::new(origin, size(side, side));
                let center = bounds.center();
                let scale = window.scale_factor();
                let transform = gpui::TransformationMatrix::unit()
                    .translate(center.scale(scale))
                    .rotate(gpui::radians(phase * std::f32::consts::TAU))
                    .translate(center.scale(-scale));
                for (path, matrix, color) in [
                    ("icons/nebula-spinner-track.svg", gpui::TransformationMatrix::unit(), track),
                    ("icons/nebula-spinner-arc.svg", transform, head),
                ] {
                    if let Err(error) =
                        window.paint_svg(bounds, path.into(), None, matrix, color.into(), cx)
                    {
                        log::warn!("cannot draw activity spinner: {error}");
                    }
                }
            },
        )
        .size(px(12.0))
    }

    /// 保留打包字体 U+F256 的开掌轮廓，用其墨迹边界居中。
    /// 原字形宽 866 units、advance 600；按文字排版会向右溢出且受基线影响。
    /// 等输入与授权/提问共用手掌，颜色继续表达严重度。
    pub(super) fn waiting_hand(size_px: f32, color: gpui::Hsla) -> impl IntoElement {
        gpui::svg()
            .path("icons/nebula-waiting-hand.svg")
            .size(px(size_px))
            .flex_shrink_0()
            .text_color(color)
    }

    fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header_group: SharedString = "sidebar-tabs-header-hover".into();
        let grouping_control = self.sidebar_group_control(header_group.clone(), window, cx);
        let row_height = tab_scroll::tab_row_height(self.density);
        let row_pitch = self.tab_row_pitch();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        // 选中 tab 与内容板同材质地浮起（浮起底 + 极淡环线与投影），读作
        // “它就是右边那块”；不再用强调色水洗。
        let active_bg = crate::gpui_shell::theme::raised_fill(cx);
        let active_shadow = crate::gpui_shell::theme::raised_shadow(cx);
        let (active_fg, idle_title, shell_ink) = crate::gpui_shell::theme::tab_text_colors(cx);
        let hover_bg = theme.list_hover;
        let dark = theme.is_dark();
        // 标题走 UI sans，路径/状态短标才用 Normal 等宽；有图标时才占图标槽。
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let tab_close_visible = settings.map(|settings| settings.tab_close_visible).unwrap_or(true);
        let tab_reveal = settings
            .map(|settings| settings.tab_reveal)
            .unwrap_or(nebula_settings::TabRevealName::Slide);
        let chrome_family = theme.font_family.clone();
        let metadata_family = theme.mono_font_family.clone();
        let symbol_family: SharedString = crate::font_install::SYMBOL_FONT_FAMILY.into();
        // Preserve the prototype's type hierarchy when the UI font size changes.
        let ui_scale = f32::from(theme.font_size) / 14.0;
        let label_px = 13.0 * ui_scale;
        let tag_px = 11.0 * ui_scale;

        let rows = self.sidebar_tab_rows(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        // 受约束拖拽的渲染参数：激活后被拖行骑指针位移，落点槽位由位移换算。
        let drag = self
            .tab_drag
            .as_ref()
            .filter(|d| d.active && d.axis == TabDragAxis::Vertical)
            .and_then(|d| {
                let target = Self::sidebar_drag_slot(d, &rows);
                Some((rows.index_of(d.source)?, rows.index_of(target)?, d.offset))
            });

        let (tabs_scroll, tabs_show) = self.tabs_visible_window(rows.len());
        // 行的确定宽度：侧栏宽 − 侧栏 p_2 两边 − 列表右侧滚动条留白。
        // 与下面 `label_avail` 同一份减法口径，两者不能各算一套。
        // 再左右各让 1px：列表容器 overflow_hidden，选中行的浮起环线贴边会被裁掉。
        let row_w = (self.sidebar_width - 16.0 - tab_scroll::TAB_SCROLL_GUTTER - 2.0).max(1.0);
        let items = (tabs_scroll..tabs_scroll + tabs_show)
            .filter_map(|slot| rows.row(slot).map(|item| (slot, item)))
            .map(|(slot, item)| {
                let ix = match item {
                    SidebarTabRow::Tab(ix) => ix,
                    SidebarTabRow::Group(group) => {
                        let line = || div().flex_1().min_w(px(12.0)).h(px(1.0)).bg(theme.border);
                        let mut heading = h_flex()
                            .id(("sidebar-tab-group", slot))
                            .w(px(row_w))
                            .h(px(row_height))
                            .px_2()
                            .gap_2()
                            .items_center()
                            .child(line())
                            .child(div().max_w(px(row_w * 0.62)).truncate()
                                .font_family(chrome_family.clone())
                                .text_size(px(10.5 * ui_scale)).font_weight(FontWeight::SEMIBOLD)
                                .text_color(muted).child(group.label(language)))
                            .child(line());
                        if let Some(path) = group.directory_path() {
                            heading = heading.tooltip(move |window, cx| {
                                Tooltip::new(path.clone()).build(window, cx)
                            });
                        }
                        return heading.into_any_element();
                    },
                };
                let active = ix == self.active;
                let TabPresentation {
                    title,
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
                } = self.tab_presentation(ix, cx, dark);
                let hover_group: SharedString = format!("sidebar-tab-hover-{ix}").into();
                let has_program_glyph = program_glyph.is_some();
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
                let resting_status = self.tab_activity_badge(activity, active, cx).or_else(|| {
                    shell_tag.map(|tag| {
                        Self::shell_status_label(tag, metadata_family.clone(), tag_px, shell_ink)
                            .into_any_element()
                    })
                });
                // 三类行位移（旧壳 tab_drag_draw_y 的语义）：被拖行骑指针，
                // 源与落点之间的行向反方向让一个槽位，其余不动。存储顺序在
                // 拖拽期间不变，释放时一次性提交。
                let (dragged, shift) = match drag {
                    Some((src, _, _)) if slot == src => (true, 0.0),
                    Some((src, tgt, _)) if src < tgt && slot > src && slot <= tgt => {
                        (false, -row_pitch)
                    },
                    Some((src, tgt, _)) if src > tgt && slot >= tgt && slot < src => {
                        (false, row_pitch)
                    },
                    _ => (false, 0.0),
                };
                let row = h_flex()
                .id(("sidebar-tab", ix))
                .debug_selector(move || format!("sidebar-tab-{ix}"))
                .when_some(tooltip.filter(|_| self.tab_menu.is_none()), |row, text| row.tooltip(move |window, cx| {
                    tab_presentation::tooltip(text.clone(), window, cx)
                }))
                .group(hover_group.clone())
                .relative()
                // 旧壳 `layout.tabs[i]` 的命中矩形覆盖整条可见行。
                //
                // 这里必须是**显式像素宽**，不能用 `w_full`：行在
                // `tab_scroll::wrap_tabs_scroll_list` 的 overflow_hidden 容器
                // 里，百分比宽度到不了这一层，会回落成 shrink-to-fit——表现
                // 就是行宽跟着文件名长短变，带图标的行还整体右移一个图标宽
                // （用户 08-19 报的侧栏 tab 宽度乱跳）。双击重命名"看起来正常"
                // 只是因为 Input 恰好把容器撑满，不是宽度真的对了。
                .w(px(row_w))
                .ml(px(1.0))
                // 内容再宽也不许把行撑开：截断后的标题若比测量值宽一两像素，
                // 撑开的行会重新引入上面那个症状。
                .overflow_hidden()
                .gap_2()
                .px_2()
                .h(px(row_height))
                .items_center()
                // 旧壳 pill 圆角 = UI_CORNER_RADIUS_LOGICAL(8)，rounded_md(6)
                // 偏小一圈，选中水洗的轮廓形状会不一样。
                .rounded(px(crate::display::UI_CORNER_RADIUS_LOGICAL))
                // GPUI 默认文本样式可能把侧栏整行带到中等/粗体；旧壳
                // tab chrome 使用终端 Regular face，所有子文本从这里继承常规字重。
                .font_weight(FontWeight::NORMAL)
                .cursor_pointer()
                .when(active, |item| {
                    item.bg(active_bg).shadow(active_shadow.clone()).text_color(active_fg)
                })
                .when(!active && !dragged, |item| {
                    item.text_color(idle_title).hover(|style| style.bg(hover_bg))
                })
                .when(!active && dragged, |item| item.text_color(idle_title).bg(hover_bg))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_tab(ix, window, cx);
                }))
                // 受约束拖拽：按下待命（源下标 + 按点 Y），移动阈值与让位
                // 由 update_tab_drag 驱动；激活后的指针独占见 render 根部
                // 的透明罩层。
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
                            force_tear_out: false,
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
                            .h(px(row_height - 14.0))
                            .rounded_full()
                            .bg(color),
                    )
                })
                // 权限属于整个会话，先于程序身份呈现，且不被 AI 图标替换。
                .when(local_administrator, |row| {
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
                            // 共用原图和 GPU 灰度处理；hover 只切绘制层，不重解码或改变占位。
                            div()
                                .relative()
                                .size(px(TAB_LABEL_ICON_SIZE))
                                .child(
                                    img(image.clone())
                                        .id(("sidebar-logo-idle", ix))
                                        .size_full()
                                        .object_fit(ObjectFit::Contain)
                                        .when(!active, |icon| {
                                            icon.grayscale(true).opacity(0.85)
                                                .group_hover(hover_group.clone(), |style| style.invisible())
                                        }),
                                )
                                .when(!active, |slot| {
                                    slot.child(
                                        img(image)
                                            .id(("sidebar-logo-hover", ix))
                                            .absolute()
                                            .inset_0()
                                            .size_full()
                                            .object_fit(ObjectFit::Contain)
                                            .invisible()
                                            .group_hover(hover_group.clone(), |style| style.visible()),
                                    )
                                }),
                        ),
                    )
                })
                .when_some(program_glyph, |row, glyph| {
                    row.child(
                        div()
                            .id(("sidebar-program-icon", ix))
                            .w(px(if logo_pending { TAB_LABEL_ICON_SIZE } else { TAB_LABEL_ICON_W }))
                            .flex_shrink_0()
                            .font_family(symbol_family.clone())
                            .text_size(px(label_px))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(if active { active_fg } else { muted })
                            .group_hover(hover_group.clone(), |style| style.text_color(active_fg))
                            .child(crate::gpui_shell::widgets::terminal_glyph(glyph, TAB_LABEL_ICON_SIZE)),
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
                        .child(title)
                        .into_any_element(),
                })
                .when(pane_count > 1, |row| {
                    row.child(
                        div()
                            .id(("sidebar-pane-count", ix))
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .hover(|style| style.bg(theme.muted.opacity(0.5)))
                            .active(|style| style.bg(theme.muted))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.cycle_pane_focus(ix, window, cx);
                            }))
                            .child(pane_header::split_layout_icon(
                                &self.tabs[ix],
                                label_px,
                                if active { active_fg } else { muted },
                            )),
                    )
                })
                .child(
                    Self::tab_status_slot(TAB_STATUS_SLOT_W)
                        .when_some(resting_status, |slot, status| {
                            slot.child(
                                Self::tab_status_lane()
                                    .group_hover(hover_group.clone(), |item| item.invisible())
                                    .child(if matches!(activity, SidebarActivity::Idle) {
                                        status
                                    } else {
                                        Self::tab_status_icon(status).into_any_element()
                                    }),
                            )
                        })
                        .child(
                            // 关闭按钮跟状态徽章共用同一个居中槽位：位置由
                            // flex 给出，不再硬写 top 偏移。
                            Self::tab_status_lane()
                                .invisible()
                                .group_hover(hover_group, |slot| slot.visible())
                                .when(tab_close_visible, |slot| {
                                        slot.child(Self::tab_status_icon(
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
                                        ))
                                    }),
                        ),
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
                if dragged {
                    // 骑指针 + 提到最上层画（deferred 只延后绘制、不动布局），
                    // 阴影给"拿起来"的抬升感。
                    gpui::deferred(
                        row.top(px(drag.map(|(_, _, off)| off).unwrap_or(0.0))).shadow_md(),
                    )
                    .into_any_element()
                } else if shift != 0.0 {
                    // 让位滑动：进位方向 ease-out 滑入（旧壳是双向弹簧；回位
                    // 这里先直落，违和再补逐帧插值）。设置「标签动画=立即」时
                    // 直接落位（旧壳 TabRevealMotion::Instant 的 Snap 语义）。
                    if tab_reveal == nebula_settings::TabRevealName::Instant {
                        row.top(px(shift)).into_any_element()
                    } else {
                        row.with_animation(
                            ("tab-make-way", ix),
                            Animation::new(Duration::from_millis(120))
                                .with_easing(ease_out_quint()),
                            move |row, t| row.top(px(shift * t)),
                        )
                        .into_any_element()
                    }
                } else {
                    row.into_any_element()
                }
            })
            .collect::<Vec<_>>();

        let count: SharedString = self.tabs.len().to_string().into();

        let sidebar = v_flex()
            .debug_selector(|| "workspace-sidebar".into())
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            // workspace 根保持透明，侧栏自己只铺一层壳色；否则终端卡会
            // 叠到根底色上，把 Acrylic 的目标透明度二次增浓。
            .bg(crate::gpui_shell::theme::tab_bar_background(cx))
            // The sidebar surface meets the title bar; its content starts below it.
            .px_2()
            .pt(px(8.0))
            .pb_2()
            .gap(px(tab_scroll::tab_row_gap(self.density)))
            // 待命阶段（未过阈值）的指针跟踪；激活后由根部罩层独占接管。
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_tab_drag(event, window, cx);
            }))
            .child(
                h_flex()
                    .id("sidebar-tabs-header")
                    .debug_selector(|| "sidebar-tabs-header".into())
                    .group(header_group.clone())
                    .w_full()
                    .h(px(row_height))
                    .pb_1()
                    .pl_2()
                    .items_center()
                    .child(
                        h_flex()
                            .h_full()
                            .items_center()
                            .gap(px(6.0))
                            .font_family(chrome_family.clone())
                            .pr_1()
                            .child(
                                h_flex()
                                    .id("sidebar-tabs-caption")
                                    .role(gpui::Role::Label)
                                    .aria_label("TABS")
                                    .gap(px(0.9 * ui_scale))
                                    .text_size(px(11.0 * ui_scale))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(muted)
                                    .children(["T", "A", "B", "S"]),
                            )
                            .child(
                                div()
                                    .font_family(metadata_family.clone())
                                    .text_size(px(10.0 * ui_scale))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(faint.opacity(0.72))
                                    .child(count),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(2.0))
                            .child(
                                // 旧壳 `ChromeHit::NewTab`：直接开设置里的默认
                                // shell，不经过选择器。三点才是 NewTabMenu。
                                sidebar_new_tab_control(
                                    header_group,
                                    cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.add_terminal(window, cx);
                                    }),
                                )
                                    .rounded(px(8.0))
                                    .text_color(muted)
                                    .hover(|button| button.bg(hover_bg).text_color(theme.foreground))
                                    .tooltip(|window, cx| {
                                        gpui_component::tooltip::Tooltip::new(
                                            crate::gpui_shell::config::ui_language(cx)
                                                .text(crate::i18n::Message::ChromeNewTerminalCtrlShiftT),
                                        )
                                        .build(window, cx)
                                    })
                                    .child(
                                        div()
                                            .debug_selector(|| "sidebar-new-tab-icon".into())
                                            .size(px(SIDEBAR_HEADER_ICON))
                                            .child(Icon::new(IconName::Plus).with_size(px(SIDEBAR_HEADER_ICON))),
                                    ),
                            )
                            .child(grouping_control)
                            .child(
                                h_flex()
                                    .id("sidebar-tabs-menu")
                                    .size(px(SIDEBAR_HEADER_ACTION))
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.0))
                                    .cursor_pointer()
                                    .text_color(muted)
                                    .hover(|button| button.bg(hover_bg).text_color(theme.foreground))
                                    .tooltip(|window, cx| {
                                        gpui_component::tooltip::Tooltip::new(
                                            crate::gpui_shell::config::ui_language(cx)
                                                .text(crate::i18n::Message::ChromeNewTerminalCtrlK),
                                        )
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
                    ),
            )
            .child(self.wrap_tabs_scroll_list(items, rows.len(), cx));
        sidebar
    }

    fn sidebar_fold_widths(&self, cx: &App) -> Option<(f32, f32)> {
        if cx.reduce_motion() || tab_reveal_instant(cx) || !self.sidebar_fold_armed {
            return None;
        }
        let width = self.sidebar_closing_width.unwrap_or(self.sidebar_width);
        Some(if self.sidebar_collapsed { (width, 0.0) } else { (0.0, width) })
    }

    fn animate_sidebar_width(
        &self,
        element: gpui::Div,
        id: &'static str,
        (from, to): (f32, f32),
        apply_width: impl Fn(gpui::Div, f32) -> gpui::Div + 'static,
    ) -> gpui::AnyElement {
        element
            .with_animation(
                (id, self.sidebar_collapsed as usize),
                Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()),
                move |element, t| apply_width(element, from + (to - from) * t),
            )
            .into_any_element()
    }

    /// 侧栏槽位：宽度在 0..持久化宽度间以 ease-out 滑动，近似旧壳
    /// response=0.14 的 swift-out 弹簧；内容保持固定宽、由槽位裁剪，
    /// 终端卡随 flex 布局自然滑移收编空间（对齐旧壳"卡骑在折叠动画上"
    /// 的观感）。动画按方向换 key 重启，端点随运行时设置变化。
    pub(super) fn render_sidebar_slot(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(widths) = self.sidebar_fold_widths(cx) else {
            return if self.sidebar_collapsed {
                div().into_any_element()
            } else {
                self.render_sidebar(window, cx).into_any_element()
            };
        };
        self.animate_sidebar_width(
            div().h_full().flex_shrink_0().overflow_hidden().child(self.render_sidebar(window, cx)),
            "sidebar-fold",
            widths,
            |slot, width| slot.w(px(width)),
        )
    }

    /// 侧栏模式的标题在终端区上方，位于侧栏右缘和右侧工具之间。
    pub(super) fn render_sidebar_title_bar(
        &self,
        settings_active: bool,
        leading_inset: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // 颜色先取出来：`cx.theme()` 不可变借着 cx，后面每个 `cx.listener`
        // 都要可变借，混在一个表达式里借用检查过不去。
        let settings_active_bg = cx.theme().sidebar_accent;
        let settings_active_fg = cx.theme().sidebar_accent_foreground;
        let reader_focus = self.reader_focus_active(cx);
        let sidebar_visible = !self.sidebar_collapsed && !reader_focus;
        let sidebar_icon = if sidebar_visible {
            Icon::new(Icon::empty()).path(crate::gpui_shell::assets::nav::SIDEBAR_EXPANDED)
        } else {
            Icon::new(IconName::PanelLeft)
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let port_forward_button = self.render_port_forward_button(cx);
        let broadcast_button = self.render_toolbar_broadcast(cx);
        let left_tools = h_flex()
            .debug_selector(|| "titlebar-left-tools".into())
            .gap(px(8.0))
            .flex_shrink_0()
            .occlude()
            .child(
                toolbar_button("toggle-sidebar", sidebar_icon, settings_active)
                    .aria_label(language.text(Message::ChromeToggleSidebar))
                    .tooltip(language.text(Message::ChromeToggleSidebar))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.reader_focus_active(cx) {
                            this.clear_reader_focus(cx);
                        } else {
                            this.sidebar_collapsed = !this.sidebar_collapsed;
                        }
                        this.sidebar_fold_armed = !tab_reveal_instant(cx);
                        cx.notify();
                    })),
            )
            .child(
                toolbar_button("open-settings", IconName::Settings, false)
                    .selected(settings_active)
                    .when(settings_active, |button| {
                        button.bg(settings_active_bg).text_color(settings_active_fg)
                    })
                    .tooltip(language.text(Message::ChromeSettingsShortcut))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(window, cx);
                    })),
            );
        let left = h_flex().child(left_tools);
        // Two 32px buttons and their 8px gap still fit when the sidebar is hidden.
        // A narrow window may shrink the reserved width before crowding the right tools.
        let fit_sidebar = move |left: gpui::Div, width: f32| {
            left.w(px((width - leading_inset).max(72.0))).min_w(px(72.0))
        };
        let left = if settings_active || reader_focus {
            left.into_any_element()
        } else if let Some(widths) = self.sidebar_fold_widths(cx) {
            self.animate_sidebar_width(left, "sidebar-title-fold", widths, fit_sidebar)
        } else if sidebar_visible {
            fit_sidebar(left, self.sidebar_width).into_any_element()
        } else {
            left.into_any_element()
        };
        h_flex()
            .size_full()
            .child(left)
            .child(self.render_active_pane_title(cx))
            .child(
                window_titlebar::title_bar_right_controls()
                    .when_some(broadcast_button, |controls, button| controls.child(button))
                    .child(self.render_terminal_find_button(settings_active, cx))
                    .child(
                        toolbar_button(
                            "toggle-command-manager",
                            Icon::new(Icon::empty())
                                .path(crate::gpui_shell::assets::nav::COMMAND_MANAGER),
                            false,
                        )
                        .debug_selector(|| "command-manager-toggle".into())
                        .selected(self.command_manager_open)
                        .tooltip(language.text(Message::ChromeCommandList))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_command_manager(window, cx);
                        })),
                    )
                    .when_some(port_forward_button, |controls, button| controls.child(button))
                    .child(self.render_right_sidebar_button(settings_active, cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    mod layout {
        use super::*;
        use std::cell::RefCell;
        use std::rc::Rc;

        struct StatusProbe {
            label: SharedString,
            font_size: f32,
            mask: Rc<RefCell<Option<Bounds<Pixels>>>>,
        }

        impl Render for StatusProbe {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                let mask = self.mask.clone();
                h_flex()
                    .id("status-probe-row")
                    .debug_selector(|| "status-probe-row".to_owned())
                    .w(px(200.0))
                    .h(px(tab_scroll::tab_row_height(nebula_settings::DensityName::Standard)))
                    .px_2()
                    .overflow_hidden()
                    .child(div().flex_1())
                    .child(
                        NebulaWorkspace::tab_status_slot(TAB_STATUS_SLOT_W)
                            .id("status-probe-slot")
                            .debug_selector(|| "status-probe-slot".to_owned())
                            .child(
                                NebulaWorkspace::tab_status_lane().child(
                                    NebulaWorkspace::shell_status_label(
                                        self.label.clone(),
                                        ".SystemUIFont".into(),
                                        self.font_size,
                                        gpui::black(),
                                    )
                                    .id("status-probe-label")
                                    .debug_selector(|| "status-probe-label".to_owned()),
                                ),
                            )
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |_, _, window, _| {
                                        *mask.borrow_mut() = Some(window.content_mask().bounds);
                                    },
                                )
                                .absolute()
                                .size_full(),
                            ),
                    )
            }
        }

        #[gpui::test]
        fn shell_tags_share_the_status_center_and_keep_row_padding(cx: &mut gpui::TestAppContext) {
            cx.update(gpui_component::init);
            for font_size in [12.0, 15.0, 20.0] {
                for label in ["sh", "debian", "a-very-long-shell-distribution-name"] {
                    let mask = Rc::new(RefCell::new(None));
                    let recorded = mask.clone();
                    let (_, visual) = cx.add_window_view(|_, _| StatusProbe {
                        label: label.into(),
                        font_size,
                        mask,
                    });
                    let row = visual.debug_bounds("status-probe-row").unwrap();
                    let slot = visual.debug_bounds("status-probe-slot").unwrap();
                    let text = visual.debug_bounds("status-probe-label").unwrap();
                    assert!(text.size.width <= slot.size.width);
                    let offset = text.center() - slot.center();
                    assert!(
                        f32::from(offset.x).abs() <= 1.0 && f32::from(offset.y).abs() <= 1.0,
                        "shell and badge share the slot center: {offset:?}",
                    );
                    if label == "sh" {
                        assert!(text.size.width < slot.size.width, "short labels stay intrinsic");
                    }
                    let clip = recorded.borrow().expect("status paint mask");
                    assert!(clip.right() > slot.right(), "glyph overhang must retain row padding");
                    assert!(clip.right() <= row.right(), "the outer row still clips overflow");
                }
            }
        }
    }

    #[test]
    fn finished_dot_is_an_event_so_the_tab_you_are_watching_never_shows_it() {
        // 你正盯着 agent 跑完：那个点在你眼前亮起没有任何信息量。
        assert_eq!(resting_activity(SidebarActivity::Done, true, false), SidebarActivity::Idle);
        // 后台跑完才留痕。
        assert_eq!(resting_activity(SidebarActivity::Done, false, false), SidebarActivity::Done);
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
            assert_eq!(resting_activity(state, true, false), state);
            assert_eq!(resting_activity(state, false, false), state);
            // 响铃补位只从 Idle 起跳，不能盖掉任何进行中的状态。
            assert_eq!(resting_activity(state, false, true), state);
        }
    }

    /// 对勾是「完成」的第一阶段，跟圆点同属事件：当前 tab 一律不画。命令失败
    /// 不是事件——看一眼不等于处理完了，所以它留在当前 tab 上。
    #[test]
    fn completion_flash_follows_the_same_unattended_rule_as_the_dot() {
        assert_eq!(
            resting_activity(SidebarActivity::Completed, true, false),
            SidebarActivity::Idle
        );
        assert_eq!(
            resting_activity(SidebarActivity::Completed, false, false),
            SidebarActivity::Completed
        );
        assert_eq!(
            resting_activity(SidebarActivity::CommandFailed, true, false),
            SidebarActivity::CommandFailed
        );
    }

    #[test]
    fn background_bell_stands_in_for_a_finish_mark_but_only_while_unattended() {
        assert_eq!(resting_activity(SidebarActivity::Idle, false, true), SidebarActivity::Done);
        assert_eq!(resting_activity(SidebarActivity::Idle, true, true), SidebarActivity::Idle);
        assert_eq!(resting_activity(SidebarActivity::Idle, false, false), SidebarActivity::Idle);
    }
}
