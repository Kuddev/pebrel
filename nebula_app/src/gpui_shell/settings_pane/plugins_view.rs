//! 插件页渲染：安装入口、授权确认卡片与已安装插件列表。

use super::plugins::{Busy, Pending};
use super::*;
use crate::i18n::Message;
use crate::plugins::Grant;
use crate::plugins::install::{Broken, Installed};

fn grant_label(grant: Grant) -> Message {
    match grant {
        Grant::ThemeLibrary => Message::SettingsPluginsGrantThemeLibrary,
    }
}

impl SettingsPane {
    pub(super) fn section_plugins(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        if self.plugins.listing.is_none() && !self.plugins.loading && self.plugins.busy.is_none() {
            self.refresh_plugins(cx);
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let idle = !self.plugins.loading && self.plugins.busy.is_none();
        let empty = self.plugins.listing.as_ref().is_some_and(|listing| {
            listing
                .as_ref()
                .is_ok_and(|listing| listing.plugins.is_empty() && listing.broken.is_empty())
        });
        let header = h_flex()
            .items_start()
            .justify_between()
            .flex_wrap()
            .gap(px(14.0))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(160.0))
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_semibold()
                            .child(language.text(Message::SettingsPluginsTitle)),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(gpui::relative(1.5))
                            .text_color(muted)
                            .child(language.text(Message::SettingsPluginsDescription)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when(!empty, |row| row.child(self.plugin_install_button(false, cx)))
                    .child(
                        crate::gpui_shell::widgets::settings_button(
                            Button::new("plugins-refresh")
                                .debug_selector(|| "plugins-refresh".to_owned())
                                .icon(IconName::Redo)
                                .label(language.text(Message::SettingsPluginsRefresh))
                                .disabled(!idle)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_plugins(cx))),
                            true,
                            cx,
                        )
                        .h(px(30.0))
                        .text_size(px(12.0)),
                    ),
            );
        let mut page = v_flex().w_full().max_w(px(720.0)).gap(px(20.0)).child(header);
        if let Some(pending) = self.plugins.pending.clone() {
            page = page.child(self.plugin_grant_card(&pending, window, cx));
        }
        if let Some((None, result)) = &self.plugins.feedback {
            page = page.child(feedback_line(None, result, cx));
        }
        let border = crate::gpui_shell::theme::settings_hairline(cx);
        match &self.plugins.listing {
            None => {
                page = page.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::SettingsPluginsLoading)),
                );
            },
            Some(Err(error)) => {
                page =
                    page.child(div().text_sm().text_color(cx.theme().danger).child(
                        language.format(Message::SettingsPluginsFailed, &[("error", error)]),
                    ));
            },
            Some(Ok(_)) if empty => {
                page = page.child(
                    v_flex()
                        .id("plugins-empty")
                        .debug_selector(|| "plugins-empty".into())
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .text_center()
                        .px(px(24.0))
                        .py(px(44.0))
                        .gap(px(12.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_dashed()
                        .border_color(border)
                        .bg(cx.theme().foreground.opacity(0.015))
                        .child(
                            div()
                                .size(px(36.0))
                                .rounded(px(8.0))
                                .bg(cx.theme().secondary)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::default()
                                        .path(crate::gpui_shell::assets::nav::PUZZLE)
                                        .size(px(18.0))
                                        .text_color(muted),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_medium()
                                .child(language.text(Message::SettingsPluginsEmpty)),
                        )
                        .child(
                            div()
                                .max_w(px(360.0))
                                .text_size(px(12.0))
                                .line_height(gpui::relative(1.5))
                                .text_color(muted)
                                .child(language.text(Message::SettingsPluginsEmptyHint)),
                        )
                        .child(self.plugin_install_button(true, cx).mt(px(6.0))),
                );
            },
            Some(Ok(listing)) => {
                let total = listing.plugins.len() + listing.broken.len();
                let mut list = v_flex().w_full().rounded_lg().border_1().border_color(border);
                for (index, plugin) in listing.plugins.iter().enumerate() {
                    list = list.child(self.plugin_row(index, plugin, index + 1 < total, cx));
                }
                for (offset, broken) in listing.broken.iter().enumerate() {
                    let index = listing.plugins.len() + offset;
                    list = list.child(self.broken_plugin_row(index, broken, index + 1 < total, cx));
                }
                page = page.child(list);
            },
        }
        page.child(
            div()
                .text_sm()
                .text_color(muted)
                .child(language.text(Message::SettingsPluginsThemesKept)),
        )
    }

    fn plugin_install_button(&self, primary: bool, cx: &mut Context<Self>) -> Button {
        let language = crate::gpui_shell::config::ui_language(cx);
        let idle = !self.plugins.loading && self.plugins.busy.is_none();
        let button = Button::new("plugins-install")
            .debug_selector(|| "plugins-install".into())
            .label(language.text(if self.plugins.busy == Some(Busy::Installing) {
                Message::SettingsPluginsInstalling
            } else {
                Message::SettingsPluginsInstall
            }))
            .loading(matches!(self.plugins.busy, Some(Busy::Picking | Busy::Installing)))
            .disabled(!idle || self.plugins.pending.is_some())
            .on_click(cx.listener(|this, _, window, cx| this.pick_plugin_folder(window, cx)));
        let button = if primary {
            button.primary().px(px(12.0)).rounded(px(6.0))
        } else {
            crate::gpui_shell::widgets::settings_button(button, true, cx)
        };
        button.h(px(30.0)).text_size(px(12.0))
    }

    fn plugin_grant_card(
        &self,
        pending: &Pending,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let mut grants = v_flex().gap_1();
        for grant in &pending.request.requested {
            grants = grants
                .child(div().text_sm().child(format!("• {}", language.text(grant_label(*grant)))));
        }
        v_flex()
            .id("plugins-grant")
            .debug_selector(|| "plugins-grant".into())
            .w_full()
            .p_4()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().ring)
            .child(
                div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(language.format(
                    Message::SettingsPluginsGrantTitle,
                    &[("name", &pending.request.name)],
                )),
            )
            .child(grants)
            .child(div().text_xs().text_color(muted).child(pending.path.display().to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("plugins-grant-allow")
                            .debug_selector(|| "plugins-grant-allow".to_owned())
                            .primary()
                            .small()
                            .label(language.text(Message::SettingsPluginsGrantAllow))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.confirm_plugin_install(cx)),
                            ),
                    )
                    .child(
                        Button::new("plugins-grant-cancel")
                            .debug_selector(|| "plugins-grant-cancel".to_owned())
                            .ghost()
                            .small()
                            .label(language.text(Message::SettingsPluginsGrantCancel))
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_plugin_install(cx))),
                    ),
            )
    }

    fn plugin_row(
        &self,
        index: usize,
        plugin: &Installed,
        divider: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let idle = !self.plugins.loading && self.plugins.busy.is_none();
        let busy = self.plugins.busy == Some(Busy::Plugin(plugin.id.clone()));
        let id = plugin.id.clone();
        let mut text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(plugin.name.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(format!("{} · {}", plugin.version, plugin.id)),
            );
        if !plugin.grants.is_empty() {
            let granted: Vec<_> =
                plugin.grants.iter().map(|grant| language.text(grant_label(*grant))).collect();
            text =
                text.child(div().text_xs().text_color(muted).child(
                    language.format(
                        Message::SettingsPluginsGranted,
                        &[("grants", &granted.join(", "))],
                    ),
                ));
        }
        if let Some((Some(row), result)) = &self.plugins.feedback {
            if *row == plugin.id {
                text = text.child(feedback_line(Some(index), result, cx));
            }
        }
        let mut actions = h_flex().flex_shrink_0().gap_2();
        for (command_index, command) in plugin.theme_commands.iter().enumerate() {
            let (id, command_id) = (id.clone(), command.id.clone());
            actions = actions.child(
                Button::new(("plugin-generate", index * 64 + command_index))
                    .debug_selector(move || format!("plugin-generate-{index}-{command_index}"))
                    .outline()
                    .small()
                    .icon(IconName::Palette)
                    .label(command.title.clone())
                    .tooltip(language.text(Message::SettingsPluginsGenerateTooltip))
                    .loading(busy)
                    .disabled(!idle || !plugin.enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.generate_plugin_theme(id.clone(), command_id.clone(), cx)
                    })),
            );
        }
        let uninstall_id = id.clone();
        actions = actions.child(
            Button::new(("plugin-uninstall", index))
                .debug_selector(move || format!("plugin-uninstall-{index}"))
                .ghost()
                .small()
                .label(language.text(Message::SettingsPluginsUninstall))
                .disabled(!idle)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.uninstall_plugin(uninstall_id.clone(), cx)
                })),
        );
        let enabled = plugin.enabled;
        actions = actions.child(
            div()
                .id(("plugin-enabled", index))
                .debug_selector(move || format!("plugin-enabled-{index}"))
                .min_w(px(40.0))
                .min_h(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    crate::gpui_shell::widgets::NebulaSwitch::new(format!(
                        "plugin-enabled-{index}"
                    ))
                    .checked(enabled)
                    .disabled(!idle)
                    .on_click(cx.listener(move |this, checked, _, cx| {
                        this.set_plugin_enabled(id.clone(), *checked, cx)
                    })),
                ),
        );
        row_frame(index, divider, cx)
            .aria_label(format!(
                "{} — {}",
                plugin.name,
                language.text(if enabled {
                    Message::SettingsPluginsEnabled
                } else {
                    Message::SettingsPluginsDisabled
                })
            ))
            .child(text)
            .child(actions)
    }

    fn broken_plugin_row(
        &self,
        index: usize,
        broken: &Broken,
        divider: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let idle = !self.plugins.loading && self.plugins.busy.is_none();
        let id = broken.id.clone();
        row_frame(index, divider, cx)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(id.clone()))
                    .child(
                        div().text_xs().text_color(cx.theme().danger).child(
                            language.format(
                                Message::SettingsPluginsBroken,
                                &[("error", &broken.error)],
                            ),
                        ),
                    ),
            )
            .child(
                Button::new(("plugin-uninstall", index))
                    .debug_selector(move || format!("plugin-uninstall-{index}"))
                    .ghost()
                    .small()
                    .label(language.text(Message::SettingsPluginsUninstall))
                    .disabled(!idle)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.uninstall_plugin(id.clone(), cx)),
                    ),
            )
    }
}

fn row_frame(index: usize, divider: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(("plugin-row", index))
        .debug_selector(move || format!("plugin-row-{index}"))
        .w_full()
        .min_h(px(64.0))
        .px_4()
        .py_3()
        .gap_3()
        .border_b_1()
        .border_color(if divider {
            crate::gpui_shell::theme::settings_hairline(cx)
        } else {
            gpui::transparent_black()
        })
}

/// `row` is `None` for page-level results and the row index otherwise.
fn feedback_line(
    row: Option<usize>,
    result: &Result<String, String>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let (text, color) = match result {
        Ok(message) => (message.clone(), cx.theme().success),
        Err(message) => (message.clone(), cx.theme().danger),
    };
    let selector = match row {
        Some(index) => format!("plugin-feedback-{index}"),
        None => "plugins-feedback".to_owned(),
    };
    div()
        .id(("plugin-feedback", row.map_or(usize::MAX, |index| index)))
        .debug_selector(move || selector.clone())
        .text_xs()
        .text_color(color)
        .child(text)
}
