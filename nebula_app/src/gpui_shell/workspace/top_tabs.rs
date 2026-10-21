//! 自定义标题栏里的横向标签布局。
//!
//! 这里只负责同一组 workspace tab 的第二种呈现；激活、关闭、重命名、排序
//! 与 dock 都调用 `NebulaWorkspace` 既有动作，不维护平行状态。

mod label;
pub(super) mod layout;
mod style;

use layout::{MAX_WIDTH as TOP_TAB_MAX_W, MIN_WIDTH as TOP_TAB_MIN_W, TabStripLayout};
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, App, ClickEvent, Context, FontWeight, InteractiveElement as _,
    IntoElement as _, KeyDownEvent, MouseButton, MouseDownEvent, ObjectFit, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div,
    ease_out_quint, img, px,
};
use gpui_component::menu::PopupMenuItem;

use crate::gpui_shell::prelude::*;
use crate::gpui_shell::terminal::view::SidebarActivity;
use crate::gpui_shell::widgets::toolbar_button;
use crate::i18n::{Message, UiLanguage};

use super::{
    NebulaWorkspace, NewWindow, OpenSettings, TAB_LABEL_ICON_SIZE, TAB_LABEL_ICON_W, TabDrag,
    TabDragAxis, TabPresentation, ToggleShellPicker, pane_header, title_bar_panel_controls,
};

/// 与顶部模式正文卡片的 `px_2` 左边距同源，让首个 tab 和终端左缘对齐。
pub(super) const TOP_TAB_LEFT_INSET: f32 = 8.0;
const TOP_TAB_STATUS_W: f32 = 28.0;
/// 标题栏中不属于 tab 视口的固定预算：左内边距、五枚 32px 操作按钮、
/// 三枚 34px 窗口按钮，以及至少 72px 的可拖拽空白。
const TOP_TAB_RESERVED_W: f32 = TOP_TAB_LEFT_INSET + 32.0 * 5.0 + 34.0 * 3.0 + 72.0;
/// 拖拽越界自动滚的触发带宽：指针进到视口左/右这么近就开始滚。
const TOP_TAB_EDGE_BAND: f32 = 24.0;

/// GPUI 横向滚动偏移是 ≤ 0（内容向左移）。
/// 可滚范围因此是 `[-(strip - viewport), 0]`。
fn clamp_strip_offset(x: f32, strip_w: f32, viewport_w: f32) -> f32 {
    let max = (strip_w - viewport_w).max(0.0);
    x.clamp(-max, 0.0)
}

/// 一次翻页：`dir` 为 -1 向左（露出前面的 tab）、+1 向右。
fn nudge_offset(x: f32, step: f32, dir: f32, strip_w: f32, viewport_w: f32) -> f32 {
    clamp_strip_offset(x - dir * step, strip_w, viewport_w)
}

/// 已经贴住某一端时按钮要灰掉——一枚点了没反应的箭头比没有箭头更让人怀疑
/// 是不是卡了。
fn at_strip_start(x: f32) -> bool {
    x >= -0.5
}

fn at_strip_end(x: f32, strip_w: f32, viewport_w: f32) -> bool {
    x <= -(strip_w - viewport_w).max(0.0) + 0.5
}

/// 顶部加号的生产调用点与鼠标测试共用同一个元素构造，避免测试只证明
/// `title_bar_panel_controls` 本身，却漏掉真实按钮没有接入它。
pub(super) fn top_new_tab_control(
    language: UiLanguage,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    title_bar_panel_controls().h_auto().child(
        Button::new("top-new-tab")
            .icon(IconName::Plus)
            .ghost()
            .tooltip(language.text(Message::ChromeNewTerminalCtrlShiftT))
            .on_click(on_click),
    )
}

/// “更多”入口的生产按钮与几何探针共用同一个构造，避免测试用近似尺寸替代。
pub(super) fn top_tabs_menu_button(settings_active: bool, language: UiLanguage) -> Button {
    Button::new("top-tabs-menu")
        .icon(IconName::EllipsisVertical)
        .ghost()
        .selected(settings_active)
        .tooltip(language.text(Message::ChromeMore))
}

