//! Disk overview selects useful mounts while the full inventory remains inspectable.
use super::ResourceMonitor;
use super::overview::section_heading;
use super::presentation::{card, card_heading, progress};
use crate::gpui_shell::prelude::*;
use crate::i18n::{Message, UiLanguage};
use crate::resource_monitor::{Disk, Probe, bytes, filesystems};
use gpui::{
    App, AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Styled as _, Window, div, px,
};

impl ResourceMonitor {
    /// Show the system volume, optionally expanded data mounts and a complete read-only snapshot.
    pub(super) fn render_disks(
        &self,
        disks: &Probe<Vec<Disk>>,
        language: UiLanguage,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut body = card(cx).child(section_heading(
            IconName::HardDrive,
            language.text(Message::MonitorDiskSpace),
            String::new(),
            cx,
        ));
        match disks {
            Probe::Ready(items) => {
                body = body.child(
                    h_flex().justify_end().child(
                        Button::new("monitor-all-disks")
                            .ghost()
                            .small()
                            .label(language.text(Message::MonitorViewAll))
                            .on_click(cx.listener(|view, _, window, cx| {
                                let Some(crate::resource_monitor::Snapshot {
                                    disks: Probe::Ready(items),
                                    ..
                                }) = &view.snapshot
                                else {
                                    return;
                                };
                                let items = items.clone();
                                let host = view.scope.label.clone();
                                window.defer(cx, move |window, cx| {
                                    let dialog = cx.new(|_| DiskDialog { items, host });
                                    window.open_dialog(cx, move |modal, window, cx| {
                                        let language = crate::gpui_shell::config::ui_language(cx);
                                        modal
                                            .title(language.text(Message::MonitorAllDisks))
                                            .width(px((f32::from(window.viewport_size().width)
                                                - 32.0)
                                                .clamp(240.0, 960.0)))
                                            .margin_top(px(32.0))
                                            .max_h(window.viewport_size().height - px(64.0))
                                            .content({
                                                let dialog = dialog.clone();
                                                move |content, _, _| content.child(dialog.clone())
                                            })
                                    });
                                });
                            })),
                    ),
                );
                if let Some(system) = filesystems::primary(items) {
                    body = body.child(disk_summary(system, language, cx));
                } else {
                    body = body.child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text(Message::MonitorMetricUnavailable)),
                    );
                }
                let other = filesystems::other(items);
                if !other.is_empty() {
                    body = body.child(
                        Button::new("monitor-other-disks")
                            .ghost()
                            .small()
                            .icon(if self.other_disks_expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .label(language.format(
                                Message::MonitorOtherDisks,
                                &[("count", &other.len().to_string())],
                            ))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.other_disks_expanded = !view.other_disks_expanded;
                                cx.notify();
                            })),
                    );
                    if self.other_disks_expanded {
                        for disk in other {
                            body = body.child(disk_summary(disk, language, cx));
                        }
                    }
                }
            },
            Probe::Failed(error) => {
                body = body.child(div().text_color(cx.theme().danger).child(error.clone()))
            },
            _ => body = body.child("—"),
        }
        body.into_any_element()
    }
}

/// Present one filesystem's capacity without duplicating unrelated mount rows.
fn disk_summary(disk: &Disk, language: UiLanguage, cx: &App) -> Div {
    let used = disk.total.saturating_sub(disk.available);
    let usage = 100.0 * used as f32 / disk.total.max(1) as f32;
    v_flex()
        .gap_2()
        .py_2()
        .child(card_heading(&disk.mount, format!("{} / {}", bytes(used), bytes(disk.total)), cx))
        .child(progress(Some(usage), cx))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(cx.theme().muted_foreground)
                .child(disk.filesystem.clone().unwrap_or_else(|| "—".into())),
        )
        .child(div().w_full().overflow_x_scrollbar().child(disk_table(
            &[disk],
            language,
            false,
            cx,
        )))
}

/// A full mount snapshot is pinned to its opening host, independent of subsequent pane selection.
struct DiskDialog {
    /// Complete inventory, including mounts suppressed in the compact overview.
    items: Vec<Disk>,
    /// Captured host label identifies the immutable sample.
    host: String,
}

impl Render for DiskDialog {
    /// Render all mount capacities and sources using the active appearance and language.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let items = self.items.iter().collect::<Vec<_>>();
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.format(Message::MonitorDiskSnapshot, &[("host", &self.host)])),
            )
            .child(
                div()
                    .id("monitor-disk-inventory")
                    .max_h((window.viewport_size().height - px(180.0)).max(px(80.0)))
                    .overflow_scrollbar()
                    .child(disk_table(&items, language, true, cx)),
            )
    }
}

/// Capacity columns are shared by the compact card and the complete mount inspector.
fn disk_table(items: &[&Disk], language: UiLanguage, detailed: bool, cx: &App) -> Div {
    let mut heading = h_flex()
        .py_2()
        .text_size(px(10.0))
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_1().min_w(px(70.0)).child(language.text(Message::MonitorMount)))
        .child(div().w(px(70.0)).text_right().child(language.text(Message::MonitorTotal)))
        .child(div().w(px(70.0)).text_right().child(language.text(Message::MonitorAvailable)))
        .child(div().w(px(42.0)).text_right().child("%"));
    if detailed {
        heading = heading
            .child(div().w(px(100.0)).pl_2().child(language.text(Message::MonitorFilesystem)))
            .child(div().w(px(150.0)).pl_2().child(language.text(Message::MonitorDiskSource)));
    }
    let mut table = v_flex().min_w(px(if detailed { 580.0 } else { 260.0 })).child(heading);
    for disk in items {
        let usage =
            100.0 * disk.total.saturating_sub(disk.available) as f32 / disk.total.max(1) as f32;
        let mut row = h_flex()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_size(px(11.0))
            .child(div().flex_1().min_w(px(70.0)).child(disk.mount.clone()))
            .child(div().w(px(70.0)).text_right().child(bytes(disk.total)))
            .child(div().w(px(70.0)).text_right().child(bytes(disk.available)))
            .child(div().w(px(42.0)).text_right().child(format!("{usage:.0}%")));
        if detailed {
            row = row
                .child(
                    div()
                        .w(px(100.0))
                        .pl_2()
                        .child(disk.filesystem.clone().unwrap_or_else(|| "—".into())),
                )
                .child(
                    div()
                        .w(px(150.0))
                        .pl_2()
                        .child(disk.source.clone().unwrap_or_else(|| "—".into())),
                );
        }
        table = table.child(row);
    }
    table
}
