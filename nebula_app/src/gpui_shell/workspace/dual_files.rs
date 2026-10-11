//! An opt-in source drawer and an explicitly paired SSH browser. Focus does not
//! choose transfer authority; pane identities and the remote generation do.
use super::local_files_drawer as local;
use super::*;
use crate::gpui_shell::widgets::toolbar_button;
use crate::i18n::Message;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PanePair {
    local: u64,
    remote: u64,
    destination: String,
}

pub(super) struct DualFilesState {
    pub(super) enabled: bool,
    pair: Option<PanePair>,
    choosing: bool,
    drawers: HashMap<u64, Entity<local::LocalFilesDrawer>>,
    width: f32,
    resize: Option<sidebar_resize::ResizeDrag>,
}

impl Default for DualFilesState {
    fn default() -> Self {
        Self {
            enabled: false,
            pair: None,
            choosing: false,
            drawers: HashMap::new(),
            width: 280.0,
            resize: None,
        }
    }
}

fn dual_width(preferred: f32, available: f32) -> f32 {
    // Both drawers share at most 60% of the area after the tab sidebar. The
    // preferred sizes survive a temporary narrow window or larger tab sidebar.
    let maximum = (available.max(0.0) * 0.30).min(560.0);
    preferred.clamp(160.0_f32.min(maximum), maximum)
}

impl NebulaWorkspace {
    fn dual_files_visible(&self, cx: &App) -> bool {
        self.dual_files.enabled
            && !self.settings_open
            && !self.reader_focus_active(cx)
            && matches!(self.tabs.get(self.active), Some(WorkspaceTab::Terminal { .. }))
    }

    fn dual_files_available_width(&self, window: &Window) -> f32 {
        let sidebar = if self.sidebar_collapsed
            || self.tabs_position == nebula_settings::TabsPositionName::Top
        {
            0.0
        } else {
            self.sidebar_width
        };
        (f32::from(window.viewport_size().width) - sidebar).max(0.0)
    }

    pub(super) fn details_sidebar_maximum(&self, window: &Window, cx: &App) -> f32 {
        if self.dual_files_visible(cx) {
            dual_width(560.0, self.dual_files_available_width(window))
        } else {
            details_panel::panel_width(560.0, f32::from(window.viewport_size().width))
        }
    }

    pub(super) fn details_sidebar_width(&self, window: &Window, cx: &App) -> f32 {
        if self.dual_files_visible(cx) {
            dual_width(
                self.details_panel.preferred_width(),
                self.dual_files_available_width(window),
            )
        } else {
            details_panel::panel_width(
                self.details_panel.preferred_width(),
                f32::from(window.viewport_size().width),
            )
        }
    }

    fn available_file_pairs(&self, cx: &App) -> Vec<(PanePair, String)> {
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(self.active) else {
            return Vec::new();
        };
        let language = crate::gpui_shell::config::ui_language(cx);
        let mut pairs = Vec::new();
        for source in panes.iter().filter(|pane| pane.view.read(cx).ssh_destination.is_none()) {
            let local = format!(
                "{} #{}",
                source.custom_name.clone().unwrap_or_else(|| source.view.read(cx).tab_label()),
                source.id
            );
            for target in panes {
                let Some(destination) = target.view.read(cx).ready_ssh_destination() else {
                    continue;
                };
                let remote = format!(
                    "{} #{}",
                    target.custom_name.as_deref().unwrap_or(destination),
                    target.id
                );
                pairs.push((
                    PanePair {
                        local: source.id,
                        remote: target.id,
                        destination: destination.to_owned(),
                    },
                    language
                        .format(Message::DualFilesPair, &[("local", &local), ("remote", &remote)]),
                ));
            }
        }
        pairs
    }

