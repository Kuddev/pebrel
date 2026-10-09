//! Compact resource tables use the same sidebar theme roles and control semantics.
use super::*;
use crate::gpui_shell::prelude::*;
use crate::i18n::{Message, UiLanguage};
use crate::resource_monitor::{Process, bytes};
use gpui::{
    AppContext as _, Div, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

impl ResourceMonitor {
    /// Compose host selection, polling status and the selected category from immutable samples.
    pub(super) fn render_panel(&self, cx: &mut Context<'_, Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let status = self.collection_status();
        let mut panel = v_flex()
            .size_full()
            .min_h_0()
            .px(px(10.0))
            .gap(px(8.0))
            .text_size(px(12.0))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("monitor-follow")
                            .ghost()
                            .small()
                            .selected(!self.local_only)
                            .label(language.text(Message::MonitorFollow))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.local_only = false;
                                view.bind(view.focused.clone(), view.visible, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("monitor-local")
                            .ghost()
                            .small()
                            .selected(self.local_only)
                            .label(language.text(Message::MonitorLocal))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.local_only = true;
                                view.bind(view.focused.clone(), view.visible, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().child(self.scope.label.clone()))
                    .child(
                        Button::new("monitor-pause")
                            .ghost()
                            .small()
                            .selected(self.paused)
                            .label(language.text(if self.paused {
                                Message::MonitorResume
                            } else {
                                Message::MonitorPause
                            }))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.paused = !view.paused;
                                view.stop_polling();
                                view.start_polling(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_size(px(10.0))
                    .text_color(muted)
                    .child(div().flex_1().min_w_0().child(language.text(status)))
                    .child(
                        div().flex_shrink_0().child(
                            self.snapshot
                                .as_ref()
                                .map(|snapshot| {
                                    language.format(
                                        Message::MonitorAge,
                                        &[(
                                            "seconds",
                                            &snapshot.sampled.elapsed().as_secs().to_string(),
                                        )],
                                    )
                                })
                                .unwrap_or_default(),
                        ),
                    ),
            );
        if let Some(error) = &self.error {
            panel = panel.child(div().text_color(cx.theme().danger).child(error.clone()));
            if matches!(self.scope.target, Some(Target::Ssh(_) | Target::Wsl(_))) {
                panel = panel.child(
                    div().text_color(muted).child(language.text(Message::MonitorGuestRequirements)),
                );
            }
        }
        let mut tabs =
            h_flex().w_full().gap_1().pb_2().border_b_1().border_color(cx.theme().border);
        for (index, category, label) in [
            (0usize, Category::Overview, Message::MonitorOverview),
            (1, Category::Docker, Message::MonitorDocker),
        ] {
            tabs = tabs.child(
                Button::new(("resource-category", index))
                    .ghost()
                    .small()
                    .flex_1()
                    .selected(self.category == category)
                    .label(language.text(label))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.category = category;
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(tabs);
        let body = match self.category {
            Category::Overview => self.render_overview(language, cx),
            Category::Docker => self.render_docker(language, cx),
        };
        panel
            .child(
                div()
                    .id("resource-monitor-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    /// Render compact container rows; port bindings remain hidden until explicitly expanded.
    fn render_docker(&self, language: UiLanguage, cx: &mut Context<'_, Self>) -> gpui::AnyElement {
        let mut body = card(cx);
        let Some(snapshot) = &self.snapshot else { return body.into_any_element() };
        let docker = match &snapshot.docker {
            Probe::Ready(docker) => docker,
            Probe::Skipped => {
                return div().child(language.text(Message::MonitorLoading)).into_any_element();
            },
            Probe::Unavailable => {
                return div()
                    .child(language.text(Message::MonitorDockerUnavailable))
                    .into_any_element();
            },
            Probe::Failed(error) => {
                return v_flex()
                    .gap_2()
                    .child(language.text(Message::MonitorDockerFailed))
                    .child(error.clone())
                    .into_any_element();
            },
        };
        body = body
            .child(div().text_color(cx.theme().muted_foreground).child(docker.endpoint.clone()));
        if let Some(error) = &docker.stats_error {
            body = body.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        if docker.containers.is_empty() {
            return body.child(language.text(Message::MonitorNoContainers)).into_any_element();
        }
        body = body.child(table_header(
            language.text(Message::MonitorContainer),
            language.text(Message::MonitorCpu),
            language.text(Message::MonitorMemory),
            cx,
        ));
        for container in &docker.containers {
            let id = container.id.clone();
            let expanded = self.expanded.contains(&id);
            let name = container.name.clone();
            let status = language.text(if container.running {
                Message::MonitorRunning
            } else {
                Message::MonitorStopped
            });
            let row = h_flex()
                .w_full()
                .gap_1()
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(12.0)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .items_start()
                        .child(div().w_full().truncate().child(name))
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} · {}", status, container.status)),
                        ),
                )
                .child(number_cell(percent(container.cpu), 48.0))
                .child(number_cell(optional_bytes(container.memory), 68.0));
            body = body.child(
                Button::new(SharedString::from(format!("monitor-container-{id}")))
                    .ghost()
                    .w_full()
                    .h(px(48.0))
                    .px_1()
                    .selected(expanded)
                    .tooltip(language.text(if expanded {
                        Message::MonitorHidePorts
                    } else {
                        Message::MonitorShowPorts
                    }))
                    .child(row)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if !view.expanded.remove(&id) {
                            view.expanded.insert(id.clone());
                        }
                        cx.notify();
                    })),
            );
            if expanded {
                let mut ports = v_flex()
                    .pl_4()
                    .pb_3()
                    .gap_1()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground);
                if container.ports.is_empty() {
                    ports = ports.child(language.text(Message::MonitorNoPorts));
                }
                for port in &container.ports {
                    let text = match (&port.address, &port.host) {
                        (Some(address), Some(host)) => format!(
                            "{}:{} → {}",
                            if address.contains(':') {
                                format!("[{address}]")
                            } else {
                                address.clone()
                            },
                            host,
                            port.container
                        ),
                        _ => format!(
                            "{} · {}",
                            port.container,
                            language.text(Message::MonitorUnpublished)
                        ),
                    };
                    ports = ports.child(div().child(text));
                }
                if !container.running && !container.ports.is_empty() {
                    ports = ports.child(language.text(Message::MonitorInactivePorts));
                }
                body = body.child(ports);
            }
        }
        body.child(
            div()
                .mt_3()
                .text_color(cx.theme().muted_foreground)
                .child(language.text(Message::MonitorPortMeaning)),
        )
        .into_any_element()
    }
}

/// Keep table values in fixed columns while allowing the process name to shrink.
pub(super) fn process_row(process: &Process, ranking: Ranking, cx: &gpui::App) -> Div {
    let value = match ranking {
        Ranking::Cpu | Ranking::Memory => bytes(process.memory),
        Ranking::Io => optional_rate(process.io_rate),
        Ranking::Vram => optional_bytes(process.vram),
    };
    h_flex()
        .w_full()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            v_flex().flex_1().min_w_0().child(div().truncate().child(process.name.clone())).child(
                div()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(process.pid.to_string()),
            ),
        )
        .child(number_cell(percent(process.cpu), 48.0))
        .child(number_cell(value, 68.0))
}

/// Provide consistent numeric-column alignment for process and container tables.
pub(super) fn table_header(
    name: &'static str,
    cpu: &'static str,
    memory: &'static str,
    cx: &gpui::App,
) -> Div {
    h_flex()
        .w_full()
        .py_2()
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_1().min_w_0().child(name))
        .child(number_cell(cpu.into(), 48.0))
        .child(number_cell(memory.into(), 68.0))
}

