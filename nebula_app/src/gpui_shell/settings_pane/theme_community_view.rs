//! Community catalog cards in the theme picker's list column.

use super::appearance_picker::{AppearanceColors, AppearanceSelection};
use super::theme_community::{CatalogLoad, InstallState, Preview};
use super::*;
use crate::i18n::Message;
use crate::theme_library::community::Installed;
use gpui::{ObjectFit, Role, StyledImage as _, accesskit::Toggled};
use gpui_component::{Sizable as _, progress::Progress};
use std::sync::atomic::Ordering;

const CARD_RADIUS: f32 = 8.0;
const CARD_BORDER: f32 = 1.0;

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
            let installed_choice = match &installed {
                Installed::Current(theme_id) => picker
                    .custom_themes
                    .iter()
                    .position(|document| document.id() == Some(theme_id.as_str()))
                    .map(AppearanceSelection::Custom),
                _ => None,
            };
            let previewing = installed_choice == Some(picker.draft);
            let inner_radius = px(CARD_RADIUS - CARD_BORDER);
            let preview = match community.previews.get(&entry.id) {
                Some(Preview::Ready(image)) => img(image.clone())
                    .size_full()
                    .rounded_t(inner_radius)
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                state => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.5))
                    .text_color(colors.secondary)
                    .child(language.text(if matches!(state, Some(Preview::Loading)) {
                        Message::ThemeCommunityPreviewLoading
                    } else {
                        Message::ThemeCommunityNoPreview
                    }))
                    .into_any_element(),
            };
            let failure = match install {
                Some(InstallState::Failed(error)) => Some(error.clone()),
                _ => None,
            };
            let footer = match install {
                Some(InstallState::Running(progress)) => {
                    let downloaded = progress.load(Ordering::Relaxed);
                    let downloading = downloaded < entry.archive.bytes;
                    let percent = (downloaded * 100 / entry.archive.bytes).min(100);
                    v_flex()
                        .debug_selector(move || format!("community-progress-{index}"))
                        .gap(px(5.0))
                        .child(note(if downloading {
                            language.format(
                                Message::ThemeCommunityDownloading,
                                &[("percent", &percent.to_string())],
                            )
                        } else {
                            language.text(Message::ThemeCommunityInstalling).to_owned()
                        }))
                        .when(downloading, |status| {
                            status.child(
                                Progress::new(("community-download", index))
                                    .with_size(px(3.0))
                                    .color(colors.primary)
                                    .value(percent as f32),
                            )
                        })
                        .into_any_element()
                },
                _ if installed_choice.is_some() => h_flex()
                    .gap(px(6.0))
                    .justify_between()
                    .text_size(px(11.5))
                    .text_color(colors.secondary)
                    .child(language.text(if previewing {
                        Message::ThemeCommunityPreviewing
                    } else {
                        Message::ThemeCommunityInstalled
                    }))
                    .child(if previewing {
                        Icon::new(IconName::Check)
                            .size(px(14.0))
                            .text_color(colors.primary)
                            .into_any_element()
                    } else {
                        h_flex()
                            .gap(px(4.0))
                            .text_color(colors.ink)
                            .child(language.text(Message::ThemeCommunitySelect))
                            .child(Icon::new(IconName::ChevronRight).size(px(12.0)))
                            .into_any_element()
                    })
                    .into_any_element(),
                _ => {
                    let id = entry.id.clone();
                    h_flex()
                        .justify_end()
                        .child(
                            Button::new(("community-action", index))
                                .debug_selector(move || format!("community-action-{index}"))
                                .h(px(30.0))
                                .px(px(11.0))
                                .text_size(px(12.0))
                                .rounded(px(6.0))
                                .label(if failure.is_some() {
                                    language.text(Message::ThemeCommunityRetry).to_owned()
                                } else if installed == Installed::Other {
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
                        )
                        .into_any_element()
                },
            };
            v_flex()
                .id(("community-theme", index))
                .debug_selector(move || format!("community-theme-{index}"))
                .w(px(card_width))
                .flex_shrink_0()
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(if previewing { colors.primary } else { colors.line })
                .when_some(installed_choice.filter(|_| !busy), |card, choice| {
                    let focus = picker
                        .options
                        .iter()
                        .find(|(option, _)| *option == choice)
                        .map(|(_, focus)| focus.clone().tab_stop(true));
                    card.when_some(focus, |card, focus| card.track_focus(&focus))
                        .role(Role::RadioButton)
                        .aria_label(entry.name.clone())
                        .aria_toggled(if previewing { Toggled::True } else { Toggled::False })
                        .cursor_pointer()
                        .when(!previewing, |card| {
                            card.hover(move |style| style.border_color(colors.control))
                        })
                        .active(move |style| style.bg(colors.selected))
                        .focus_visible(move |style| style.border_color(colors.primary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.preview_community_theme(choice, window, cx);
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                                cx.stop_propagation();
                                this.preview_community_theme(choice, window, cx);
                            }
                        }))
                })
                .child(
                    div()
                        .w_full()
                        .h(px((card_width * 0.5).round()))
                        .rounded_t(inner_radius)
                        .bg(colors.subtle)
                        .overflow_hidden()
                        .child(preview),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .justify_between()
                        .p(px(10.0))
                        .gap(px(8.0))
                        .child(
                            v_flex()
                                .gap(px(4.0))
                                .child(
                                    h_flex()
                                        .gap(px(6.0))
                                        .justify_between()
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(13.0))
                                                .font_semibold()
                                                .child(entry.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(px(10.5))
                                                .text_color(colors.secondary)
                                                .child(entry.version.clone()),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap(px(8.0))
                                        .justify_between()
                                        .text_size(px(11.5))
                                        .text_color(colors.secondary)
                                        .child(div().min_w_0().truncate().child(language.format(
                                            Message::ThemeCommunityAuthor,
                                            &[("author", &entry.author.name)],
                                        )))
                                        .when(installed_choice.is_none(), |metadata| {
                                            metadata.child(div().flex_shrink_0().child(
                                                language.format(
                                                    Message::ThemeCommunityDownloadSize,
                                                    &[(
                                                        "size",
                                                        &download_size(entry.archive.bytes),
                                                    )],
                                                ),
                                            ))
                                        }),
                                )
                                .when_some(entry.description.clone(), |content, description| {
                                    content.child(
                                        div()
                                            .text_size(px(11.5))
                                            .line_height(gpui::relative(1.5))
                                            .text_color(colors.ink)
                                            .child(description),
                                    )
                                })
                                .when_some(failure, |content, error| {
                                    content.child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(cx.theme().danger)
                                            .child(error),
                                    )
                                }),
                        )
                        .child(footer),
                )
        });
        column.child(h_flex().w(px(width)).flex_wrap().items_stretch().gap(px(gap)).children(cards))
    }
}
