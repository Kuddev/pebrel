//! Cached host search, grouping, virtual rows and credential-free exchange.

use super::*;
use gpui::AppContext as _;
use gpui_component::menu::{PopupMenu, PopupMenuItem};

mod exchange;
#[cfg(test)]
mod tests;

// Virtual rows share the prototype panel separators; hit testing uses the full row.
const HOST_ROW_HEIGHT: f32 = 61.0;

/// Keep the dropdown's own selected state authoritative while revealing the
/// trigger for row hover or keyboard focus. Opacity preserves its tab stop and
/// fixed hit area, so showing the menu never shifts the Connect button.
#[derive(IntoElement)]
struct HostMoreTrigger {
    button: Button,
    hover_group: SharedString,
}

impl gpui::Styled for HostMoreTrigger {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.button.style()
    }
}

impl gpui::InteractiveElement for HostMoreTrigger {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.button.interactivity()
    }
}

impl gpui_component::Selectable for HostMoreTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.button = self.button.selected(selected);
        self
    }

    fn is_selected(&self) -> bool {
        self.button.is_selected()
    }
}

impl gpui_component::menu::DropdownMenu for HostMoreTrigger {}

impl gpui::RenderOnce for HostMoreTrigger {
    fn render(self, _window: &mut Window, _cx: &mut gpui::App) -> impl IntoElement {
        let open = self.button.is_selected();
        self.button
            .opacity(if open { 1.0 } else { 0.0 })
            .group_hover(self.hover_group, |style| style.opacity(1.0))
            .focus(|style| style.opacity(1.0))
    }
}

/// Center the glyph's ink, not its monospace advance or the surrounding text line.
fn host_icon(glyph: char, family: SharedString, color: gpui::Hsla) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, _| {
            let font = gpui::font(family);
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&font);
            let base_size = px(18.0);
            let ink = text_system.typographic_bounds(font_id, base_size, glyph).ok();
            let scale = ink
                .filter(|ink| ink.size.width > px(0.0) && ink.size.height > px(0.0))
                .map(|ink| 18.0 / f32::from(ink.size.width.max(ink.size.height)))
                .unwrap_or(1.0);
            let font_size = base_size * scale;
            let text: SharedString = glyph.to_string().into();
            let line = text_system.shape_line(
                text.clone(),
                font_size,
                &[gpui::TextRun {
                    len: text.len(),
                    font,
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );
            let height = line.ascent + line.descent;
            let origin = if let Some(ink) = ink {
                // Font coordinates are relative to the baseline with positive Y upwards.
                let center = ink.center() * scale;
                bounds.center() - gpui::point(center.x, line.ascent - center.y)
            } else {
                bounds.center() - gpui::point(line.width / 2.0, height / 2.0)
            };
            (line, origin, height)
        },
        |_, (line, origin, height), window, cx| {
            if let Err(error) = line.paint(origin, height, gpui::TextAlign::Left, None, window, cx)
            {
                log::warn!("Unable to paint SSH host icon: {error}");
            }
        },
    )
    .size(px(22.0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::gpui_shell) enum HostScope {
    All,
    Pinned,
    Recent,
}

pub(in crate::gpui_shell) struct HostLibraryState {
    pub search: Entity<InputState>,
    pub group: Entity<InputState>,
    pub tags: Entity<InputState>,
    pub notes: Entity<InputState>,
    scope: HostScope,
    group_filter: Option<String>,
    scroll: gpui::UniformListScrollHandle,
    pub(super) busy: bool,
    sequence: u64,
}

impl HostLibraryState {
    pub(in crate::gpui_shell) fn new(window: &mut Window, cx: &mut Context<SettingsPane>) -> Self {
        let language = crate::gpui_shell::config::ui_language(cx);
        Self {
            search: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(language.text(Message::HostsSearchPlaceholder))
            }),
            group: cx.new(|cx| {
                InputState::new(window, cx).placeholder(language.text(Message::HostsGroup))
            }),
            tags: cx.new(|cx| {
                InputState::new(window, cx).placeholder(language.text(Message::HostsTagsHint))
            }),
            notes: cx.new(|cx| InputState::new(window, cx).multi_line(true).soft_wrap(true)),
            scope: HostScope::All,
            group_filter: None,
            scroll: Default::default(),
            busy: false,
            sequence: 0,
        }
    }

    pub(in crate::gpui_shell) fn reset_scroll(&self) {
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
    }
}

