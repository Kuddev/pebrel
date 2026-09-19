//! Short fixed choices stay visible and share the existing preference authority.

use super::*;

impl SettingsPane {
    pub(super) fn segmented_setting(
        &self,
        key: &'static str,
        cx: &Context<Self>,
    ) -> Option<gpui::AnyElement> {
        // Long prose choices (window routing, language, etc.) retain their dropdown.
        if !matches!(
            key,
            "density"
                | "tabs_position"
                | "tab_reveal"
                | "new_tab_position"
                | "vcs_display"
                | "cell_width_mode"
                | "completion_style"
        ) {
            return None;
        }
        let (_, state, values) = self.selects.iter().find(|(candidate, _, _)| *candidate == key)?;
        let selected = state.read(cx).selected_index(cx).map(|index| index.row).unwrap_or(0);
        let labels =
            localized_select_labels(key, values, crate::gpui_shell::config::ui_language(cx));
        Some(
            h_flex()
                .w(px(SETTINGS_SELECT_WIDTH))
                .gap_1()
                .children(values.iter().copied().zip(labels).enumerate().map(
                    |(index, (value, label))| {
                        Button::new(SharedString::from(format!("settings-choice-{key}-{value}")))
                            .debug_selector(move || format!("settings-choice-{key}-{value}"))
                            .flex_1()
                            .min_w_0()
                            .small()
                            .outline()
                            .selected(index == selected)
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                match this.try_persist(&[(key, value.to_owned())], cx) {
                                    Ok(()) => this.sync_select(key, value, window, cx),
                                    Err(error) => crate::gpui_shell::toast::toast(
                                        window,
                                        cx,
                                        crate::display::ToastKind::Warning,
                                        error.to_string(),
                                    ),
                                }
                            }))
                    },
                ))
                .into_any_element(),
        )
    }
}
