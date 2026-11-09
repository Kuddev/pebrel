//! Headerless panes in sidebar-tab mode: a floating grip and one window title.

use super::*;
use crate::gpui_shell::workspace::TerminalPane;
use gpui::{AvailableSpace, Focusable as _, point, relative};
use gpui_component::menu::PopupMenuItem;

fn grip_mark(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            for row in 0..2 {
                for column in 0..3 {
                    window.paint_quad(
                        gpui::fill(
                            Bounds::new(
                                bounds.origin
                                    + point(
                                        px(2.0 + column as f32 * 5.0),
                                        px(4.0 + row as f32 * 5.0),
                                    ),
                                size(px(2.0), px(2.0)),
                            ),
                            color,
                        )
                        .corner_radii(px(1.0)),
                    );
                }
            }
        },
    )
    .size(px(16.0))
}

/// Center on the window, bounded symmetrically by the actual toolbar slot.
/// Native window controls and optional toolbar buttons need no duplicated insets.
fn centered_title(title: gpui::AnyElement) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let center = window.viewport_size().width / 2.0;
            let half = (center - bounds.left()).min(bounds.right() - center) - px(8.0);
            if half <= px(0.0) {
                return None;
            }
            let width = half * 2.0;
            let mut container = h_flex()
                .w(width)
                .h(bounds.size.height)
                .items_center()
                .justify_center()
                .min_w_0()
                .child(title)
                .into_any_element();
            container.prepaint_as_root(
                point(center - half, bounds.top()),
                size(AvailableSpace::Definite(width), AvailableSpace::Definite(bounds.size.height)),
                window,
                cx,
            );
            Some(container)
        },
        |_, title, window, cx| {
            if let Some(mut title) = title {
                title.paint(window, cx);
            }
        },
    )
    .flex_1()
    .min_w_0()
    .h_full()
}

impl NebulaWorkspace {
    pub(in crate::gpui_shell::workspace) fn render_active_pane_title(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let family = theme.font_family.clone();
        let ink = theme.foreground;
        let font_size = cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .map_or(15.0, |settings| settings.ui_font_size_px);
        let pane = self.tabs.get(self.active).and_then(WorkspaceTab::primary_terminal_pane);
        if !self.settings_open
            && let Some(pane) = pane
            && let Some(editor) =
                self.render_pane_name_editor(pane.id, font_size, 28.0, family.clone(), cx)
        {
            return centered_title(editor).into_any_element();
        }
        let title = if self.settings_open {
            language.text(Message::CommonSettings).into()
        } else if let Some(pane) = pane {
            pane.custom_name
                .as_deref()
                .map(SharedString::from)
                .unwrap_or_else(|| self.pane_title(&pane.view, cx, cx.theme().is_dark()).text)
        } else if self.tabs.is_empty() {
            SharedString::default()
        } else {
            self.tab_presentation(self.active, cx, cx.theme().is_dark()).title
        };
        let full_title = title.clone();
        centered_title(
            div()
                .id("active-pane-title")
                .debug_selector(|| "active-pane-title".into())
                .max_w_full()
                .min_w_0()
                .truncate()
                .font_family(family)
                .text_size(px(font_size))
                .text_color(ink)
                .font_weight(FontWeight::NORMAL)
                .tooltip(move |window, cx| {
                    super::super::tab_presentation::tooltip(full_title.clone(), window, cx)
                })
                .child(title)
                .into_any_element(),
        )
        .into_any_element()
    }