/// Render a noninteractive aligned numeric table cell.
fn number_cell(value: String, width: f32) -> Div {
    div().w(px(width)).flex_shrink_0().text_right().child(value)
}

/// Format missing counters independently from genuine zero utilization.
pub(super) fn percent(value: Option<f32>) -> String {
    value.map(|n| format!("{n:.1}%")).unwrap_or_else(|| "—".into())
}

/// Format missing memory counters independently from measured zero bytes.
pub(super) fn optional_bytes(value: Option<u64>) -> String {
    value.map(bytes).unwrap_or_else(|| "—".into())
}

/// Format optional transfer rates consistently across traffic graphs and process tables.
pub(super) fn optional_rate(value: Option<f64>) -> String {
    value.map(|rate| format!("{}/s", bytes(rate as u64))).unwrap_or_else(|| "—".into())
}

/// Resource groups share the application's semantic surface, border and compact spacing.
pub(super) fn card(cx: &gpui::App) -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .p_3()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
}

/// Keep headings and current measurements aligned even in a narrow sidebar.
pub(super) fn card_heading(label: &str, value: String, cx: &gpui::App) -> Div {
    h_flex()
        .gap_2()
        .child(div().flex_1().min_w_0().truncate().font_semibold().child(label.to_owned()))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(cx.theme().muted_foreground)
                .child(value),
        )
}

/// Clamp percentage tracks without treating an unavailable counter as measured zero.
pub(super) fn progress(usage: Option<f32>, cx: &gpui::App) -> Div {
    let mut track = div().w_full().h(px(4.0)).rounded_sm().overflow_hidden().bg(cx.theme().border);
    if let Some(usage) = usage {
        track = track.child(
            div()
                .h_full()
                .w(gpui::relative((usage / 100.0).clamp(0.0, 1.0)))
                .bg(cx.theme().primary),
        );
    }
    track
}
