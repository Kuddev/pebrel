//! 侧栏任务列表的整行滚动（变节距行模型版）。
//!
//! 行高不再均匀（组头 26px、条目两行 42–46px），窗口化、reveal 与 thumb
//! 偏移都基于 [`super::task_groups`] 的前缀和；滚轮仍按整行步进，thumb 仅
//! 在列表悬停或拖拽时绘制。
//!
//! 「滚到 active」走 pending 标志：事件路径（新建/激活/恢复 tab）只置
//! [`NebulaWorkspace::tabs_scroll_pending_reveal`]，实际窗口在 render 侧由
//! 行模型换算——那里才有完整分组信息；用户滚轮或拖 thumb 会消费并清除
//! pending，之后 render 只做越界钳制，不写回滚动状态。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Bounds, Context, InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, ScrollWheelEvent, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, px,
};

use crate::gpui_shell::prelude::*;
use crate::scrollbar::{self, OverlayScrollbar};

use nebula_settings::DensityName;

use super::NebulaWorkspace;
use super::task_groups::{self, SidebarRow};

pub(super) fn tab_row_height(density: DensityName) -> f32 {
    match density {
        DensityName::Standard => 34.0,
        DensityName::Compact => crate::display::ui::tokens::control::MIN_HIT_TARGET,
    }
}

pub(super) fn tab_row_gap(density: DensityName) -> f32 {
    use crate::display::ui::tokens::space;
    match density {
        DensityName::Standard => space::XS,
        DensityName::Compact => space::XXS,
    }
}

/// 旧壳 `tab_right_pad`：给覆盖式滚动条留的恒定沟槽，出现与否都不改行宽。
pub(super) const TAB_SCROLL_GUTTER: f32 = 9.0;

// ---- 经典侧栏（`ai_agent_sidebar` 关）的均匀行高窗口数学：main 原样 ----
// 分组路径的等价物在 [`super::task_groups`]（变节距前缀和版）。

/// n 行占的高度：间距只在行之间，末行后面不留。
pub(super) fn rows_h(n: usize, pitch: f32, gap: f32) -> f32 {
    if n == 0 { 0.0 } else { n as f32 * pitch - gap }
}

/// 视口里能放下几行。尚未量到高度时先展示全部，避免首帧空列表。
///
/// 与旧壳 `chrome_tab_layout` 同一条规则：不够一行就是 0，**绝不**退成
/// 一行。折叠动画若把裁剪高度写回来，强制 1 行会把整列锁成一行滚动区。
pub(super) fn visible_count(want: usize, avail_h: f32, pitch: f32, gap: f32) -> usize {
    if want == 0 {
        return 0;
    }
    if avail_h <= 1.0 {
        return want;
    }
    let mut show = want;
    while show > 0 && rows_h(show, pitch, gap) > avail_h + 0.5 {
        show -= 1;
    }
    show
}

pub(super) fn max_scroll(want: usize, show: usize) -> usize {
    want.saturating_sub(show)
}

pub(super) fn clamp_scroll(scroll: usize, max: usize) -> usize {
    scroll.min(max)
}

pub(super) fn index_visible(ix: usize, scroll: usize, show: usize) -> bool {
    show > 0 && ix >= scroll && ix < scroll + show
}

/// 激活项滚进窗口：在窗口上方则对齐窗口顶，在下方则对齐窗口底。
pub(super) fn reveal_index(ix: usize, scroll: usize, show: usize) -> usize {
    if show == 0 {
        return 0;
    }
    if ix < scroll {
        ix
    } else if ix >= scroll + show {
        ix + 1 - show
    } else {
        scroll
    }
}

/// 经典侧栏的均匀行高窗口快照（main 版）。
struct TabsWindow {
    scroll: usize,
    show: usize,
    max: usize,
    want: usize,
}

/// 旧壳 `sidebar_wheel`：一次手势一行，符号与 winit 的 `-signum` 一致。
pub(super) fn wheel_rows(delta_y: f32) -> i32 {
    if delta_y > 0.0 {
        -1
    } else if delta_y < 0.0 {
        1
    } else {
        0
    }
}

pub(super) fn apply_wheel(scroll: usize, max: usize, rows: i32) -> usize {
    (scroll as i32 + rows).clamp(0, max as i32) as usize
}