    pub(in crate::gpui_shell::workspace) fn render_toolbar_broadcast(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Button> {
        if self.settings_open {
            return None;
        }
        let WorkspaceTab::Terminal { panes, broadcast, .. } = self.tabs.get(self.active)? else {
            return None;
        };
        if panes.len() < 2 {
            return None;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let ink = if *broadcast { cx.theme().primary } else { cx.theme().muted_foreground };
        let label = language.text(if *broadcast {
            Message::WorkspacePaneStopBroadcastInput
        } else {
            Message::WorkspacePaneBroadcastInputTooltip
        });
        Some(
            Button::new("toolbar-broadcast")
                .debug_selector(|| "toolbar-broadcast".into())
                .ghost()
                .size(px(32.0))
                .cursor_pointer()
                .selected(*broadcast)
                .aria_label(label)
                .tooltip(label)
                .child(broadcast_mark(18.0, ink))
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_tab_broadcast(this.active, window, cx);
                })),
        )
    }

    pub(super) fn render_pane_grip(
        &self,
        pane: &TerminalPane,
        group: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = pane.id;
        let theme = cx.theme();
        let (ink, bg, border, hover, accent) =
            (theme.foreground, theme.popover, theme.border, theme.secondary, theme.primary);
        let dragging = self.pane_drag.as_ref().is_some_and(|drag| drag.pane == id && drag.active);
        let language = crate::gpui_shell::config::ui_language(cx);
        let name = pane
            .custom_name
            .as_deref()
            .map(SharedString::from)
            .unwrap_or_else(|| self.pane_title(&pane.view, cx, cx.theme().is_dark()).text);
        let tooltip: SharedString =
            format!("{name}\n{}", language.text(Message::WorkspacePaneDragHint)).into();
        // Hidden grips do not intercept terminal selection. The open menu is
        // mounted on the workspace root and outlives this hover state.
        div()
            .id(("pane-grip-overlay", id as usize))
            .absolute()
            .top_0()
            .left(relative(0.5))
            .ml(px(-20.0))
            .w(px(40.0))
            .h(px(28.0))
            .focusable()
            .role(gpui::Role::Button)
            .aria_label(name)
            .aria_description(language.text(Message::WorkspacePaneDragHint))
            .tab_index(0)
            .when(
                !dragging && !self.tab_menu.as_ref().is_some_and(|menu| menu.pane == Some(id)),
                |grip| grip.invisible().group_hover(group, |style| style.visible()),
            )
            .focus_visible(|style| style.visible().rounded(px(6.0)).border_1().border_color(accent))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        cx.stop_propagation();
                        let bounds = this.pane_bounds.borrow().get(&id).copied();
                        if let Some(bounds) = bounds {
                            this.open_pane_menu(
                                id,
                                point(bounds.center().x, bounds.top() + px(28.0)),
                                window,
                                cx,
                            );
                        }
                    },
                    "f2" => {
                        cx.stop_propagation();
                        this.rename_pane_from_chrome(id, window, cx);
                    },
                    _ => {},
                }
            }))
            .child(
                h_flex()
                    .id(("pane-header-grip", id as usize))
                    .debug_selector(move || format!("pane-header-grip-{id}"))
                    .size_full()
                    .items_center()
                    .justify_center()
                    .cursor_grab()
                    // Stop the press below rather than occluding the parent
                    // hover region: occlusion would hide the grip under itself.
                    .tooltip(move |window, cx| {
                        super::super::tab_presentation::tooltip(tooltip.clone(), window, cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if let Some(tab) = this.tab_of_pane(id) {
                                this.focus_pane(tab, id, window, cx);
                                this.begin_pane_drag(tab, id, event.position, cx);
                            }
                        }),
                    )
                    .on_double_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.rename_pane_from_chrome(id, window, cx);
                    }))
                    .child(
                        h_flex()
                            .id(("pane-grip-face", id as usize))
                            .w(px(34.0))
                            .h(px(18.0))
                            .items_center()
                            .justify_center()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(border)
                            .bg(bg)
                            .hover(|style| style.bg(hover))
                            .active(|style| style.bg(hover).border_color(accent))
                            .child(grip_mark(ink)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_pane_menu(id, event.position, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn rename_pane_from_chrome(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_of_pane(id) else {
            return;
        };
        let WorkspaceTab::Terminal { panes, .. } = &self.tabs[tab] else {
            return;
        };
        let Some(pane) = panes.iter().find(|pane| pane.id == id) else {
            return;
        };
        let title = self.pane_title(&pane.view, cx, cx.theme().is_dark()).text;
        self.focus_pane(tab, id, window, cx);
        self.begin_pane_rename(id, title, window, cx);
    }

    fn open_pane_menu(
        &mut self,
        id: u64,
        position: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_of_pane(id) else {
            return;
        };
        self.pane_drag = None;
        self.focus_pane(tab, id, window, cx);
        let WorkspaceTab::Terminal { zoomed, .. } = &self.tabs[tab] else {
            return;
        };
        let zoomed = *zoomed;
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        let focus = self.tabs[tab].focused_view().map(|view| view.read(cx).focus_handle(cx));
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            Self::pane_actions_menu(
                menu.when_some(focus, |menu, focus| menu.action_context(focus)),
                owner,
                id,
                zoomed,
                language,
            )
        });
        self.show_context_menu(menu, tab, Some(id), position, window, cx);
    }

    fn pane_actions_menu(
        menu: PopupMenu,
        owner: gpui::WeakEntity<Self>,
        id: u64,
        zoomed: bool,
        language: crate::display::UiLanguage,
    ) -> PopupMenu {
        let rename_owner = owner;
        let zoom_owner = rename_owner.clone();
        let close_owner = rename_owner.clone();
        menu.item(PopupMenuItem::new(language.text(Message::CommonRename)).on_click(
            move |_, window, cx| {
                let _ = rename_owner
                    .update(cx, |this, cx| this.rename_pane_from_chrome(id, window, cx));
            },
        ))
        .item(
            PopupMenuItem::new(language.text(if zoomed {
                Message::WorkspacePaneRestoreLayout
            } else {
                Message::WorkspacePaneZoom
            }))
            .icon(if zoomed { IconName::Minimize } else { IconName::Maximize })
            .on_click(move |_, window, cx| {
                let _ = zoom_owner.update(cx, |this, cx| {
                    if let Some(tab) = this.tab_of_pane(id) {
                        this.focus_pane(tab, id, window, cx);
                        this.toggle_zoom(cx);
                    }
                });
            }),
        )
        .separator()
        .item(
            PopupMenuItem::new(language.text(Message::WorkspacePaneClose))
                .icon(IconName::Close)
                .on_click(move |_, window, cx| {
                    let _ = close_owner.update(cx, |this, cx| {
                        if let Some(tab) = this.tab_of_pane(id) {
                            this.request_close_pane(tab, id, window, cx);
                        }
                    });
                }),
        )
    }
}
