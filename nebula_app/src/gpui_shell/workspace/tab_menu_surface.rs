//! Shared presentation for the two flat menus in the tab area.

use gpui::{
    App, AppContext as _, Bounds, BoxShadow, ClickEvent, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement as _, IntoElement as _,
    KeyBinding, ParentElement as _, Pixels, Render, Role, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Window, div, hsla, point,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{kbd::Kbd, menu::PopupMenuItem};
use std::{cell::Cell, rc::Rc};

use crate::gpui_shell::{prelude::*, theme};

const KEY_CONTEXT: &str = "PebrelTabMenu";
gpui::actions!(tab_popup, [SelectPrevious, SelectNext, ActivateItem, DismissMenu]);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", ActivateItem, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", DismissMenu, Some(KEY_CONTEXT)),
    ]);
}

struct MenuRow {
    item: PopupMenuItem,
    hint: Option<&'static str>,
    radio: Option<bool>,
}

pub(super) struct TabMenuSurface {
    items: Vec<MenuRow>,
    group_trigger: Option<Rc<Cell<Bounds<Pixels>>>>,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    keyboard_row: Option<usize>,
    scroll: ScrollHandle,
    min_width: Pixels,
}

impl TabMenuSurface {
    pub(super) fn build(
        cx: &mut App,
        min_width: Pixels,
        populate: impl FnOnce(Self) -> Self,
    ) -> Entity<Self> {
        cx.new(|cx| {
            populate(Self {
                items: Vec::new(),
                group_trigger: None,
                focus: cx.focus_handle(),
                return_focus: None,
                keyboard_row: None,
                scroll: ScrollHandle::default(),
                min_width,
            })
        })
    }

    pub(super) fn grouping(mut self, trigger: Rc<Cell<Bounds<Pixels>>>) -> Self {
        self.group_trigger = Some(trigger);
        self
    }

    pub(super) fn item(self, item: PopupMenuItem) -> Self {
        self.push(item, None, None)
    }

    pub(super) fn with_hint(self, item: PopupMenuItem, hint: &'static str) -> Self {
        self.push(item, Some(hint), None)
    }

    pub(super) fn choice(
        self,
        item: PopupMenuItem,
        checked: bool,
        hint: Option<&'static str>,
    ) -> Self {
        self.push(item, hint, Some(checked))
    }

    fn push(
        mut self,
        item: PopupMenuItem,
        hint: Option<&'static str>,
        radio: Option<bool>,
    ) -> Self {
        debug_assert!(!matches!(item, PopupMenuItem::Submenu { .. }));
        self.items.push(MenuRow { item, hint, radio });
        self
    }

    pub(super) fn separator(self) -> Self {
        if self.items.last().is_some_and(|row| !matches!(row.item, PopupMenuItem::Separator)) {
            self.item(PopupMenuItem::separator())
        } else {
            self
        }
    }

    pub(super) fn remember_focus(&mut self, focus: Option<FocusHandle>) {
        self.return_focus = focus;
    }

    fn enabled(item: &PopupMenuItem) -> bool {
        matches!(item, PopupMenuItem::Item { disabled: false, handler: Some(_), .. })
    }

    fn select(&mut self, reverse: bool, cx: &mut Context<'_, Self>) {
        let enabled = || {
            self.items
                .iter()
                .enumerate()
                .filter_map(|(ix, row)| Self::enabled(&row.item).then_some(ix))
        };
        self.keyboard_row = if reverse {
            enabled()
                .rev()
                .find(|&ix| self.keyboard_row.is_some_and(|row| ix < row))
                .or_else(|| enabled().next_back())
        } else {
            enabled()
                .find(|&ix| self.keyboard_row.is_some_and(|row| ix > row))
                .or_else(|| enabled().next())
        };
        if let Some(ix) = self.keyboard_row {
            self.scroll.scroll_to_item(ix);
        }
        cx.notify();
    }

    pub(super) fn dismiss(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(focus) = &self.return_focus
            && self.focus.contains_focused(window, cx)
        {
            focus.focus(window, cx);
        }
        cx.emit(DismissEvent);
    }

    fn activate(
        &self,
        ix: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(PopupMenuItem::Item { disabled: false, handler: Some(handler), .. }) =
            self.items.get(ix).map(|row| &row.item)
        else {
            return;
        };
        cx.stop_propagation();
        window.prevent_default();
        handler(event, window, cx);
        self.dismiss(window, cx);
    }