    pub(super) fn remote_browser_owner(&self, cx: &App) -> Option<(u64, String)> {
        if !self.dual_files.enabled {
            return self.focused_remote_pane(cx);
        }
        if !self.dual_files_visible(cx) {
            return None;
        }
        let pair = self.dual_files.pair.as_ref()?;
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(self.active) else {
            return None;
        };
        let local = panes.iter().find(|pane| pane.id == pair.local)?;
        if local.view.read(cx).ssh_destination.is_some() {
            return None;
        }
        let remote = panes.iter().find(|pane| pane.id == pair.remote)?.view.read(cx);
        (remote.ready_ssh_destination()? == pair.destination)
            .then(|| (pair.remote, pair.destination.clone()))
    }

    fn select_file_pair(&mut self, pair: PanePair, window: &mut Window, cx: &mut Context<Self>) {
        if !self.available_file_pairs(cx).iter().any(|(candidate, _)| candidate == &pair) {
            return;
        }
        if self.dual_files.pair.as_ref() != Some(&pair) {
            self.remote_browser.detach();
        }
        if !self.dual_files.drawers.contains_key(&pair.local) {
            let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(self.active) else {
                return;
            };
            let source = panes.iter().find(|pane| pane.id == pair.local).unwrap().view.downgrade();
            let drawer = cx.new(|cx| local::LocalFilesDrawer::new(source, window, cx));
            self.dual_files.drawers.insert(pair.local, drawer);
        }
        self.dual_files.pair = Some(pair);
        self.dual_files.choosing = false;
        self.details_panel.section = None;
        self.select_side_panel_view(crate::display::side_panel::PanelView::Files, cx);
        cx.notify();
    }

    fn toggle_dual_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dual_files.enabled = !self.dual_files.enabled;
        self.dual_files.resize = None;
        if self.dual_files.enabled {
            let pairs = self.available_file_pairs(cx);
            if pairs.len() == 1 {
                self.select_file_pair(pairs[0].0.clone(), window, cx);
            } else {
                self.dual_files.choosing = true;
                self.details_panel.section = None;
                self.select_side_panel_view(crate::display::side_panel::PanelView::Files, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn render_dual_files_button(
        &self,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        toolbar_button("dual-file-trees", IconName::FolderOpen)
            .debug_selector(|| "dual-file-trees".into())
            .disabled(
                disabled
                    || !matches!(self.tabs.get(self.active), Some(WorkspaceTab::Terminal { .. })),
            )
            .selected(self.dual_files.enabled)
            .tooltip(crate::gpui_shell::config::ui_language(cx).text(Message::DualFilesToggle))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_dual_files(window, cx)))
    }

    pub(super) fn sync_dual_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live: Vec<u64> = self
            .tabs
            .iter()
            .flat_map(|tab| match tab {
                WorkspaceTab::Terminal { panes, .. } => panes.iter().map(|pane| pane.id).collect(),
                _ => Vec::new(),
            })
            .collect();
        self.dual_files.drawers.retain(|pane, _| live.contains(pane));
        let source =
            self.remote_browser_owner(cx).and(self.dual_files.pair.as_ref().map(|pair| pair.local));
        for (pane, drawer) in &self.dual_files.drawers {
            drawer.update(cx, |drawer, cx| drawer.set_visible(source == Some(*pane), window, cx));
        }
        if !self.dual_files_visible(cx) {
            self.dual_files.resize = None
        }
    }

    pub(super) fn render_dual_files_unavailable(&self, cx: &App) -> gpui::AnyElement {
        let unavailable = self.dual_files.pair.is_some();
        div()
            .debug_selector(move || {
                if unavailable { "dual-files-unavailable" } else { "dual-files-no-pair" }.into()
            })
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(crate::gpui_shell::config::ui_language(cx).text(
                if self.dual_files.pair.is_some() {
                    Message::DualFilesUnavailable
                } else {
                    Message::DualFilesNoPair
                },
            ))
            .into_any_element()
    }