fn point_in_rect((rx, ry, rw, rh): (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rx && y >= ry && y < ry + rh && x < rx + rw
}

fn tabs_scroll_thumb(bar: &OverlayScrollbar, color: gpui::Hsla) -> gpui::Div {
    // 滑块只负责绘制，命中由独立轨道处理；遮挡父列表会形成 hover 显隐反馈循环。
    div()
        .debug_selector(|| "sidebar-tabs-scroll-thumb".to_owned())
        .absolute()
        .left(px(bar.thumb.0))
        .top(px(bar.thumb.1))
        .w(px(bar.thumb.2))
        .h(px(bar.thumb.3))
        .rounded_full()
        .bg(color)
}

/// render 侧实际生效的行窗口（已按 pending reveal 或钳制修正）。
pub(super) struct RowsWindow {
    pub(super) scroll: usize,
    pub(super) show: usize,
    pub(super) max: usize,
    pub(super) rows: usize,
}

impl NebulaWorkspace {
    pub(super) fn tab_row_pitch(&self) -> f32 {
        tab_row_height(self.density) + tab_row_gap(self.density)
    }

    /// 经典侧栏的均匀行高窗口（main 版）：折叠只裁剪槽位，可用高度仍按
    /// `tabs_viewport_h` 算；`scroll` 是虚拟钳制值，不写回。
    fn tabs_window(&self) -> TabsWindow {
        let want = self.tabs.len();
        let show = visible_count(
            want,
            self.tabs_viewport_h,
            self.tab_row_pitch(),
            tab_row_gap(self.density),
        );
        let max = max_scroll(want, show);
        let scroll = clamp_scroll(self.tabs_scroll, max);
        TabsWindow { scroll, show, max, want }
    }

    pub(super) fn tabs_visible_window(&self) -> (usize, usize) {
        let window = self.tabs_window();
        (window.scroll, window.show)
    }

    /// 行几何：高度表 + 顶偏移表（含行间 gap）。render 与事件路径共用；
    /// 行高随密度与界面字号派生（见 [`task_groups::RowMetrics`]）。
    pub(super) fn rows_geometry(
        &self,
        rows: &[SidebarRow],
        cx: &gpui::App,
    ) -> (Vec<f32>, Vec<f32>) {
        let gap = tab_row_gap(self.density);
        let metrics = self.row_metrics(cx);
        let heights: Vec<f32> =
            rows.iter().map(|row| task_groups::row_height(row, &metrics)).collect();
        let offsets = task_groups::row_top_offsets(&heights, gap);
        (heights, offsets)
    }

    /// 当前应显示的行窗口。pending reveal 时把 active tab 所在行滚入视口，
    /// 否则仅钳制越界。**不写回** `tabs_scroll`——滚轮/拖拽才是持久滚动
    /// 状态的拥有者，render 只做虚拟修正。
    pub(super) fn rows_window(&self, rows: &[SidebarRow], heights: &[f32]) -> RowsWindow {
        let show = task_groups::visible_row_count(
            heights,
            self.tabs_viewport_h,
            tab_row_gap(self.density),
        );
        let max = task_groups::max_row_scroll(rows.len(), show);
        let scroll = if self.tabs_scroll_pending_reveal {
            let row_ix = rows
                .iter()
                .position(|row| matches!(row, SidebarRow::Tab { ix, .. } if *ix == self.active))
                .unwrap_or(0);
            task_groups::clamp_row_scroll(
                task_groups::reveal_row(row_ix, self.tabs_scroll, show),
                max,
            )
        } else {
            task_groups::clamp_row_scroll(self.tabs_scroll, max)
        };
        RowsWindow { scroll, show, max, rows: rows.len() }
    }

    /// 事件路径的精确钳制（有 `cx` 时用，行模型可现算——纯内存推导）。
    fn clamp_rows_scroll_now(&mut self, cx: &gpui::App) {
        let rows = self.sidebar_list_rows(cx);
        let (heights, _) = self.rows_geometry(&rows, cx);
        let window = self.rows_window(&rows, &heights);
        if self.tabs_scroll > window.max {
            self.tabs_scroll = window.max;
        }
    }

    /// 结构变化后的粗略钳制（无 `cx` 场景）：行数上界 = TABS 段每 tab 至多
    /// 主机头+组头+条目三行的 3n，加最近会话段每条一头一行的 2m，再加分区
    /// 头/占位行的余量。精确修正随后由滚轮/事件路径的 `clamp_rows_scroll_now`
    /// 收口。
    pub(super) fn clamp_tabs_scroll(&mut self) {
        let max = self
            .tabs
            .len()
            .saturating_mul(3)
            .saturating_add(self.recent_sessions.row_count().saturating_mul(2))
            .saturating_add(8);
        if self.tabs_scroll > max {
            self.tabs_scroll = max;
        }
    }

    /// 激活/新建 tab 的揭示。两条侧栏路径各取所需，互不干扰：
    /// 经典均匀窗口在这里即时换算写回（main 行为，无行模型可等）；分组
    /// 行模型只在 render 侧完整，交给 pending 标志换算——legacy 写入的
    /// 口径差异由 `rows_window` 的虚拟钳制兜底。顶栏（ScrollHandle）立即滚。
    pub(super) fn reveal_active_tab(&mut self) {
        let window = self.tabs_window();
        self.tabs_scroll =
            clamp_scroll(reveal_index(self.active, window.scroll, window.show), window.max);
        self.tabs_scroll_pending_reveal = true;
        self.top_tabs_scroll.scroll_to_item(if self.settings_open {
            self.tabs.len()
        } else {
            self.active
        });
    }

    pub(super) fn tabs_overlay_bar(
        &self,
        heights: &[f32],
        offsets: &[f32],
        window: &RowsWindow,
    ) -> Option<OverlayScrollbar> {
        if window.show == 0 || window.rows <= window.show {
            return None;
        }
        let gap = tab_row_gap(self.density);
        let viewport = task_groups::rows_total_height(&heights[..window.show], gap).max(1.0);
        let content = task_groups::rows_total_height(heights, gap);
        let width = self.tabs_list_width.max(1.0);
        let height = self.tabs_viewport_h.max(viewport);
        let at = window.scroll.min(offsets.len().saturating_sub(1));
        scrollbar::overlay_scrollbar((0.0, 0.0, width, height), viewport, content, offsets[at], 1.0)
    }

    /// 滚动条几何 + 允许的最大起始行，供按下/拖拽换算（事件路径现算）。
    fn scrollbar_geometry(&self, cx: &gpui::App) -> Option<(OverlayScrollbar, usize)> {
        let rows = self.sidebar_list_rows(cx);
        let (heights, offsets) = self.rows_geometry(&rows, cx);
        let window = self.rows_window(&rows, &heights);
        let bar = self.tabs_overlay_bar(&heights, &offsets, &window)?;
        Some((bar, window.max))
    }

    /// 经典侧栏的覆盖 thumb 几何（main 版，均匀行高口径）。
    fn legacy_tabs_overlay_bar(&self) -> Option<OverlayScrollbar> {
        if self.tabs_section_collapsed {
            return None;
        }
        let window = self.tabs_window();
        if window.show == 0 || window.want <= window.show {
            return None;
        }
        let viewport =
            rows_h(window.show, self.tab_row_pitch(), tab_row_gap(self.density)).max(1.0);
        let content = rows_h(window.want, self.tab_row_pitch(), tab_row_gap(self.density));
        let width = self.tabs_list_width.max(1.0);
        let height = self.tabs_viewport_h.max(viewport);
        scrollbar::overlay_scrollbar(
            (0.0, 0.0, width, height),
            viewport,
            content,
            window.scroll as f32 * self.tab_row_pitch(),
            1.0,
        )
    }

    /// 按当前侧栏模式取「(thumb 几何, 允许的最大起始行)」：分组模式现算
    /// 行模型，经典模式用均匀窗口。两模式的 press/drag 换算共用。
    fn scrollbar_target(&self, cx: &gpui::App) -> Option<(OverlayScrollbar, usize)> {
        if crate::gpui_shell::config::ai_agent_sidebar(cx) {
            self.scrollbar_geometry(cx)
        } else {
            let bar = self.legacy_tabs_overlay_bar()?;
            Some((bar, self.tabs_window().max))
        }
    }

    fn local_tabs_point(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.tabs_list_origin.x),
            f32::from(position.y - self.tabs_list_origin.y),
        )
    }

    fn press_tabs_scrollbar(&mut self, event: &MouseDownEvent, cx: &gpui::App) -> bool {
        let Some((bar, max)) = self.scrollbar_target(cx) else { return false };
        let (x, y) = self.local_tabs_point(event.position);
        if !bar.hit_test(x, y) {
            return false;
        }
        let grab = if point_in_rect(bar.thumb, x, y) { y - bar.thumb.1 } else { bar.thumb.3 * 0.5 };
        self.tabs_scroll_grab = Some(grab);
        self.tabs_scroll_pending_reveal = false;
        self.tabs_scroll = bar.target_offset(y, grab, max);
        true
    }

    fn drag_tabs_scrollbar_to(&mut self, position: Point<Pixels>, cx: &gpui::App) -> bool {
        let Some(grab) = self.tabs_scroll_grab else { return false };
        let Some((bar, max)) = self.scrollbar_target(cx) else { return false };
        let (_, y) = self.local_tabs_point(position);
        let next = bar.target_offset(y, grab, max);
        if next == self.tabs_scroll {
            return false;
        }
        self.tabs_scroll = next;
        true
    }

    fn end_tabs_scrollbar_drag(&mut self) -> bool {
        self.tabs_scroll_grab.take().is_some()
    }

    fn on_tabs_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 窗口口径按侧栏模式取：分组用行模型现算，经典用均匀窗口。
        let (scroll, max) = if crate::gpui_shell::config::ai_agent_sidebar(cx) {
            let rows = self.sidebar_list_rows(cx);
            let (heights, _) = self.rows_geometry(&rows, cx);
            let window = self.rows_window(&rows, &heights);
            (window.scroll, window.max)
        } else {
            let window = self.tabs_window();
            (window.scroll, window.max)
        };
        let delta_y = f32::from(event.delta.pixel_delta(px(tab_row_height(self.density))).y);
        let rows = wheel_rows(delta_y);
        if rows == 0 {
            return;
        }
        let next = apply_wheel(scroll, max, rows);
        if next != scroll || self.tabs_scroll_pending_reveal {
            self.tabs_scroll = next;
            self.tabs_scroll_pending_reveal = false;
            cx.notify();
        }
        cx.stop_propagation();
    }

    /// 统一列表槽（两个分区都在内）：恒定右沟槽 + 覆盖 thumb + 滚轮 +
    /// 目录树引导线。`items` 已是可见窗口；`guides` 是同一坐标系（相对
    /// 可见窗口首行）的树线矩形，垫在行底之下先画。thumb 维持「悬停/拖拽
    /// 才显示」；不溢出时 `tabs_overlay_bar` 直接给 `None`，一屏可展示就
    /// 永远没有滚动条。
    pub(super) fn wrap_rows_scroll_list(
        &self,
        items: impl IntoIterator<Item = gpui::AnyElement>,
        guides: Vec<task_groups::TreeGuide>,
        bar: Option<OverlayScrollbar>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let dragging = self.tabs_scroll_grab.is_some();
        let show_thumb = bar.is_some() && (self.tabs_list_hot || dragging);
        let workspace = cx.entity().downgrade();
        let thumb_color = {
            let theme = cx.theme();
            let alpha = if dragging {
                0.78
            } else if self.tabs_list_hot {
                0.64
            } else {
                0.46
            };
            theme.scrollbar_thumb.opacity(alpha)
        };

        div()
            .id("sidebar-tabs-list")
            .flex_1()
            .w_full()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .pr(px(TAB_SCROLL_GUTTER))
            .on_scroll_wheel(cx.listener(Self::on_tabs_wheel))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.tabs_list_hot != *hovered {
                    this.tabs_list_hot = *hovered;
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = workspace.update(cx, |this, cx| {
                            // 视口探针与最近任务门控探针合一：同一容器
                            // 常驻（幂等），侧栏不可见时不渲染即自灭。
                            this.remember_tabs_list_bounds(bounds, cx);
                            this.remember_recent_sessions_gate(cx);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                v_flex()
                    .w_full()
                    .relative()
                    .gap(px(tab_row_gap(self.density)))
                    .children(items)
                    // 树线画在行**之后**（盖在行背景之上）：激活/悬停行有
                    // 不透明底色，垫底画法会把线整段挡没（用户 10-10 报）。
                    // 线的 x 全部 ≤ 摘要文字左缘 −4，不会压到图标或文字。
                    .children(guides.iter().map(|guide| {
                        let (x, y, w, h) = guide.bounds;
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .w(px(w))
                            .h(px(h))
                            .bg(crate::gpui_shell::theme::card_divider_color(cx))
                            .debug_selector(move || {
                                format!("sidebar-guide-{}-{}-{}", guide.kind, guide.group, guide.tag)
                            })
                    })),
            )
            .when_some(bar.filter(|_| show_thumb), |list, bar| {
                list.child(tabs_scroll_thumb(&bar, thumb_color))
            })
            .when_some(bar, |list, bar| {
                list.child(
                    div()
                        .id("sidebar-tabs-scrollbar")
                        .absolute()
                        .left(px(bar.hit.0))
                        .top(px(bar.hit.1))
                        .w(px(bar.hit.2))
                        .h(px(bar.hit.3))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                if this.press_tabs_scrollbar(event, cx) {
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }),
                        ),
                )
            })
            .into_any_element()
    }

    fn remember_tabs_list_bounds(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let h = f32::from(bounds.size.height);
        let w = f32::from(bounds.size.width);
        let origin = bounds.origin;
        let height_changed = (self.tabs_viewport_h - h).abs() > 0.5;
        let width_changed = (self.tabs_list_width - w).abs() > 0.5;
        self.tabs_list_origin = origin;
        if !height_changed && !width_changed {
            return;
        }
        // 统一列表常驻（分区折叠只是行数变化），视口即面板剩余高度；仍保留
        // 最小高度下限，防止启动首帧的 0/1px 误写把窗口锁死。
        if h >= task_groups::GROUP_HEADER_H {
            self.tabs_viewport_h = h;
        }
        self.tabs_list_width = w;
        self.clamp_rows_scroll_now(cx);
        cx.notify();
    }

    pub(super) fn tabs_scrollbar_drag_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self.tabs_scroll_grab.is_none() {
            return None;
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                    if this.drag_tabs_scrollbar_to(event.position, cx) {
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        if this.end_tabs_scrollbar_drag() {
                            cx.notify();
                        }
                    }),
                )
                .into_any_element(),
        )
    }

    /// 分组折叠/展开后行数变化：事件路径（有 `cx`）就地精确钳制。
    pub(super) fn clamp_rows_after_group_toggle(&mut self, cx: &gpui::App) {
        self.clamp_rows_scroll_now(cx);
    }

    // ---- 经典侧栏（`ai_agent_sidebar` 关）的列表槽与视口探针：main 原样 ----

    /// 经典列表槽（main 版）：恒定右沟槽 + 覆盖 thumb + 滚轮，无树线、无
    /// 最近任务门控。`items` 已是可见窗口。
    pub(super) fn wrap_tabs_scroll_list(
        &self,
        items: impl IntoIterator<Item = gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let bar = self.legacy_tabs_overlay_bar();
        let dragging = self.tabs_scroll_grab.is_some();
        let show_thumb = bar.is_some() && (self.tabs_list_hot || dragging);
        let workspace = cx.entity().downgrade();
        let thumb_color = {
            let theme = cx.theme();
            let alpha = if dragging {
                0.78
            } else if self.tabs_list_hot {
                0.64
            } else {
                0.46
            };
            theme.scrollbar_thumb.opacity(alpha)
        };

        div()
            .id("sidebar-tabs-list")
            .flex_1()
            .w_full()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .pr(px(TAB_SCROLL_GUTTER))
            .on_scroll_wheel(cx.listener(Self::on_tabs_wheel))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.tabs_list_hot != *hovered {
                    this.tabs_list_hot = *hovered;
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = workspace.update(cx, |this, cx| {
                            this.legacy_remember_tabs_list_bounds(bounds, cx);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(v_flex().w_full().gap(px(tab_row_gap(self.density))).children(items))
            .when_some(bar.filter(|_| show_thumb), |list, bar| {
                list.child(
                    div()
                        .absolute()
                        .left(px(bar.thumb.0))
                        .top(px(bar.thumb.1))
                        .w(px(bar.thumb.2))
                        .h(px(bar.thumb.3))
                        .rounded_full()
                        .bg(thumb_color)
                        .occlude(),
                )
            })
            .when_some(bar, |list, bar| {
                list.child(
                    div()
                        .id("sidebar-tabs-scrollbar")
                        .absolute()
                        .left(px(bar.hit.0))
                        .top(px(bar.hit.1))
                        .w(px(bar.hit.2))
                        .h(px(bar.hit.3))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                if this.press_tabs_scrollbar(event, cx) {
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }),
                        ),
                )
            })
            .into_any_element()
    }

    /// 经典列表的视口探针（main 版）：卷帘折叠期间冻结视口高度，防止把
    /// 裁剪高度写回 `tabs_viewport_h` 锁死滚动窗口。
    fn legacy_remember_tabs_list_bounds(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let h = f32::from(bounds.size.height);
        let w = f32::from(bounds.size.width);
        let origin = bounds.origin;
        let height_changed = (self.tabs_viewport_h - h).abs() > 0.5;
        let width_changed = (self.tabs_list_width - w).abs() > 0.5;
        self.tabs_list_origin = origin;
        if !height_changed && !width_changed {
            return;
        }
        // 折叠中/已折叠：裁剪高度不是 `tabs_avail`。写回去会让
        // `visible_count` 按 20–40px 算出 0/1 行，展开后整列锁死。
        if !self.tabs_section_collapsed
            && !self.tabs_fold_frozen
            && h >= tab_row_height(self.density)
        {
            self.tabs_viewport_h = h;
        }
        self.tabs_list_width = w;
        self.clamp_tabs_scroll();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_wheel, clamp_scroll, index_visible, max_scroll, reveal_index, rows_h, tab_row_gap,
        tab_row_height, visible_count, wheel_rows,
    };

    #[test]
    fn compact_rows_keep_min_hit_target_height() {
        use super::DensityName;
        for (density, expected_height) in
            [(DensityName::Standard, 34.0), (DensityName::Compact, 32.0)]
        {
            let height = tab_row_height(density);
            assert_eq!(height, expected_height);
            assert!(height >= crate::display::ui::tokens::control::MIN_HIT_TARGET);
            assert!(tab_row_gap(density) > 0.0);
        }
    }

    #[test]
    fn compact_rows_fit_more_tabs_without_shrinking_hit_targets() {
        use super::DensityName;
        for (density, expected_height, expected_show) in
            [(DensityName::Standard, 34.0, 2), (DensityName::Compact, 32.0, 3)]
        {
            let height = tab_row_height(density);
            let gap = tab_row_gap(density);
            let pitch = height + gap;
            assert_eq!(height, expected_height);
            assert!(height >= crate::display::ui::tokens::control::MIN_HIT_TARGET);
            assert_eq!(rows_h(1, pitch, gap), height);
            let show = visible_count(10, 104.0, pitch, gap);
            assert_eq!(show, expected_show);
            assert_eq!(reveal_index(9, 0, show), max_scroll(10, show));
        }
    }

    const PITCH: f32 = 42.0;
    const GAP: f32 = 8.0;

    #[test]
    fn row_height_drops_the_trailing_gap() {
        assert_eq!(rows_h(0, PITCH, GAP), 0.0);
        assert_eq!(rows_h(1, PITCH, GAP), 34.0);
        assert_eq!(rows_h(3, PITCH, GAP), 3.0 * PITCH - GAP);
    }

    #[test]
    fn visible_count_fits_whole_rows_into_the_band() {
        // 100px 放下 2 行（76px），放不下 3 行（118px）。
        assert_eq!(visible_count(10, 100.0, PITCH, GAP), 2);
        assert_eq!(visible_count(2, 100.0, PITCH, GAP), 2);
        assert_eq!(visible_count(10, 0.0, PITCH, GAP), 10);
        assert_eq!(visible_count(0, 100.0, PITCH, GAP), 0);
        // 旧壳：不够一行就是 0，不能退成「一行滚动区」。
        assert_eq!(visible_count(10, 20.0, PITCH, GAP), 0);
    }

    #[test]
    fn scrollbar_only_exists_when_the_window_overflows() {
        assert_eq!(max_scroll(10, 10), 0);
        assert_eq!(max_scroll(10, 3), 7);
        assert_eq!(clamp_scroll(9, 7), 7);
        assert!(!index_visible(0, 2, 3));
        assert!(index_visible(2, 2, 3));
        assert!(index_visible(4, 2, 3));
        assert!(!index_visible(5, 2, 3));
    }

    #[test]
    fn activating_a_scrolled_out_tab_moves_the_window() {
        assert_eq!(reveal_index(0, 2, 3), 0);
        assert_eq!(reveal_index(3, 2, 3), 2);
        assert_eq!(reveal_index(9, 0, 3), 7);
    }

    #[test]
    fn wheel_steps_one_row_and_clamps() {
        assert_eq!(wheel_rows(12.0), -1);
        assert_eq!(wheel_rows(-3.0), 1);
        assert_eq!(wheel_rows(0.0), 0);
        assert_eq!(apply_wheel(0, 5, -1), 0);
        assert_eq!(apply_wheel(5, 5, 1), 5);
        assert_eq!(apply_wheel(2, 5, 1), 3);
    }

    #[cfg(feature = "gpui-test-support")]
    mod hover {
        use super::super::{scrollbar, tabs_scroll_thumb};
        use gpui::prelude::*;
        use gpui::{Context, MouseButton, Render, TestAppContext, Window, div, point, px};

        #[derive(Default)]
        struct ScrollHoverProbe {
            hovered: bool,
            presses: usize,
            hover_transitions: usize,
        }

        impl Render for ScrollHoverProbe {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let bar =
                    scrollbar::overlay_scrollbar((0.0, 0.0, 200.0, 200.0), 200.0, 800.0, 0.0, 1.0)
                        .unwrap();
                div()
                    .id("tabs-hover-probe")
                    .relative()
                    .w(px(200.0))
                    .h(px(200.0))
                    .on_hover(cx.listener(|this, hovered, _, cx| {
                        if this.hovered != *hovered {
                            this.hovered = *hovered;
                            this.hover_transitions += 1;
                            // 本场景只进出列表各一次；直接拦住反馈循环，避免回归时挂住测试。
                            assert!(
                                this.hover_transitions <= 2,
                                "ancestor hover keeps oscillating"
                            );
                            cx.notify();
                        }
                    }))
                    .when(self.hovered, |list| list.child(tabs_scroll_thumb(&bar, gpui::black())))
                    .child(
                        div()
                            .id("tabs-hover-track")
                            .absolute()
                            .left(px(bar.hit.0))
                            .top(px(bar.hit.1))
                            .w(px(bar.hit.2))
                            .h(px(bar.hit.3))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.presses += 1;
                                    cx.stop_propagation();
                                }),
                            ),
                    )
            }
        }

        #[gpui::test]
        fn thumb_keeps_parent_hover_stable_and_track_clickable(cx: &mut TestAppContext) {
            let (probe, cx) = cx.add_window_view(|_, _| ScrollHoverProbe::default());
            let draw = |cx: &mut gpui::VisualTestContext| {
                cx.update(|window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                });
            };
            cx.simulate_mouse_move(point(px(195.0), px(190.0)), None, Default::default());
            draw(cx);
            let thumb = cx.debug_bounds("sidebar-tabs-scroll-thumb").expect("hover shows thumb");
            cx.simulate_mouse_move(thumb.center(), None, Default::default());
            // 鼠标保持不动、连续重绘，复现遮挡父 hover 导致的显示/隐藏反馈循环。
            for frame in 0..8 {
                draw(cx);
                assert!(probe.read_with(cx, |probe, _| probe.hovered), "frame {frame}");
                assert_eq!(cx.debug_bounds("sidebar-tabs-scroll-thumb"), Some(thumb));
            }
            cx.simulate_mouse_down(thumb.center(), MouseButton::Left, Default::default());
            cx.simulate_mouse_up(thumb.center(), MouseButton::Left, Default::default());
            assert_eq!(probe.read_with(cx, |probe, _| probe.presses), 1);
            cx.simulate_mouse_move(point(px(220.0), px(100.0)), None, Default::default());
            draw(cx);
            assert!(!probe.read_with(cx, |probe, _| probe.hovered));
            assert!(cx.debug_bounds("sidebar-tabs-scroll-thumb").is_none());
        }
    }
}
