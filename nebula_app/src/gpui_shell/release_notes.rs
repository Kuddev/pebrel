//! One read-only Markdown view for Settings and the post-install notice.
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Task, Window, div, px,
};
use gpui_component::text::{TextView, TextViewState};

use super::prelude::*;
use crate::i18n::Message;
use crate::update_check::release_notes::ReleaseNotes;

pub(super) struct ReleaseNotesView {
    text: Option<Entity<TextViewState>>,
    error: Option<String>,
    empty: bool,
    task: Option<Task<()>>,
}

impl ReleaseNotesView {
    pub(super) fn new(notes: Option<ReleaseNotes>, cx: &mut Context<Self>) -> Self {
        let mut view = Self { text: None, error: None, empty: false, task: None };
        if let Some(notes) = notes {
            view.set_notes(notes, cx);
        } else {
            view.load(cx);
        }
        view
    }

    fn set_notes(&mut self, notes: ReleaseNotes, cx: &mut Context<Self>) {
        self.empty = notes.body.trim().is_empty();
        self.text = Some(cx.new(|cx| TextViewState::markdown(&notes.body, cx)));
        self.error = None;
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let task =
            cx.background_executor().spawn(async { crate::update_check::release_notes::current() });
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, cx| {
                view.task = None;
                match result {
                    Ok(notes) => view.set_notes(notes, cx),
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for ReleaseNotesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = super::config::ui_language(cx);
        let mut body = v_flex().w_full().min_w_0().gap_4().child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_2()
                .child(div().font_semibold().child(format!("v{}", env!("CARGO_PKG_VERSION"))))
                .child(
                    Button::new("release-notes-github")
                        .icon(IconName::ExternalLink)
                        .label(language.text(Message::UpdateOpenRelease))
                        .ghost()
                        .small()
                        .on_click(|_, _, cx| {
                            cx.open_url(&format!(
                                "{}/tag/v{}",
                                crate::update_check::RELEASES_PAGE,
                                env!("CARGO_PKG_VERSION")
                            ))
                        }),
                ),
        );
        if self.task.is_some() {
            body = body.child(
                h_flex()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child(language.text(Message::UpdateReleaseNotesLoading)),
            );
        } else if let Some(error) = &self.error {
            body =
                body.child(div().text_color(cx.theme().danger).child(
                    language.format(Message::UpdateReleaseNotesFailed, &[("error", error)]),
                ))
                .child(
                    Button::new("release-notes-retry")
                        .label(language.text(Message::UpdateReleaseNotesRetry))
                        .on_click(cx.listener(|view, _, _, cx| view.load(cx))),
                );
        } else if self.empty {
            body = body.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::UpdateReleaseNotesEmpty)),
            );
        } else if let Some(text) = &self.text {
            body = body.child(
                TextView::new(text)
                    .w_full()
                    .min_w_0()
                    .selectable(true)
                    .scrollable(false)
                    .on_link_click(|url, _, _, cx| {
                        if url.starts_with("https://") || url.starts_with("http://") {
                            cx.open_url(url);
                        }
                    }),
            );
        }
        body
    }
}

pub(super) fn open_dialog(notes: ReleaseNotes, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ReleaseNotesView::new(Some(notes), cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let language = super::config::ui_language(cx);
        let height = (f32::from(window.viewport_size().height) - 160.0).clamp(120.0, 440.0);
        super::prelude::center_modal_dialog(dialog, window, height + 120.0)
            .bg(super::theme::settings_panel_bg(cx))
            .title(language.text(Message::UpdateReleaseNotesTitle))
            .close_button(true)
            .overlay_closable(true)
            .child(
                div().w_full().h(px(height)).overflow_y_scrollbar().child(
                    div()
                        .w_full()
                        .debug_selector(|| "release-notes-content".into())
                        .child(view.clone()),
                ),
            )
            .footer(
                DialogFooter::new().child(
                    DialogClose::new().child(
                        Button::new("release-notes-close")
                            .debug_selector(|| "release-notes-close".into())
                            .primary()
                            .label(language.text(Message::CommonClose)),
                    ),
                ),
            )
    });
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{TestAppContext, size};
    use gpui_component::{Root, Theme, ThemeMode};

    struct DialogHost;

    impl Render for DialogHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(Root::render_dialog_layer(window, cx))
        }
    }

    #[gpui::test]
    fn installed_notes_render_in_the_dialog_and_close_by_click_or_escape(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| DialogHost);
            Root::new(host, window, cx)
        });
        for (theme, mode) in [
            (nebula_settings::ThemeName::Nord, ThemeMode::Dark),
            (nebula_settings::ThemeName::LinenLight, ThemeMode::Light),
        ] {
            cx.update(|window, cx| {
                let runtime = nebula_settings::RuntimeSettings::from_raw(
                    &nebula_settings::RawSettings::default(),
                );
                cx.set_global(crate::gpui_shell::config::Settings::load_with_runtime(
                    theme, runtime,
                ));
                Theme::change(mode, Some(window), cx);
            });
            for width in [420.0, 900.0] {
                cx.simulate_resize(size(px(width), px(620.0)));
                cx.update(|window, cx| open_dialog(ReleaseNotes {
                    version: env!("CARGO_PKG_VERSION").into(),
                    body: "# Changes\n\n**Bold**, `inline`, [release](https://github.com/Kuddev/pebrel)\n\n- Item\n\n```sh\npwd\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n".repeat(8),
                }, window, cx));
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let close = cx.debug_bounds("release-notes-close").expect("visible close action");
                let content = cx.debug_bounds("release-notes-content").expect("rendered Markdown");
                assert!(content.size.width > px(0.0));
                assert!(content.size.width <= px(width));
                assert!(close.origin.y >= px(0.0) && close.bottom() <= px(620.0));
                if width == 420.0 {
                    cx.simulate_click(close.center(), gpui::Modifiers::default());
                } else {
                    cx.simulate_keystrokes("escape");
                }
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                assert!(cx.debug_bounds("release-notes-content").is_none());
            }
        }
    }
}