/// 紧邻 TabView 的操作按钮占满同一条标签行，再在槽内居中 32px 按钮。
/// 外层仍贴标题栏底边，tab 与正文相接的既有布局不变。
pub(super) fn top_tab_action_slot(
    density: nebula_settings::DensityName,
    child: impl gpui::IntoElement,
) -> gpui::Div {
    h_flex().h(px(super::tab_scroll::tab_row_height(density))).items_center().child(child)
}

impl NebulaWorkspace {
    /// Shell 面板缓存打开时的候选；导入后只刷新现有面板，关闭态下次打开会读盘。
    pub(super) fn refresh_shell_if_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell_picker_open {
            self.open_shell_palette(window, cx);
        }
    }

    pub(super) fn render_top_title_bar(
        &self,
        settings_active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row_height = super::tab_scroll::tab_row_height(self.density);
        let language = crate::gpui_shell::config::ui_language(cx);
        let port_forward_button = self.render_port_forward_button(cx);
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let active_bg = theme.sidebar_accent;
        let tab_style = cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .map(|s| s.tab_style)
            .unwrap_or_default();
        // Chrome's 12px flared feet need another 8px inside the visible body.
        let tab_padding = if tab_style == nebula_settings::TabStyle::Chrome { 20.0 } else { 12.0 };
        let active_fg = if tab_style == nebula_settings::TabStyle::Chrome {
            theme.sidebar_accent_foreground
        } else {
            theme.foreground
        };
        let hover_bg = theme.list_hover;
        let dark = theme.is_dark();
        let settings = cx.try_global::<crate::gpui_shell::config::Settings>();
        let tab_close_visible = settings.map(|settings| settings.tab_close_visible).unwrap_or(true);
        let tab_reveal = settings
            .map(|settings| settings.tab_reveal)
            .unwrap_or(nebula_settings::TabRevealName::Slide);
        let chrome_family = theme.font_family.clone();
        let metadata_family = theme.mono_font_family.clone();
        let symbol_family: SharedString = crate::font_install::REQUIRED_FONT_FAMILY.into();
        let label_px = settings.map(|settings| settings.ui_font_size_px).unwrap_or(15.0);
        let port_forward_w = if port_forward_button.is_some() { 32.0 } else { 0.0 };
        let tab_capacity_w =
            (f32::from(window.viewport_size().width) - TOP_TAB_RESERVED_W - port_forward_w)
                .max(TOP_TAB_MIN_W);
        let presentations: Vec<_> =
            (0..self.top_tab_count()).map(|ix| self.top_tab_presentation(ix, cx, dark)).collect();
        let top_tab_count = presentations.len();
        let status_widths: Vec<_> = presentations
            .iter()
            .map(|tab| {
                Self::shell_status_width(
                    window,
                    tab.shell_tag.as_ref(),
                    &metadata_family,
                    label_px * 0.8,
                    TOP_TAB_STATUS_W,
                    TOP_TAB_MAX_W * 0.4,
                )
            })
            .collect();
        let text_widths: Vec<_> = presentations
            .iter()
            .map(|tab| {
                layout::text_width(tab.title.clone(), chrome_family.clone(), label_px, window)
            })
            .collect();
        let widths: Vec<_> = presentations
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                if tab.renaming.is_some() {
                    return TOP_TAB_MAX_W;
                }
                let icon = if tab.is_settings
                    || tab.logo_image.is_some()
                    || tab.program_glyph.is_some()
                    || tab.pane_count > 1
                {
                    TAB_LABEL_ICON_SIZE.max(TAB_LABEL_ICON_W) + 8.0
                } else {
                    0.0
                };
                let admin = if tab.local_administrator { TAB_LABEL_ICON_SIZE + 8.0 } else { 0.0 };
                let badge = if tab.pane_count > 1 {
                    pane_header::split_badge_slot_w(label_px) + 8.0
                } else {
                    0.0
                };
                text_widths[ix]
                    + if tab_style == nebula_settings::TabStyle::Tag { 2.0 } else { 0.0 }
                    + tab_padding * 2.0
                    + icon
                    + admin
                    + badge
                    + 8.0
                    + status_widths[ix]
            })
            .collect();
        let inset = crate::platform::window_chrome::layout(window)
            .map_or(TOP_TAB_LEFT_INSET, |(_, inset)| inset);
        let layout = self
            .tab_drag
            .as_ref()
            .and_then(|drag| drag.horizontal.as_ref())
            .filter(|(layout, _)| layout.tab_count() == top_tab_count)
            .map(|(layout, _)| layout.clone())
            .unwrap_or_else(|| {
                TabStripLayout::new(
                    widths.iter().copied(),
                    tab_capacity_w,
                    inset,
                    layout::gap(tab_style),
                )
            });
        let strip_w = layout.width;
        let overflow = layout.overflow;
        let tab_viewport_w = layout.viewport;
        let scroll_x = f32::from(self.top_tabs_scroll.offset().x);
        let drag = self
            .tab_drag
            .as_ref()
            .filter(|drag| drag.active && drag.axis == TabDragAxis::Horizontal)
            .map(|drag| (drag.source, Self::drag_slot(drag, self.tabs.len()), drag.offset));
        let items = presentations.into_iter().enumerate()
            .map(|(ix, presentation)| {
                let tab_w = layout.tab_width(ix);
                let drag_layout = layout.clone();
                let settings_navigation = self.settings_tab_open && ix == self.tabs.len();
                let active = if settings_navigation {
                    self.settings_open
                } else {
                    !self.settings_open && ix == self.active
                };
                let TabPresentation {
                    title,
                    tooltip,
                    is_settings,
                    activity,
                    logo_image,
                    logo_pending,
                    program_glyph,
                    shell_tag,
                    color,
                    renaming,
                    pane_count,
                    local_administrator,
                } = presentation;
                let tooltip = Some(tooltip.unwrap_or_else(|| title.clone()));
                let hover_group: SharedString = format!("top-tab-hover-{ix}").into();
                let cross_window_drag = self.cross_window_drag_payload(ix, cx);
                let status_width = status_widths[ix];
                let resting_status: Option<gpui::AnyElement> = match activity {
                    SidebarActivity::Running => {
                        let (track, head) =
                            crate::gpui_shell::theme::sidebar_spinner_colors(cx, active);
                        Some(Self::spinner(self.spinner_phase, track, head, cx.weak_entity()).into_any_element())
                    },
                    SidebarActivity::Paused => Some(
                        Icon::new(IconName::Pause)
                            .xsmall()
                            .text_color(theme.warning)
                            .into_any_element(),
                    ),
                    SidebarActivity::Done => Some(
                        div().size(px(6.0)).rounded_full().bg(theme.primary).into_any_element(),
                    ),
                    SidebarActivity::Completed => Some(
                        Icon::new(IconName::Check)
                            .xsmall()
                            .text_color(theme.success)
                            .into_any_element(),
                    ),
                    SidebarActivity::CommandFailed => Some(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(theme.danger)
                            .into_any_element(),
                    ),
                    SidebarActivity::WaitingInput => Some(
                        Self::waiting_hand(symbol_family.clone(), label_px, theme.primary)
                            .into_any_element(),
                    ),
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
                        Self::shell_status_label(tag, metadata_family.clone(), label_px * 0.8, muted)
                            .into_any_element()
                    }),
                };
                let strip = color.map(|color| gpui::Rgba {
                    r: color.r as f32 / 255.0,
                    g: color.g as f32 / 255.0,
                    b: color.b as f32 / 255.0,
                    a: 1.0,
                });
                let (dragged, shift) = match drag {
                    Some((source, _, _)) if ix == source => (true, 0.0),
                    Some((source, target, _)) if source < target && ix > source && ix <= target => {
                        (false, layout.shift(ix, source, target))
                    },
                    Some((source, target, _)) if source > target && ix >= target && ix < source => {
                        (false, layout.shift(ix, source, target))
                    },
                    _ => (false, 0.0),
                };

                let row = h_flex()
                    .id(("top-tab", ix))
                    .when_some(tooltip, |row, text| row.tooltip(move |window, cx| {
                        super::tab_presentation::tooltip(text.clone(), window, cx)
                    }))
                    .debug_selector(|| format!("top-tab-{ix}"))
                    .group(hover_group.clone())
                    .relative()
                    .w(px(tab_w))
                    .h(px(row_height))
                    .flex_shrink_0()
                    .min_w_0()
                    .overflow_hidden()
                    .items_center()
                    .gap_2()
                    .px(px(tab_padding))
                    .font_family(chrome_family.clone())
                    .map(|row| style::apply(row, tab_style, active, dragged, hover_group.clone(), cx))
                    .font_weight(FontWeight::NORMAL)
                    .cursor_pointer()
                    // TitleBar 的父层是 WindowControlArea::Drag；tab 必须自己
                    // 占住普通鼠标命中区，滚轮仍交给外层标签滚动容器。
                    .block_mouse_except_scroll()
                    .when(active, |item| item.text_color(active_fg))
                    .when(!active && !dragged, |item| {
                        item.text_color(muted).when(tab_style != nebula_settings::TabStyle::Tag, |item| {
                            item.hover(|style| style.bg(hover_bg))
                        })
                    })
                    .when(!active && dragged, |item| {
                        item.text_color(muted).when(tab_style != nebula_settings::TabStyle::Tag, |item| item.bg(hover_bg))
                    })
                    .on_mouse_move(cx.listener(|this, event, window, cx| {
                        // Occluding tabs own hover; their ancestors may not receive this move.
                        if this.tab_drag.is_some() {
                            this.update_tab_drag(event, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.activate_top_tab(ix, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if settings_navigation {
                                return;
                            }
                            this.tab_drag = Some(TabDrag {
                                source: ix,
                                cross_window: this.cross_window_drag_payload(ix, cx),
                                cross_window_target: None,
                                press_x: f32::from(event.position.x),
                                press_y: f32::from(event.position.y),
                                axis: TabDragAxis::Horizontal,
                                pitch: tab_w + drag_layout.gap,
                                horizontal: Some((drag_layout.clone(), scroll_x)),
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
                            this.close_top_tab(ix, window, cx);
                        }),
                    )
                    .on_double_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if !settings_navigation {
                            this.begin_rename(ix, window, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if !settings_navigation {
                                this.open_tab_context_menu(ix, event.position, window, cx);
                            }
                        }),
                    )
                    .when_some(strip, |row, color| {
                        row.child(
                            div()
                                .absolute()
                                .left(px(6.0))
                                .right(px(6.0))
                                .bottom_0()
                                .h(px(2.5))
                                .rounded_full()
                                .bg(color),
                        )
                    })
                    .when(local_administrator, |row| {
                        row.child(super::tab_presentation::administrator_badge(
                            format!("top-tab-admin-{ix}").into(),
                            cx,
                        ))
                    })
                    // 图标优先级与侧栏同源：先身份（跟随聚焦 pane），分屏标记
                    // 只在没有身份图标时补位。理由见 sidebar.rs 同处注释。
                    .when(is_settings, |row| {
                        row.child(
                            div()
                                .w(px(TAB_LABEL_ICON_W))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::Settings)
                                        .small()
                                        .text_color(if active { active_fg } else { muted }),
                                ),
                        )
                    })
                    .when_some(logo_image.clone(), |row, image| {
                        row.child(
                            img(image)
                                .size(px(TAB_LABEL_ICON_SIZE))
                                .flex_shrink_0()
                                .object_fit(ObjectFit::Contain),
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
                        pane_count > 1
                            && !is_settings
                            && logo_image.is_none()
                            && program_glyph.is_none(),
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
                    .child(match renaming {
                        Some(input) => div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_key_down(cx.listener(
                                |this, event: &KeyDownEvent, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        this.cancel_rename(window, cx);
                                    }
                                },
                            ))
                            .child(
                                Input::new(&input)
                                    .w_full()
                                    .text_size(px(label_px))
                                    .font_family(chrome_family.clone()),
                            )
                            .into_any_element(),
                        None => label::render(
                            ix,
                            title,
                            chrome_family.clone(),
                            label_px,
                            text_widths[ix],
                            (widths[ix] - tab_w).max(0.0),
                        ),
                    })
                    .when(pane_count > 1, |row| {
                        row.child(
                            div()
                                .id(("top-pane-count", ix))
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
                            .when_some(resting_status.filter(|_| active), |slot, status| {
                                slot.child(
                                    h_flex()
                                        .debug_selector(move || format!("top-tab-resting-status-{ix}"))
                                        .absolute()
                                        .inset_0()
                                        .justify_end()
                                        .items_center()
                                        .group_hover(hover_group.clone(), |item| item.invisible())
                                        .child(status),
                                )
                            })
                            .child(
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .justify_end()
                                    .items_center()
                                    .invisible()
                                    .group_hover(hover_group, |slot| slot.visible())
                                    .when(tab_close_visible || settings_navigation, |slot| {
                                            slot.child(
                                                Button::new(("top-close-tab", ix))
                                                    .debug_selector(move || format!("top-close-tab-{ix}"))
                                                    .icon(IconName::Close)
                                                    .ghost()
                                                    .xsmall()
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            cx.stop_propagation();
                                                            this.close_top_tab(ix, window, cx);
                                                        },
                                                    )),
                                            )
                                        }),
                            ),
                    )
                    .when_some(cross_window_drag, |row, payload| {
                        row.on_drag(payload, |payload, _, _, cx| {
                            NebulaWorkspace::cross_window_drag_preview(payload, cx)
                        })
                    });

                let row = if dragged {
                    gpui::deferred(
                        row.left(px(drag.map(|(_, _, offset)| offset).unwrap_or(0.0))).shadow_md(),
                    )
                    .with_priority(1)
                    .into_any_element()
                } else if shift != 0.0 {
                    if tab_reveal == nebula_settings::TabRevealName::Instant {
                        row.left(px(shift)).into_any_element()
                    } else {
                        row.with_animation(
                            ("top-tab-make-way", ix),
                            Animation::new(Duration::from_millis(120))
                                .with_easing(ease_out_quint()),
                            move |row, t| row.left(px(shift * t)),
                        )
                        .into_any_element()
                    }
                } else {
                    row.into_any_element()
                };
                if active && !dragged {
                    gpui::deferred(row).into_any_element()
                } else {
                    row
                }
            })
            .collect::<Vec<_>>();

        let menu_workspace = cx.entity().downgrade();

        h_flex()
            .size_full()
            .min_w_0()
            .items_center()
            .child(
                // TabView 贴住标题栏底边；只让 tab 与其相邻操作按钮下沉，
                // 标题栏两侧的独立工具仍保持垂直居中。
                h_flex()
                    .h_full()
                    .flex_shrink(1.0)
                    .min_w_0()
                    .items_end()
                    .when(tab_style == nebula_settings::TabStyle::Tag, |bar| bar.items_center())
                    // 溢出翻页：只在装不下时出现。滚轮已经能滚，但滚轮是个
                    // 看不见的入口——tab 一多，用户根本不知道右边还有东西。
                    .when(overflow, |bar| {
                        let nudge_layout = layout.clone();
                        bar.child(
                            div().w(px(layout::NUDGE_WIDTH)).h(px(row_height)).flex_shrink_0().flex().items_center().justify_center().child(
                                title_bar_panel_controls().h_auto().child(
                                    Button::new("top-tabs-prev")
                                        .icon(IconName::ChevronLeft)
                                        .ghost()
                                        .xsmall()
                                        .disabled(at_strip_start(scroll_x))
                                        .tooltip(language.text(Message::ChromeScrollTabsLeft))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.nudge_top_tabs(-1.0, &nudge_layout, cx);
                                        })),
                                ),
                            ),
                        )
                    })
                    .child(
                        div()
                            .id("top-tabs-viewport")
                            .debug_selector(|| "top-tabs-viewport".to_owned())
                            .relative()
                            .w(px(tab_viewport_w))
                            .flex_shrink(1.0)
                            .min_w_0()
                            .h(px(row_height))
                            .overflow_hidden()
                            // 保留内部滚动命中，截断外层原生标题栏 Drag 命中区。
                            .occlude()
                            // 待命拖拽在越过 4px 前由此接收移动；激活后根罩层接管。
                            .on_mouse_move(cx.listener(|this, event, window, cx| {
                                this.update_tab_drag(event, window, cx);
                            }))
                            // 内层 GPUI scroller 是唯一偏移所有者；这里只消费事件。
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .child(
                                h_flex()
                                    .id("top-tabs-scroll")
                                    .size_full()
                                    .items_center()
                                    .gap(px(layout.gap))
                                    .overflow_x_scroll()
                                    .track_scroll(&self.top_tabs_scroll)
                                    .children(items),
                            ),
                    )
                    .when(overflow, |bar| {
                        let nudge_layout = layout.clone();
                        bar.child(
                            div().w(px(layout::NUDGE_WIDTH)).h(px(row_height)).flex_shrink_0().flex().items_center().justify_center().child(
                                title_bar_panel_controls().h_auto().child(
                                    Button::new("top-tabs-next")
                                        .icon(IconName::ChevronRight)
                                        .ghost()
                                        .xsmall()
                                        .disabled(at_strip_end(scroll_x, strip_w, tab_viewport_w))
                                        .tooltip(language.text(Message::ChromeScrollTabsRight))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.nudge_top_tabs(1.0, &nudge_layout, cx);
                                        })),
                                ),
                            ),
                        )
                    })
                    .child(
                        top_tab_action_slot(self.density, top_new_tab_control(language, cx.listener(
                            |this, _, window, cx| {
                                this.add_terminal(window, cx);
                            },
                        ))),
                    )
                    .child(
                        top_tab_action_slot(
                            self.density,
                            top_tabs_menu_button(settings_active, language).dropdown_menu_with_anchor(
                                gpui::Anchor::TopRight,
                                move |menu, _, _| {
                                    let shell_picker = menu_workspace.clone();
                                    let new_window = menu_workspace.clone();
                                    let settings = menu_workspace.clone();
                                    menu.external_link_icon(false)
                                        .item(
                                            PopupMenuItem::new(language.text(Message::CommonNewWindow))
                                                .icon(IconName::Plus)
                                                .action(Box::new(NewWindow))
                                                .on_click(move |_, _, cx| {
                                                    if new_window.upgrade().is_some() {
                                                        cx.defer(|cx| {
                                                            if let Err(error) = crate::gpui_shell::workspace::windowing::open_new_window(cx, None) {
                                                                log::warn!("failed to open GPUI window: {error}");
                                                            }
                                                        });
                                                    }
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new(language.text(Message::ChromeSelectTerminal))
                                                .icon(IconName::SquareTerminal)
                                                .action(Box::new(ToggleShellPicker))
                                                .on_click(move |_, window, cx| {
                                                    if let Some(workspace) = shell_picker.upgrade() {
                                                        workspace.update(cx, |this, cx| {
                                                            this.open_shell_palette(window, cx);
                                                        });
                                                    }
                                                }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new(language.text(Message::CommonSettings))
                                                .icon(IconName::Settings)
                                                .action(Box::new(OpenSettings))
                                                .on_click(move |_, window, cx| {
                                                    if let Some(workspace) = settings.upgrade() {
                                                        workspace.update(cx, |this, cx| {
                                                            this.toggle_settings(window, cx);
                                                        });
                                                    }
                                                }),
                                        )
                                },
                            ),
                        ),
                    ),
            )
            // 新建 split button 紧跟 TabView；剩余空间才是可拖拽标题栏，
            // 共享详情侧栏入口固定在其右侧。
            .child(div().h_full().flex_1().min_w_0())
            .child(
                title_bar_panel_controls()
                    .gap(px(8.0))
                    .child(self.render_terminal_find_button(settings_active, cx))
                    .child(
                        toolbar_button(
                            "top-toggle-command-manager",
                            Icon::new(Icon::empty())
                                .path(crate::gpui_shell::assets::nav::COMMAND_MANAGER),
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

    fn nudge_top_tabs(&mut self, dir: f32, layout: &TabStripLayout, cx: &mut Context<Self>) {
        let mut offset = self.top_tabs_scroll.offset();
        offset.x = px(layout.nudge(f32::from(offset.x), dir));
        self.top_tabs_scroll.set_offset(offset);
        cx.notify();
    }

    /// Use the same frozen geometry as the drag gesture, including optional toolbar controls.
    pub(super) fn autoscroll_top_tabs_for_drag(
        &mut self,
        pointer_x: f32,
        _window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some((layout, _)) = self.tab_drag.as_ref().and_then(|drag| drag.horizontal.as_ref())
        else {
            return;
        };
        if !layout.overflow {
            return;
        }
        let dir = if pointer_x <= layout.left + TOP_TAB_EDGE_BAND {
            -1.0
        } else if pointer_x >= layout.left + layout.viewport - TOP_TAB_EDGE_BAND {
            1.0
        } else {
            return;
        };
        let mut offset = self.top_tabs_scroll.offset();
        offset.x = px(nudge_offset(f32::from(offset.x), 48.0, dir, layout.width, layout.viewport));
        self.top_tabs_scroll.set_offset(offset);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nudge_buttons_only_exist_once_the_strip_overflows() {
        assert!(!TabStripLayout::new([200.0; 3], 800.0, 0.0, layout::GAP).overflow);
        assert!(!TabStripLayout::new([200.0; 3], 608.0, 0.0, layout::GAP).overflow);
        assert!(TabStripLayout::new([200.0; 3], 607.0, 0.0, layout::GAP).overflow);
    }

    #[test]
    fn strip_offset_stays_inside_the_scrollable_range() {
        // 可滚 200px：偏移合法区间是 [-200, 0]。
        assert_eq!(clamp_strip_offset(50.0, 1000.0, 800.0), 0.0);
        assert_eq!(clamp_strip_offset(-500.0, 1000.0, 800.0), -200.0);
        assert_eq!(clamp_strip_offset(-120.0, 1000.0, 800.0), -120.0);
        // 装得下时根本不该有偏移。
        assert_eq!(clamp_strip_offset(-30.0, 600.0, 800.0), 0.0);
    }

    #[test]
    fn nudge_walks_one_step_and_stops_at_both_ends() {
        assert_eq!(nudge_offset(0.0, 124.0, 1.0, 1000.0, 800.0), -124.0);
        assert_eq!(nudge_offset(-124.0, 124.0, -1.0, 1000.0, 800.0), 0.0);
        // 末端：再往右也只到 -200。
        assert_eq!(nudge_offset(-150.0, 124.0, 1.0, 1000.0, 800.0), -200.0);
        // 首端：再往左也只到 0。
        assert_eq!(nudge_offset(-50.0, 124.0, -1.0, 1000.0, 800.0), 0.0);
    }

    #[test]
    fn end_state_drives_the_disabled_arrows() {
        assert!(at_strip_start(0.0));
        assert!(!at_strip_start(-124.0));
        assert!(at_strip_end(-200.0, 1000.0, 800.0));
        assert!(!at_strip_end(-100.0, 1000.0, 800.0));
        // 装得下时两端同时成立——按钮本来也不画。
        assert!(at_strip_start(0.0));
        assert!(at_strip_end(0.0, 600.0, 800.0));
    }
}
