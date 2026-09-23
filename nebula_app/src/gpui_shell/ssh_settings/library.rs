//! Cached host search, grouping, virtual rows and credential-free exchange.

use super::*;
use gpui::AppContext as _;
use gpui_component::menu::PopupMenuItem;

mod exchange;
#[cfg(test)]
mod tests;

// Each virtual item includes its bottom gutter, so scrolling and hit tests use
// the actual card extent rather than the previous contiguous table-row height.
const HOST_CARD_HEIGHT: f32 = 68.0;
const HOST_CARD_EXTENT: f32 = HOST_CARD_HEIGHT + 8.0;

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
    Managed,
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
                InputState::new(window, cx).placeholder(language.text(Message::HostsSearch))
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
            self.ssh_library.scope == HostScope::Managed,
            self.ssh_library.group_filter.as_deref(),
        )
    }

    fn library_controls(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let weak = cx.entity().downgrade();
        let group_filter = self.ssh_library.group_filter.clone();
        let groups = self.ssh_hosts.profiles.groups();
        let group_label = match group_filter.as_deref() {
            None => language.text(Message::HostsAllGroups).to_owned(),
            Some("") => language.text(Message::HostsUngrouped).to_owned(),
            Some(group) => group.to_owned(),
        };
        h_flex()
            .id("ssh-library-controls")
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                h_flex().gap_1().flex_wrap().children(
                    [
                        (HostScope::All, Message::HostsAll),
                        (HostScope::Pinned, Message::HostsPinned),
                        (HostScope::Managed, Message::HostsManaged),
                        (HostScope::Recent, Message::HostsRecent),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (scope, label))| {
                        Button::new(("host-scope", index))
                            .debug_selector(move || format!("host-scope-{index}"))
                            .label(language.text(label))
                            .small()
                            .ghost()
                            .selected(self.ssh_library.scope == scope)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.ssh_library.scope = scope;
                                this.ssh_library.reset_scroll();
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Button::new("host-group-filter").label(group_label).small().dropdown_menu(
                            move |mut menu, _, _| {
                                let choices = std::iter::once((
                                    None,
                                    language.text(Message::HostsAllGroups).to_owned(),
                                ))
                                .chain(std::iter::once((
                                    Some(String::new()),
                                    language.text(Message::HostsUngrouped).to_owned(),
                                )))
                                .chain(
                                    groups.iter().map(|group| (Some(group.clone()), group.clone())),
                                );
                                for (choice, label) in choices {
                                    let owner = weak.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label)
                                            .checked(choice == group_filter)
                                            .on_click(move |_, _, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    this.ssh_library.group_filter = choice.clone();
                                                    this.ssh_library.reset_scroll();
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }
                                menu
                            },
                        ),
                    )
                    .child(
                        div()
                            .id("ssh-inline-filter")
                            .debug_selector(|| "ssh-inline-filter".into())
                            .w(px(210.0))
                            .child(
                                Input::new(&self.ssh_library.search)
                                    .small()
                                    .prefix(Icon::new(IconName::Search).size(px(14.0))),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn library_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        h_flex()
            .gap_3()
            .items_center()
            .flex_wrap()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(220.0))
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::HostsTitle)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text(Message::HostsSubtitle)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("hosts-exchange")
                            .label(language.text(Message::HostsExchange))
                            .small()
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
                            .icon(IconName::Plus)
                            .label(language.text(Message::HostsAdd))
                            .small()
                            .primary()
                            .disabled(self.ssh_library.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_ssh_editor(None, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn library_config_banner(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let count = self.ssh_hosts.configured.len();
        let text = if count == 0 {
            language.text(Message::HostsConfigEmpty).to_owned()
        } else {
            language.format(Message::HostsConfigShared, &[("count", &count.to_string())])
        };
        h_flex()
            .id("ssh-config-banner")
            .gap_2()
            .px_3()
            .py_2()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary.opacity(0.35))
            .child(Icon::new(IconName::Info).size(px(14.0)).text_color(cx.theme().muted_foreground))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text),
            )
            .child(
                Button::new("ssh-import")
                    .label(language.text(Message::HostsReload))
                    .small()
                    .ghost()
                    .loading(self.ssh_library.busy)
                    .disabled(self.ssh_library.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_host_library(cx))),
            )
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
        _host_count: usize,
        labels: &std::collections::HashMap<String, String>,
        icons: &std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let hover_bg = crate::gpui_shell::theme::settings_hover_bg(cx, false);
        let muted = theme.muted_foreground;
        let symbol_family: SharedString = crate::font_install::REQUIRED_FONT_FAMILY.into();
        let font_px = cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .map(|s| s.ui_font_size_px)
            .unwrap_or(15.0);
        let title_h = font_px * 0.9;
        let subtitle_h = font_px * 0.8;
        let delete_confirm = self.ssh_delete_confirm.clone();

        let pinned = self.ssh_hosts.is_pinned(&host);
        let from_config = self.ssh_hosts.is_from_config(&host);
        let confirm = delete_confirm.as_deref() == Some(host.as_str());
        let label = labels.get(&host).cloned().unwrap_or_else(|| host.clone());
        let group = self.ssh_hosts.profiles.organization(&host).group.clone();
        let profile = self.ssh_hosts.profiles.for_destination(&host);
        let auth = host_auth_label(&profile, language);
        // 行首 OS 图标（旧壳裁定 2026-08-09）：id 取自 ssh_profiles 存储，
        // 未认出回落通用终端形状；mono 字体渲染 Nerd Font 字位。
        let os_icon = crate::display::ui::os_icons::resolve(icons.get(&host).map(String::as_str));
        let connect_host = host.clone();
        let edit_host = host.clone();
        let pin_host = host.clone();
        let delete_host = host.clone();
        div()
            .h(px(HOST_CARD_EXTENT))
            .w_full()
            .pb(px(8.0))
            .child(
                h_flex()
                    .id(SharedString::from(format!("ssh-host-row-{ix}")))
                    .debug_selector(move || format!("ssh-host-row-{ix}"))
                    .h(px(HOST_CARD_HEIGHT))
                    .w_full()
                    .px_3()
                    .items_center()
                    .gap_3()
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary.opacity(0.25))
                    .hover(move |row| row.bg(hover_bg))
                    .child(
                        crate::gpui_shell::widgets::device_icon_container(cx)
                            .id(SharedString::from(format!("ssh-host-icon-{ix}")))
                            .debug_selector(move || format!("ssh-host-icon-{ix}"))
                            .child(host_icon(os_icon.glyph, symbol_family, muted)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .justify_center()
                            .gap(px(3.0))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .items_center()
                                    .text_size(px(title_h))
                                    .line_height(px(title_h * 1.25))
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(label),
                                    )
                                    .when(!group.is_empty(), |line| {
                                        line.child(
                                            div()
                                                .max_w(px(120.0))
                                                .truncate()
                                                .px(px(5.0))
                                                .rounded_sm()
                                                .text_xs()
                                                .text_color(muted)
                                                .bg(theme.secondary)
                                                .child(group),
                                        )
                                    })
                                    .when(from_config, |line| {
                                        line.child(
                                            div()
                                                .flex_shrink_0()
                                                .px(px(5.0))
                                                .rounded_sm()
                                                .text_size(px(10.0))
                                                .text_color(muted)
                                                .border_1()
                                                .border_color(theme.border)
                                                .child("ssh-config"),
                                        )
                                    }),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_size(px(subtitle_h))
                                            .line_height(px(subtitle_h * 1.25))
                                            .text_color(muted)
                                            .truncate()
                                            .child(host.clone()),
                                    )
                                    .child(div().text_color(muted).child("·"))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(subtitle_h))
                                            .text_color(muted)
                                            .child(auth),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                Button::new(SharedString::from(format!("ssh-connect-{ix}")))
                                    .debug_selector(move || format!("ssh-connect-{ix}"))
                                    .label(language.text(Message::HostsConnect))
                                    .small()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.emit(SettingsPaneEvent::LaunchSsh(connect_host.clone()));
                                        this.ssh_status =
                                            Some(SshStatus::Opening(connect_host.clone()));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("ssh-edit-{ix}")))
                                    .debug_selector(move || format!("ssh-edit-{ix}"))
                                    .icon(IconName::Settings2)
                                    .ghost()
                                    .small()
                                    .size(px(32.0))
                                    .tooltip(language.pick("编辑主机", "Edit host"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_ssh_editor(Some(edit_host.clone()), window, cx);
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("ssh-pin-{ix}")))
                                    .debug_selector(move || format!("ssh-pin-{ix}"))
                                    .icon(Icon::default().path(crate::gpui_shell::assets::nav::PIN))
                                    .ghost()
                                    .small()
                                    .size(px(32.0))
                                    .selected(pinned)
                                    .toggled(pinned)
                                    .tooltip(if pinned {
                                        language.pick("取消置顶", "Unpin")
                                    } else {
                                        language.pick("置顶", "Pin")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.ssh_apply(
                                            |lists| lists.toggle_pin(&pin_host),
                                            SshStatus::Pinned,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("ssh-delete-{ix}")))
                                    .debug_selector(move || format!("ssh-delete-{ix}"))
                                    .map(|button| {
                                        if confirm {
                                            button
                                                .label(language.pick("确认删除", "Confirm delete"))
                                                .danger()
                                                .small()
                                        } else {
                                            button
                                                .icon(
                                                    Icon::default().path(
                                                        crate::gpui_shell::assets::nav::TRASH,
                                                    ),
                                                )
                                                .ghost()
                                                .small()
                                                .size(px(32.0))
                                                .tooltip(if from_config {
                                                    language.tr("settings.ssh.hide_config_host")
                                                } else {
                                                    language.pick("删除", "Delete")
                                                })
                                        }
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.ssh_delete_confirm.as_deref()
                                            == Some(delete_host.as_str())
                                        {
                                            this.delete_ssh_host(&delete_host, cx);
                                        } else {
                                            this.ssh_delete_confirm = Some(delete_host.clone());
                                            cx.notify();
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::gpui_shell) fn section_ssh(&mut self, cx: &mut Context<Self>) -> gpui::Div {
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
        .h(px(HOST_CARD_EXTENT * host_count.clamp(1, 8) as f32));

        let hidden_rows = self.ssh_show_hidden.then(|| {
            hidden
                .iter()
                .enumerate()
                .map(|(ix, host)| {
                    let restore_host = host.clone();
                    h_flex()
                        .h(px(32.0))
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

        let hidden_count = self.ssh_hosts.hidden_hosts().len();
        let undo_bar = self.ssh_delete_undo.as_ref().map(|undo| (undo.host.clone(), undo.seq));

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(config_banner)
            .child(controls)
            .children(
                self.ssh_hosts
                    .load_error
                    .as_ref()
                    .map(|error| div().text_sm().text_color(theme.danger).child(error.clone())),
            )
            .child(v_flex().w_full().when(host_count > 0, |card| card.child(host_rows)).when(
                host_count == 0,
                |card| {
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
                },
            ))
            .when(hidden_count > 0, |group| {
                let show = self.ssh_show_hidden;
                group.child(
                    NebulaButton::new("ssh-toggle-hidden")
                        .label(if show {
                            SharedString::from(language.pick("收起已隐藏", "Collapse hidden"))
                        } else {
                            SharedString::from(format!(
                                "{} {hidden_count}",
                                language.pick("已隐藏", "Hidden")
                            ))
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ssh_show_hidden = !this.ssh_show_hidden;
                            cx.notify();
                        })),
                )
            })
            .when_some(hidden_rows, |group, rows| {
                group.child(div().h(px(8.0))).child(
                    v_flex()
                        .w_full()
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .children(rows),
                )
            })
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
            return name.to_string_lossy().into_owned();
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