    fn row(
        &self,
        ix: usize,
        row: &MenuRow,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        let palette = cx.theme();
        let scale = f32::from(palette.font_size) / 14.0;
        let faint = theme::faint_ink(cx);
        let grouping = self.group_trigger.is_some();
        let item = &row.item;
        match item {
            PopupMenuItem::Label(label) => div()
                .px(px(10.0))
                .pt(px(if grouping { 8.0 } else { 7.0 }))
                .pb(px(4.0))
                .text_size(px(10.5 * scale))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(faint)
                .child(label.clone())
                .into_any_element(),
            PopupMenuItem::Separator => div()
                .mx(px(6.0))
                .my(px(if grouping { 5.0 } else { 4.0 }))
                .h(px(1.0))
                .flex_shrink_0()
                .bg(theme::settings_hairline(cx))
                .into_any_element(),
            PopupMenuItem::ElementItem { render, .. } => {
                div().px(px(10.0)).py(px(4.0)).child(render(window, cx)).into_any_element()
            },
            PopupMenuItem::Item { label, checked, action, .. } => {
                let enabled = Self::enabled(item);
                let shortcut = action.as_ref().and_then(|action| {
                    self.return_focus
                        .as_ref()
                        .and_then(|focus| {
                            Kbd::binding_for_action_in(action.as_ref(), focus, window)
                        })
                        .or_else(|| Kbd::binding_for_action(action.as_ref(), None, window))
                });
                let marked = row.radio.unwrap_or(*checked);
                let ink = if !enabled {
                    faint.opacity(0.70)
                } else if grouping && !marked {
                    palette.muted_foreground
                } else {
                    palette.popover_foreground
                };
                h_flex()
                    .id(("tab-menu-row", ix))
                    .debug_selector(move || format!("tab-menu-row-{ix}"))
                    .role(if row.radio.is_some() { Role::MenuItemRadio } else { Role::MenuItem })
                    .when_some(row.radio, |row, checked| {
                        row.aria_toggled(if checked {
                            gpui::accesskit::Toggled::True
                        } else {
                            gpui::accesskit::Toggled::False
                        })
                    })
                    .aria_label(label.clone())
                    .min_h(px(30.0))
                    .px(px(10.0))
                    .py(px(4.0))
                    .gap(px(10.0))
                    .rounded(px(7.0))
                    .text_size(px(13.0 * scale))
                    .text_color(ink)
                    .when(self.keyboard_row == Some(ix), |row| row.bg(palette.list_hover))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| {
                                style.bg(palette.list_hover).text_color(palette.popover_foreground)
                            })
                            .active(|style| style.bg(palette.foreground.opacity(0.12)))
                            .on_click(cx.listener(move |this, event, window, cx| {
                                this.activate(ix, event, window, cx)
                            }))
                    })
                    .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                    .when_some(row.hint.filter(|_| !marked), |row, hint| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .font_family(palette.mono_font_family.clone())
                                .text_size(px(11.0 * scale))
                                .text_color(faint)
                                .child(hint),
                        )
                    })
                    .when(marked, |row| {
                        row.aria_toggled(gpui::accesskit::Toggled::True).child(
                            Icon::new(IconName::Check).with_size(px(14.0)).text_color(palette.link),
                        )
                    })
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            shortcut
                                .p_0()
                                .flex_none()
                                .flex_nowrap()
                                .border_0()
                                .bg(palette.transparent),
                        )
                    })
                    .into_any_element()
            },
            PopupMenuItem::Submenu { .. } => div().into_any_element(),
        }
    }
}

impl EventEmitter<DismissEvent> for TabMenuSurface {}

impl Focusable for TabMenuSurface {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TabMenuSurface {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl gpui::IntoElement {
        let palette = cx.theme();
        let grouping = self.group_trigger.is_some();
        let popover_color = palette.popover;
        let border = palette.is_dark().then_some(palette.border.opacity(0.7));
        let shadow = if border.is_some() {
            vec![BoxShadow {
                offset: point(px(0.0), px(3.0)),
                blur_radius: px(12.0),
                spread_radius: px(-3.0),
                color: hsla(0.0, 0.0, 0.0, 0.32),
                inset: false,
            }]
        } else {
            theme::float_shadow(cx)
        };
        let rows = self
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| self.row(ix, item, window, cx))
            .collect::<Vec<_>>();
        let limit = window.viewport_size();
        let max_width = (limit.width - px(16.0)).clamp(px(80.0), px(440.0));
        v_flex()
            .id("tab-menu-surface")
            .debug_selector(move || {
                if grouping { "sidebar-grouping-panel" } else { "tab-menu-panel" }.into()
            })
            .role(Role::Menu)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                cx.stop_propagation();
                this.select(false, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                cx.stop_propagation();
                this.select(true, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivateItem, window, cx| {
                if let Some(ix) = this.keyboard_row {
                    this.activate(ix, &ClickEvent::default(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DismissMenu, window, cx| {
                cx.stop_propagation();
                this.dismiss(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                if !this
                    .group_trigger
                    .as_ref()
                    .is_some_and(|trigger| trigger.get().contains(&event.position))
                {
                    this.dismiss(window, cx);
                }
            }))
            .min_w(self.min_width.min(max_width))
            .max_w(max_width)
            .occlude()
            .rounded(px(12.0))
            .bg(if grouping { popover_color } else { theme::raised_fill(cx) })
            .when_some(border, |menu, color| menu.border_1().border_color(color))
            .shadow(shadow)
            .child(
                v_flex()
                    .id("tab-menu-rows")
                    .p(px(6.0))
                    .gap(px(if grouping { 0.0 } else { 1.0 }))
                    .max_h((limit.height - px(20.0)).max(px(40.0)))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows),
            )
    }
}