    pub(super) fn render_local_files_slot(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !self.dual_files_visible(cx) {
            return div().into_any_element();
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let width = dual_width(self.dual_files.width, self.dual_files_available_width(window));
        let pairs = self.available_file_pairs(cx);
        let label = pairs
            .iter()
            .find(|(pair, _)| Some(pair) == self.dual_files.pair.as_ref())
            .map(|(_, label)| label.clone());
        let source =
            self.remote_browser_owner(cx).and(self.dual_files.pair.as_ref().map(|pair| pair.local));
        let mut band = v_flex()
            .id("local-files-slot")
            .debug_selector(|| "local-files-slot".into())
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .min_w_0()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(48.0))
                    .px_2()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(language.text(Message::DualFilesLocal)),
                    )
                    .child(
                        Button::new("choose-file-pair")
                            .debug_selector(|| "choose-file-pair".into())
                            .icon(IconName::Settings)
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::DualFilesChoose))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dual_files.choosing = !this.dual_files.choosing;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close-local-files")
                            .debug_selector(|| "close-local-files".into())
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .tooltip(language.text(Message::CommonClose))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_dual_files(window, cx)
                            })),
                    ),
            )
            .when_some(label.clone(), |band, label| {
                band.child(div().px_2().text_xs().truncate().child(label))
            });
        if self.dual_files.choosing {
            let mut choices =
                v_flex().id("file-pair-choices").max_h(px(180.0)).overflow_y_scroll().p_1();
            for (pair, label) in pairs {
                let id = format!("file-pair-{}-{}", pair.local, pair.remote);
                choices = choices.child(
                    Button::new(id.clone())
                        .debug_selector(move || id.clone())
                        .ghost()
                        .small()
                        .w_full()
                        .min_w_0()
                        .tooltip(label.clone())
                        .child(div().w_full().min_w_0().truncate().text_left().child(label))
                        .selected(self.dual_files.pair.as_ref() == Some(&pair))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_file_pair(pair.clone(), window, cx)
                        })),
                );
            }
            band = band.child(choices);
        }
        band = if let Some(drawer) =
            source.and_then(|pane| self.dual_files.drawers.get(&pane)).cloned()
        {
            band.child(div().flex_1().min_h_0().min_w_0().child(drawer))
        } else {
            band.child(self.render_dual_files_unavailable(cx))
        };
        band.when(crate::gpui_shell::config::panel_resize(cx), |band| {
            band.child(
                div()
                    .id("local-files-resize")
                    .debug_selector(|| "local-files-resize".into())
                    .absolute()
                    .right(px(-4.0))
                    .top_0()
                    .bottom_0()
                    .w(px(8.0))
                    .occlude()
                    .cursor_col_resize()
                    .hover(|line| line.bg(cx.theme().ring.opacity(0.3)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.dual_files.resize = Some(sidebar_resize::ResizeDrag::new(
                                f32::from(event.position.x),
                                width,
                            ));
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
        })
        .into_any_element()
    }

    pub(super) fn update_local_files_resize(
        &mut self,
        event: &gpui::MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let maximum = dual_width(560.0, self.dual_files_available_width(window));
        let Some(drag) = &mut self.dual_files.resize else { return };
        if event.pressed_button != Some(MouseButton::Left) || !window.is_window_active() {
            self.dual_files.width = drag.start_width;
            self.dual_files.resize = None;
        } else {
            self.dual_files.width = drag.width(
                drag.start_width + f32::from(event.position.x) - drag.start_x,
                160.0_f32.min(maximum),
                maximum,
            );
        }
        cx.notify();
    }

    pub(super) fn finish_local_files_resize(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.dual_files.resize.take() else { return false };
        self.dual_files.width = if drag.close { drag.start_width } else { drag.open_width };
        if drag.close {
            self.dual_files.enabled = false
        }
        cx.notify();
        true
    }

    pub(super) fn cancel_local_files_resize(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.dual_files.resize.take() else { return false };
        self.dual_files.width = drag.start_width;
        cx.notify();
        true
    }

    pub(super) fn render_local_files_resize_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.dual_files.resize.map(|_| {
            div()
                .absolute()
                .inset_0()
                .occlude()
                .cursor_col_resize()
                .on_mouse_move(cx.listener(|this, event, window, cx| {
                    this.update_local_files_resize(event, window, cx);
                    cx.stop_propagation();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.finish_local_files_resize(cx);
                        cx.stop_propagation();
                    }),
                )
                .into_any_element()
        })
    }
}
