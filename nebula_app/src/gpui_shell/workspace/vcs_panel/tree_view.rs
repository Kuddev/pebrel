//! View controls and directory rows for the snapshot-derived changes hierarchy.
use super::list_model::ChangeLayout;
use super::*;

impl NebulaWorkspace {
    pub(super) fn render_vcs_layout_toggle(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if !self.vcs_list.can_choose_layout() {
            return div().into_any_element();
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        h_flex()
            .tab_group()
            .w_full()
            .gap_1()
            .children(
                [
                    ("git-layout-list", ChangeLayout::List, Message::VcsListView),
                    ("git-layout-tree", ChangeLayout::Tree, Message::VcsTreeView),
                ]
                .into_iter()
                .map(|(id, layout, message)| {
                    Button::new(id)
                        .debug_selector(move || id.to_owned())
                        .label(language.text(message))
                        .selected(self.vcs_list.layout() == layout)
                        .ghost()
                        .xsmall()
                        .flex_1()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.vcs_list.set_layout(layout);
                            cx.notify();
                        }))
                }),
            )
            .into_any_element()
    }

    pub(super) fn render_vcs_directory(
        &self,
        section_id: &'static str,
        relative_path: String,
        depth: usize,
        count: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let name = relative_path.rsplit('/').next().unwrap_or(&relative_path).to_owned();
        let selector = format!("git-directory-{section_id}-{relative_path}");
        let keyboard_path = relative_path.clone();
        let click_path = relative_path.clone();
        let button = Button::new(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .role(None::<gpui::Role>)
            .ghost()
            .w_full()
            .h(px(30.0))
            .px_2()
            .pl(px(8.0 + 14.0 * depth.min(12) as f32))
            .tooltip(language.text(if expanded {
                Message::VcsCollapseFolder
            } else {
                Message::VcsExpandFolder
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall(),
                    )
                    .child(
                        Icon::new(IconName::Folder)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(count.to_string()),
                    ),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if (event.keystroke.key == "left" && expanded)
                    || (event.keystroke.key == "right" && !expanded)
                {
                    cx.stop_propagation();
                    this.vcs_list.toggle_directory(section_id, &keyboard_path);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.vcs_list.toggle_directory(section_id, &click_path);
                cx.notify();
            }))
            .into_any_element();
        // The pinned Button exposes its role but not expanded/name metadata.
        // A real GPUI accessibility node owns those semantics; Button retains
        // its native focus, keyboard activation and pointer interaction.
        div()
            .id(SharedString::from(format!("git-directory-semantics-{section_id}-{relative_path}")))
            .role(gpui::Role::TreeItem)
            .aria_label(SharedString::from(relative_path))
            .aria_expanded(expanded)
            .aria_level(depth + 1)
            .w_full()
            .child(button)
            .into_any_element()
    }
}