impl SettingsPane {
    pub(in crate::gpui_shell) fn prepare_launcher_ssh_host(
        &mut self,
        host: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ssh_library.scope = HostScope::All;
        self.ssh_library.group_filter = None;
        self.ssh_library.search.update(cx, |input, cx| input.set_value(host, window, cx));
        self.ssh_library.reset_scroll();
    }

    fn filtered_library_hosts(&self, cx: &gpui::App) -> Vec<String> {
        let query = self.ssh_library.search.read(cx).value();
        let mut hosts = if self.ssh_library.scope == HostScope::Recent {
            self.ssh_hosts
                .saved
                .iter()
                .filter(|host| !self.ssh_hosts.hidden.contains(host))
                .cloned()
                .collect()
        } else {
            self.ssh_hosts.merged()
        };
        if self.ssh_library.scope == HostScope::Pinned {
            hosts.retain(|host| self.ssh_hosts.is_pinned(host));
        }
        self.ssh_hosts.profiles.filter_hosts(
            hosts,
            &query,
            false,
            self.ssh_library.group_filter.as_deref(),
        )
    }

    fn library_controls(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        let group_filter = self.ssh_library.group_filter.clone();
        let groups = self.ssh_hosts.profiles.groups();
        let group_label = match group_filter.as_deref() {
            None => language.text(Message::HostsAllGroups).to_owned(),
            Some("") => language.text(Message::HostsUngrouped).to_owned(),
            Some(group) => group.to_owned(),
        };
        h_flex()
            .id("ssh-library-controls")
            .debug_selector(|| "ssh-library-controls".into())
            .w_full()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                h_flex()
                    .p(px(3.0))
                    .gap(px(2.0))
                    .rounded(px(8.0))
                    .bg(cx.theme().secondary)
                    .children(
                        [
                            (HostScope::All, Message::HostsScopeAll),
                            (HostScope::Pinned, Message::HostsPinned),
                            (HostScope::Recent, Message::HostsRecent),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (scope, label))| {
                            let selected = self.ssh_library.scope == scope;
                            Button::new(("host-scope", index))
                                .debug_selector(move || format!("host-scope-{index}"))
                                .label(language.text(label))
                                .ghost()
                                .small()
                                .h(px(28.0))
                                .rounded(px(6.0))
                                .selected(selected)
                                .when(selected, |button| {
                                    button.bg(crate::gpui_shell::theme::settings_panel_bg(cx))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.ssh_library.scope = scope;
                                    this.ssh_library.reset_scroll();
                                    cx.notify();
                                }))
                        }),
                    ),
            )
            .child(
                Button::new("host-group-filter")
                    .debug_selector(|| "host-group-filter".into())
                    .label(group_label)
                    .small()
                    .w(px(140.0))
                    .h(px(32.0))
                    .dropdown_caret(true)
                    .dropdown_menu(move |mut menu, _, _| {
                        let choices = std::iter::once((
                            None,
                            language.text(Message::HostsAllGroups).to_owned(),
                        ))
                        .chain(std::iter::once((
                            Some(String::new()),
                            language.text(Message::HostsUngrouped).to_owned(),
                        )))
                        .chain(groups.iter().map(|group| (Some(group.clone()), group.clone())));
                        for (choice, label) in choices {
                            let owner = owner.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label).checked(choice == group_filter).on_click(
                                    move |_, _, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            this.ssh_library.group_filter = choice.clone();
                                            this.ssh_library.reset_scroll();
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("ssh-inline-filter")
                    .debug_selector(|| "ssh-inline-filter".into())
                    .w(px(240.0))
                    .max_w_full()
                    .child(
                        gpui::Styled::h(Input::new(&self.ssh_library.search).small(), px(32.0))
                            .prefix(Icon::new(IconName::Search).size(px(14.0))),
                    ),
            )
            .into_any_element()
    }

    fn library_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap(px(6.0))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(22.0))
                            .line_height(px(30.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::HostsTitle)),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("hosts-exchange")
                                    .label(language.text(Message::HostsExchange))
                                    .ghost()
                                    .small()
                                    .h(px(32.0))
                                    .dropdown_caret(true)
                                    .disabled(self.ssh_library.busy)
                                    .dropdown_menu(move |menu, _, _| {
                                        let import_owner = owner.clone();
                                        let export_owner = owner.clone();
                                        menu.item(
                                            PopupMenuItem::new(language.text(Message::HostsImport))
                                                .on_click(move |_, window, cx| {
                                                    let _ = import_owner.update(cx, |this, cx| {
                                                        this.import_host_csv(window, cx)
                                                    });
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new(language.text(Message::HostsExport))
                                                .on_click(move |_, window, cx| {
                                                    let _ = export_owner.update(cx, |this, cx| {
                                                        this.export_host_csv(window, cx)
                                                    });
                                                }),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("ssh-add-host")
                                    .debug_selector(|| "ssh-add-host".into())
                                    .icon(IconName::Plus)
                                    .label(language.text(Message::HostsAdd))
                                    .small()
                                    .h(px(32.0))
                                    .primary()
                                    .disabled(self.ssh_library.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_ssh_editor(None, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .max_w(px(598.0))
                    .text_size(px(13.0))
                    .line_height(px(21.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::HostsListSubtitle)),
            )
            .into_any_element()
    }

    fn library_config_banner(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let hosts = self.ssh_hosts.merged();
        let configured = hosts.iter().filter(|host| self.ssh_hosts.is_from_config(host)).count();
        let hidden = self.ssh_hosts.hidden_hosts().len();
        h_flex()
            .id("ssh-config-banner")
            .debug_selector(|| "ssh-config-banner".into())
            .w_full()
            .gap_1()
            .items_center()
            .flex_wrap()
            .child(div().text_size(px(12.5)).text_color(cx.theme().muted_foreground).child(
                language.format(
                    Message::HostsSummary,
                    &[("total", &hosts.len().to_string()), ("config", &configured.to_string())],
                ),
            ))
            .child(
                Button::new("ssh-import")
                    .label(language.text(Message::HostsReload))
                    .small()
                    .ghost()
                    .h(px(28.0))
                    .text_color(cx.theme().link)
                    .loading(self.ssh_library.busy)
                    .disabled(self.ssh_library.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_host_library(cx))),
            )
            .child(div().flex_1())
            .when(hidden > 0, |footer| {
                footer.child(
                    Button::new("ssh-toggle-hidden")
                        .debug_selector(|| "ssh-toggle-hidden".into())
                        .ghost()
                        .small()
                        .h(px(28.0))
                        .text_color(cx.theme().link)
                        .label(if self.ssh_show_hidden {
                            language.text(Message::HostsCollapseHidden).to_owned()
                        } else {
                            language.format(
                                Message::HostsHiddenCount,
                                &[("count", &hidden.to_string())],
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ssh_show_hidden = !this.ssh_show_hidden;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    pub(super) fn host_organization_fields(
        &self,
        window: &Window,
        cx: &gpui::App,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        v_flex()
            .mt_4()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().child(language.text(Message::HostsGroup)))
                            .child(editor::editor_input(
                                &self.ssh_library.group,
                                language.text(Message::HostsGroup),
                                window,
                                cx,
                            )),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().child(language.text(Message::HostsTags)))
                            .child(editor::editor_input(
                                &self.ssh_library.tags,
                                language.text(Message::HostsTags),
                                window,
                                cx,
                            )),
                    ),
            )
            .child(div().text_xs().child(language.text(Message::HostsNotes)))
            .child(Input::new(&self.ssh_library.notes).w_full().h(px(80.0)))
            .into_any_element()
    }

    fn render_library_host(
        &self,
        host: String,
        ix: usize,
        host_count: usize,
        labels: &std::collections::HashMap<String, String>,
        icons: &std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hairline = crate::gpui_shell::theme::settings_hairline(cx);
        let pinned = self.ssh_hosts.is_pinned(&host);
        let from_config = self.ssh_hosts.is_from_config(&host);
        let confirm = self.ssh_delete_confirm.as_deref() == Some(host.as_str());
        let label = labels.get(&host).cloned().unwrap_or_else(|| host.clone());
        let group = self.ssh_hosts.profiles.organization(&host).group.clone();
        let profile = self.ssh_hosts.profiles.for_destination(&host);
        let mut detail = self.ssh_hosts.profiles.connection_destination(&host).to_owned();
        if profile.connection.jump_mode == crate::ssh_profiles::SshHostJumpMode::Host {
            detail.push_str(" · ");
            detail.push_str(
                &language.format(Message::HostsVia, &[("host", &profile.connection.jump_host)]),
            );
        }
        detail.push_str(" · ");
        detail.push_str(&host_auth_label(&profile, language));
        let os_icon = crate::display::ui::os_icons::resolve(icons.get(&host).map(String::as_str));
        let owner = cx.entity().downgrade();
        let context_owner = owner.clone();
        let context_host = host.clone();
        let menu_host = host.clone();
        let hover_group: SharedString = format!("ssh-host-actions-{ix}").into();
        let connect_host = host.clone();
        let delete_host = host;
        let chip = |label: String| {
            div()
                .max_w(px(110.0))
                .truncate()
                .px(px(6.0))
                .py(px(1.0))
                .rounded(px(5.0))
                .text_size(px(11.0))
                .line_height(px(16.0))
                .text_color(muted)
                .border_1()
                .border_color(theme.border)
                .child(label)
        };
        let title = h_flex()
            .min_w_0()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.0))
                    .line_height(px(21.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(label),
            )
            .when(pinned, |line| {
                line.child(
                    Icon::default()
                        .path(crate::gpui_shell::assets::nav::PIN)
                        .size(px(12.0))
                        .text_color(theme.link),
                )
            })
            .when(!group.is_empty(), |line| line.child(chip(group)))
            .when(from_config, |line| line.child(chip("ssh config".into())));
        let actions = if confirm {
            h_flex()
                .gap(px(4.0))
                .flex_shrink_0()
                .child(
                    Button::new(SharedString::from(format!("ssh-delete-{ix}")))
                        .debug_selector(move || format!("ssh-delete-{ix}"))
                        .label(language.text(Message::HostsConfirmDelete))
                        .danger()
                        .small()
                        .h(px(32.0))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.delete_ssh_host(&delete_host, cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("ssh-cancel-delete-{ix}")))
                        .debug_selector(move || format!("ssh-cancel-delete-{ix}"))
                        .label(language.text(Message::CommonCancel))
                        .ghost()
                        .small()
                        .h(px(32.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ssh_delete_confirm = None;
                            cx.notify();
                        })),
                )
        } else {
            h_flex()
                .gap(px(4.0))
                .flex_shrink_0()
                .child(
                    HostMoreTrigger {
                        hover_group: hover_group.clone(),
                        button: Button::new(SharedString::from(format!("ssh-more-{ix}")))
                            .debug_selector(move || format!("ssh-more-{ix}"))
                            .icon(IconName::Ellipsis)
                            .ghost()
                            .small()
                            .size(px(32.0))
                            .tooltip(language.text(Message::HostsMore)),
                    }
                    .dropdown_menu(move |menu, _, _| {
                        host_actions(
                            menu,
                            owner.clone(),
                            menu_host.clone(),
                            pinned,
                            from_config,
                            language,
                        )
                    }),
                )
                .child(
                    Button::new(SharedString::from(format!("ssh-connect-{ix}")))
                        .debug_selector(move || format!("ssh-connect-{ix}"))
                        .label(language.text(Message::HostsConnect))
                        .small()
                        .h(px(32.0))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.emit(SettingsPaneEvent::LaunchSsh(connect_host.clone()));
                            this.ssh_status = Some(SshStatus::Opening(
                                this.ssh_hosts
                                    .profiles
                                    .connection_destination(&connect_host)
                                    .to_owned(),
                            ));
                            cx.notify();
                        })),
                )
        };
        let icon = div()
            .id(SharedString::from(format!("ssh-host-icon-{ix}")))
            .debug_selector(move || format!("ssh-host-icon-{ix}"))
            .size(px(32.0))
            .flex_shrink_0()
            .rounded(px(8.0))
            .bg(theme.secondary)
            .flex()
            .items_center()
            .justify_center()
            .child(host_icon(
                os_icon.glyph,
                crate::font_install::REQUIRED_FONT_FAMILY.into(),
                muted,
            ));
        h_flex()
            .id(SharedString::from(format!("ssh-host-row-{ix}")))
            .debug_selector(move || format!("ssh-host-row-{ix}"))
            .group(hover_group)
            .h(px(HOST_ROW_HEIGHT))
            .w_full()
            .px(px(14.0))
            .items_center()
            .gap(px(14.0))
            .when(ix + 1 < host_count, |row| row.border_b_1().border_color(hairline))
            .hover(|row| row.bg(crate::gpui_shell::theme::settings_hover_bg(cx, false)))
            .context_menu(move |menu, _, _| {
                host_actions(
                    menu,
                    context_owner.clone(),
                    context_host.clone(),
                    pinned,
                    from_config,
                    language,
                )
            })
            .child(icon)
            .child(
                v_flex().flex_1().min_w_0().justify_center().gap(px(1.0)).child(title).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .line_height(px(18.0))
                        .font_family(crate::font_install::REQUIRED_FONT_FAMILY)
                        .text_color(muted)
                        .child(detail),
                ),
            )
            .child(actions)
            .into_any_element()
    }

    pub(in crate::gpui_shell) fn section_ssh(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let controls = self.library_controls(cx);
        let header = self.library_header(cx);
        let config_banner = self.library_config_banner(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hosts = self.filtered_library_hosts(cx);
        let host_count = hosts.len();
        let labels = self.ssh_hosts.profiles.labels();
        let icons = self.ssh_hosts.profiles.icons();
        let hidden: Vec<String> = self.ssh_hosts.hidden_hosts().to_vec();

        let row_hosts = hosts.clone();
        let host_rows = gpui::uniform_list(
            "ssh-library-hosts",
            host_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        this.render_library_host(
                            row_hosts[index].clone(),
                            index,
                            host_count,
                            &labels,
                            &icons,
                            cx,
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.ssh_library.scroll)
        .w_full()
        .h(px(HOST_ROW_HEIGHT * host_count.clamp(1, 8) as f32));

        let hidden_rows = self.ssh_show_hidden.then(|| {
            hidden
                .iter()
                .enumerate()
                .map(|(ix, host)| {
                    let restore_host = host.clone();
                    h_flex()
                        .id(("ssh-hidden-row", ix))
                        .debug_selector(move || format!("ssh-hidden-row-{ix}"))
                        .h(px(40.0))
                        .w_full()
                        .px_3()
                        .items_center()
                        .gap_2()
                        .when(ix + 1 < hidden.len(), |row| {
                            row.border_b_1().border_color(theme.border.opacity(0.5))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(muted)
                                .truncate()
                                .child(host.clone()),
                        )
                        .child(
                            NebulaButton::new(SharedString::from(format!("ssh-restore-{ix}")))
                                .label(language.pick("恢复", "Restore"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.ssh_apply(
                                        |lists| lists.restore_hidden(&restore_host),
                                        SshStatus::Restored(restore_host.clone()),
                                        cx,
                                    );
                                })),
                        )
                })
                .collect::<Vec<_>>()
        });

        let undo_bar = self.ssh_delete_undo.as_ref().map(|undo| (undo.host.clone(), undo.seq));

        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .w_full()
            .max_w(px(1000.0))
            .px(px(if f32::from(window.viewport_size().width) < 1000.0 { 12.0 } else { 36.0 }))
            .pt(px(16.0))
            .pb(px(110.0))
            .child(header)
            .child(div().mt(px(10.0)).child(controls))
            .children(
                self.ssh_hosts
                    .load_error
                    .as_ref()
                    .map(|error| div().text_sm().text_color(theme.danger).child(error.clone())),
            )
            .child(
                v_flex()
                    .w_full()
                    .border_1()
                    .border_color(crate::gpui_shell::theme::settings_hairline(cx))
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .bg(theme.foreground.opacity(0.028))
                    .when(host_count > 0, |card| card.child(host_rows))
                    .when(host_count == 0, |card| {
                        card.child(
                            v_flex()
                                .py_6()
                                .gap_1()
                                .items_center()
                                .child(
                                    div()
                                        .text_color(muted)
                                        .child(language.text(Message::HostsNoMatches)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(language.tr("settings.ssh.empty_hint")),
                                ),
                        )
                    })
                    .when_some(hidden_rows, |panel, rows| {
                        panel.child(v_flex().w_full().children(rows))
                    })
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(crate::gpui_shell::theme::settings_hairline(cx))
                            .child(config_banner),
                    ),
            )
            .when_some(undo_bar, |group, (host, _)| {
                group.child(div().h(px(8.0))).child(
                    h_flex()
                        .h(px(36.0))
                        .px_3()
                        .items_center()
                        .gap_2()
                        .rounded(px(6.0))
                        .bg(theme.muted)
                        .child(Icon::new(IconName::Undo2).xsmall().text_color(muted))
                        .child(div().flex_1().text_sm().child(SharedString::from(format!(
                            "{} {host}; {} {SSH_DELETE_UNDO_SECS} {}",
                            language.pick("已删除", "Deleted"),
                            language.pick("可在", "undo within"),
                            language.pick("秒", "seconds")
                        ))))
                        .child(
                            NebulaButton::new("ssh-undo-delete")
                                .label(language.pick("撤销", "Undo"))
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| this.undo_ssh_delete(cx))),
                        ),
                )
            })
            .when_some(self.ssh_status.clone(), |group, status| {
                let error = status.is_error();
                let message = status.text(language);
                group.child(
                    div()
                        .pt(px(6.0))
                        .text_sm()
                        .text_color(if error { theme.danger } else { theme.success })
                        .child(message),
                )
            })
    }
}

