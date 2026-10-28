//! One terminal surface layout for split leaves, single panes and zoomed panes.

use super::*;
use crate::gpui_shell::workspace::{TerminalPane, terminal_activity};

impl NebulaWorkspace {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::gpui_shell::workspace) fn render_terminal_pane(
        &self,
        tab_ix: usize,
        pane: &TerminalPane,
        ordinal: usize,
        multiple: bool,
        focused: bool,
        zoomed: bool,
        broadcast: bool,
        corners: HeaderCorners,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let sidebar = self.tabs_position == nebula_settings::TabsPositionName::Sidebar;
        let id = pane.id;
        let group: SharedString = format!("terminal-pane-{id}").into();
        let store = self.pane_bounds.clone();
        let dim = !focused
            && cx
                .try_global::<crate::gpui_shell::config::Settings>()
                .is_none_or(|settings| settings.dim_inactive_panes);
        let veil = if sidebar {
            // Fade content toward its own background, including in light themes.
            crate::gpui_shell::theme::card_content_bg(cx).opacity(0.2)
        } else {
            gpui::black().opacity(crate::display::NEBULA_UNFOCUSED_SPLIT_DIM)
        };
        let mut body = div()
            .id(("pane-body", id as usize))
            .debug_selector(move || format!("pane-body-{id}"))
            .flex_1()
            .min_w_0()
            .min_h_0()
            .relative()
            .child(terminal_activity::terminal_surface(pane))
            .when(dim, |body| body.child(div().absolute().inset_0().bg(veil)));
        if sidebar && multiple {
            // Overlays never reserve a terminal row, including while hovered or zoomed.
            // The active pane stays identifiable even when dimming is disabled.
            body = body
                .when(focused, |body| {
                    body.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(1.0))
                            .bg(cx.theme().primary),
                    )
                })
                .when(broadcast, |body| {
                    body.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(1.5))
                            .bg(cx.theme().primary),
                    )
                })
                // Place the hover region above the terminal's failure/reader
                // overlays too. Its normal hitbox lets body input pass through.
                .child(
                    div()
                        .id(("pane-hover-region", id as usize))
                        .absolute()
                        .inset_0()
                        .group(group.clone())
                        .child(self.render_pane_grip(pane, group, cx)),
                );
        }
        v_flex()
            .id(("terminal-pane", id as usize))
            .size_full()
            .relative()
            .min_w_0()
            .min_h_0()
            // Navigation, docking and pane dragging all measure the same full leaf.
            .child(
                canvas(
                    move |bounds, _, _| {
                        store.borrow_mut().insert(id, bounds);
                    },
                    |_, _, _, _| (),
                )
                .absolute()
                .inset_0(),
            )
            .when(multiple && !sidebar, |pane_box| {
                pane_box.child(self.render_pane_header(
                    tab_ix,
                    id,
                    ordinal,
                    &pane.view,
                    pane.custom_name.as_deref(),
                    focused,
                    zoomed,
                    broadcast,
                    corners,
                    cx,
                ))
            })
            .child(body)
            .into_any_element()
    }
}
