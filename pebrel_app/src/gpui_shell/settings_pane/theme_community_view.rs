//! Community catalog cards in the theme picker's list column.

use super::appearance_picker::{AppearanceColors, AppearanceSelection};
use super::theme_community::{CatalogLoad, InstallState, Preview};
use super::*;
use crate::i18n::Message;
use crate::theme_library::community::Installed;
use gpui::{ObjectFit, StyledImage as _};

fn download_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else {
        format!("{} KiB", bytes.div_ceil(1024))
    }
}

impl SettingsPane {
    pub(super) fn community_list(
        &self,
        width: f32,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let colors = AppearanceColors::current(cx);
        let picker = self.appearance_picker.as_ref().unwrap();
        let note = |text: String| {
            div()
                .text_size(px(10.5))
                .line_height(gpui::relative(1.6))
                .text_color(colors.secondary)
                .child(text)
        };
        let column = v_flex()
            .w(px(width))
            .gap(px(10.0))
            .debug_selector(|| "community-theme-list".to_owned())
            .child(note(language.text(Message::ThemeCommunityNotice).to_owned()));
        let Some(community) = picker.community.as_ref() else {
            return column;
        };
        let retry = || {
            Button::new("community-retry")
                .debug_selector(|| "community-retry".to_owned())
                .label(language.text(Message::ThemeCommunityRetry))
                .ghost()
                .h(px(27.0))
                .px(px(10.0))
                .text_size(px(11.0))
                .rounded_full()
                .text_color(colors.primary)
                .on_click(cx.listener(|this, _, _, cx| this.load_community(true, cx)))
        };
        let listing = match &community.catalog {
            CatalogLoad::Loading => {
                return column.child(note(language.text(Message::ThemeCommunityLoading).into()));
            },
            CatalogLoad::Failed(error) => {
                return column.child(
                    v_flex()
                        .items_start()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(cx.theme().danger)
                                .child(error.clone()),
                        )
                        .child(retry()),
                );
            },
            CatalogLoad::Ready(listing) => listing,
        };
        let column = column
            .when(listing.skipped > 0, |column| {
                column.child(note(language.format(
                    Message::ThemeCommunitySkipped,
                    &[("count", &listing.skipped.to_string())],
                )))
            })
            .when_some(community.notice.clone(), |column, notice| {
                column.child(
                    div()
                        .debug_selector(|| "community-notice".to_owned())
                        .text_size(px(11.0))
                        .text_color(colors.primary)
                        .child(notice),
                )
            });
        if listing.entries.is_empty() {
            return column.child(note(language.text(Message::ThemeCommunityEmpty).into()));
        }
        let columns = if compact { 1 } else { 2 };
        let gap = 10.0;
        let card_width = (width - (columns - 1) as f32 * gap) / columns as f32;
        let busy = picker.apply_busy || picker.custom_loading;
        let cards = listing.entries.iter().enumerate().map(|(index, entry)| {
            let installed = self.community_installed(entry);
            let install = community.installs.get(&entry.id);
            let previewing = matches!(
                (&installed, picker.draft),
                (Installed::Current(id), AppearanceSelection::Custom(selected))
                    if picker.custom_themes.get(selected).and_then(|document| document.id())
                        == Some(id.as_str())
            );
            let preview = match community.previews.get(&entry.id) {
                Some(Preview::Ready(image)) => {
                    img(image.clone()).size_full().object_fit(ObjectFit::Cover).into_any_element()
                },
                Some(Preview::Loading) => div().into_any_element(),
                _ => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.5))
                    .text_color(colors.muted)
                    .child(language.text(Message::ThemeCommunityNoPreview))
                    .into_any_element(),
            };
            let id = entry.id.clone();
            let action = Button::new(("community-action", index))
                .debug_selector(move || format!("community-action-{index}"))
                .h(px(27.0))
                .px(px(11.0))
                .text_size(px(11.0))
                .rounded_full();
            let action = match (install, &installed) {
                (Some(InstallState::Running), _) => action
                    .label(language.text(Message::ThemeCommunityInstalling))
                    .loading(true)
                    .disabled(true)
                    .ghost()
                    .text_color(colors.secondary),
                (_, Installed::Current(_)) if previewing => action
                    .icon(IconName::Check)
                    .label(language.text(Message::ThemeCommunityInstalled))
                    .disabled(true)
                    .ghost()
                    .text_color(colors.secondary),
                (_, Installed::Current(theme_id)) => {
                    let theme_id = theme_id.clone();
                    action
                        .label(language.text(Message::ThemeCommunitySelect))
                        .ghost()
                        .disabled(busy)
                        .text_color(colors.primary)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.preview_community_theme(&theme_id, window, cx)
                        }))
                },
                (_, other) => action
                    .label(if *other == Installed::Other {
                        language.format(
                            Message::ThemeCommunityInstallVersion,
                            &[("version", &entry.version)],
                        )
                    } else {
                        language.text(Message::ThemeCommunityInstall).to_owned()
                    })
                    .disabled(busy)
                    .bg(colors.primary)
                    .border_color(colors.primary)
                    .text_color(colors.on_primary)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_community_theme(id.clone(), window, cx)
                    })),
            };
            let failure = match install {
                Some(InstallState::Failed(error)) => Some(error.clone()),
                _ => None,
            };
            v_flex()
                .id(("community-theme", index))
                .debug_selector(move || format!("community-theme-{index}"))
                .w(px(card_width))
                .flex_shrink_0()
                .rounded(px(8.0))
                .border_1()
                .border_color(if previewing { colors.primary } else { colors.line })
                .overflow_hidden()
                .child(
                    div()
                        .w_full()
                        .h(px((card_width * 0.5).round()))
                        .bg(colors.subtle)
                        .overflow_hidden()
                        .child(preview),
                )
                .child(
                    v_flex()
                        .p(px(10.0))
                        .gap(px(4.0))
                        .child(
                            h_flex()
                                .gap(px(6.0))
                                .justify_between()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.0))
                                        .font_semibold()
                                        .child(entry.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(10.0))
                                        .text_color(colors.secondary)
                                        .child(entry.version.clone()),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(10.5))
                                .text_color(colors.secondary)
                                .child(format!(
                                    "{} · {}",
                                    language.format(
                                        Message::ThemeCommunityAuthor,
                                        &[("author", &entry.author.name)],
                                    ),
                                    language.format(
                                        Message::ThemeCommunityDownloadSize,
                                        &[("size", &download_size(entry.archive.bytes))],
                                    ),
                                )),
                        )
                        .when_some(entry.description.clone(), |card, description| {
                            card.child(
                                div()
                                    .text_size(px(10.5))
                                    .line_height(gpui::relative(1.5))
                                    .text_color(colors.ink)
                                    .child(description),
                            )
                        })
                        .when_some(failure, |card, error| {
                            card.child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(cx.theme().danger)
                                    .child(error),
                            )
                        })
                        .child(h_flex().pt(px(4.0)).justify_end().child(action)),
                )
        });
        column.child(h_flex().w(px(width)).flex_wrap().items_start().gap(px(gap)).children(cards))
    }
}