fn host_auth_label(
    profile: &crate::ssh_profiles::SshProfileAuth,
    language: crate::display::UiLanguage,
) -> String {
    use crate::ssh_profiles::SshAuthMode;
    if profile.auth == SshAuthMode::PublicKey {
        if let Some(name) = profile.private_keys.first().and_then(|path| path.file_name()) {
            return language.format(Message::HostsKeyFile, &[("name", &name.to_string_lossy())]);
        }
    }
    language
        .text(match profile.auth {
            SshAuthMode::Auto => Message::HostsAuthAuto,
            SshAuthMode::Password => Message::HostsAuthPassword,
            SshAuthMode::PublicKey => Message::HostsAuthKey,
            SshAuthMode::KeyboardInteractive => Message::HostsAuthInteractive,
        })
        .to_owned()
}

/// Both the right-click menu and keyboard-accessible row button use these actions.
fn host_actions(
    menu: PopupMenu,
    owner: gpui::WeakEntity<SettingsPane>,
    host: String,
    pinned: bool,
    from_config: bool,
    language: crate::display::UiLanguage,
) -> PopupMenu {
    let connect_owner = owner.clone();
    let edit_owner = owner.clone();
    let copy_owner = owner.clone();
    let pin_owner = owner.clone();
    let connect_host = host.clone();
    let edit_host = host.clone();
    let copy_host = host.clone();
    let pin_host = host.clone();
    menu.item(PopupMenuItem::new(language.text(Message::LauncherConnect)).on_click(
        move |_, _, cx| {
            let _ = connect_owner.update(cx, |this, cx| {
                cx.emit(SettingsPaneEvent::LaunchSsh(connect_host.clone()));
                this.ssh_status = Some(SshStatus::Opening(
                    this.ssh_hosts.profiles.connection_destination(&connect_host).to_owned(),
                ));
                cx.notify();
            });
        },
    ))
    .item(PopupMenuItem::new(language.text(Message::LauncherEdit)).on_click(
        move |_, window, cx| {
            let _ = edit_owner
                .update(cx, |this, cx| this.open_ssh_editor(Some(edit_host.clone()), window, cx));
        },
    ))
    .item(PopupMenuItem::new(language.text(Message::CommonCopy)).on_click(move |_, _, cx| {
        let _ = copy_owner.update(cx, |this, cx| this.duplicate_ssh_host(copy_host.clone(), cx));
    }))
    .item(
        PopupMenuItem::new(language.text(if pinned {
            Message::HostsUnpin
        } else {
            Message::HostsPin
        }))
        .on_click(move |_, _, cx| {
            let _ = pin_owner.update(cx, |this, cx| {
                this.ssh_apply(|lists| lists.toggle_pin(&pin_host), SshStatus::Pinned, cx)
            });
        }),
    )
    .separator()
    .item(
        PopupMenuItem::new(language.text(if from_config {
            Message::SettingsSshHideConfigHost
        } else {
            Message::LauncherDelete
        }))
        .on_click(move |_, _, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.ssh_delete_confirm = Some(host.clone());
                cx.notify();
            });
        }),
    )
}
