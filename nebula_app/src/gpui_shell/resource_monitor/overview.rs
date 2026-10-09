//! Overview contains compact resource cards; detailed process inspection is explicitly requested.
use super::charts::{ChartPoint, ChartSeries, MemorySlice, core_track, legend, memory_ring, trend};
use super::presentation::{
    card, card_heading, optional_bytes, optional_rate, percent, process_row, progress, table_header,
};
use super::processes::ProcessDialog;
use super::*;
use crate::gpui_shell::prelude::*;
use crate::i18n::{Message, UiLanguage};
use crate::resource_monitor::bytes;
use gpui::{AppContext as _, Div, ParentElement as _, Styled as _, div, px};

/// CPU series select a named component of each common sampling interval.
enum CpuComponent {
    /// Combined whole-host utilization.
    Total,
    /// Time executing user-mode work.
    User,
    /// Time executing kernel and interrupt work.
    System,
}

impl ResourceMonitor {
    /// Compose real host summaries with theme-aware charts and a five-process preview.
    pub(super) fn render_overview(
        &self,
        language: UiLanguage,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else { return div().into_any_element() };
        let summary = &snapshot.summary;
        let mut body = v_flex().gap_3().pb_3();
        let uptime = summary
            .uptime
            .map(|seconds| {
                language.format(
                    Message::MonitorUptimeValue,
                    &[
                        ("days", &(seconds / 86400).to_string()),
                        ("hours", &((seconds % 86400) / 3600).to_string()),
                    ],
                )
            })
            .unwrap_or_else(|| "—".into());
        body = body.child(
            card(cx)
                .child(section_heading(
                    IconName::Info,
                    language.text(Message::MonitorSystem),
                    self.scope.label.clone(),
                    cx,
                ))
                .child(div().font_semibold().child(format!("{} {}", summary.name, summary.version)))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(cx.theme().muted_foreground)
                        .child(snapshot.platform.clone()),
                )
                .child(card_heading(language.text(Message::MonitorUptime), uptime, cx)),
        );
        let cpu_series = [
            (Message::MonitorTotal, cx.theme().chart_3, CpuComponent::Total),
            (Message::MonitorCpuUser, cx.theme().chart_1, CpuComponent::User),
            (Message::MonitorCpuSystem, cx.theme().chart_2, CpuComponent::System),
        ]
        .into_iter()
        .map(|(label, color, component)| ChartSeries {
            color,
            label: language.text(label).into(),
            bytes_per_second: false,
            points: self
                .history
                .iter()
                .map(|sample| ChartPoint {
                    sampled: sample.sampled,
                    value: match component {
                        CpuComponent::Total => sample.total,
                        CpuComponent::User => sample.user,
                        CpuComponent::System => sample.system,
                    },
                })
                .collect(),
        })
        .collect();
        let mut cpu = card(cx)
            .child(section_heading(
                IconName::Cpu,
                language.text(Message::MonitorCpu),
                percent(snapshot.cpu),
                cx,
            ))
            .child(trend(&self.cpu_chart, cpu_series, 100.0, cx))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::MonitorTrend)),
            );
        if summary.cpu_cores.is_empty() {
            cpu = cpu.child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(24.0)).child("CPU"))
                    .child(div().flex_1().min_w_0().child(core_track(snapshot.cpu, cx)))
                    .child(div().w(px(44.0)).text_right().child(percent(snapshot.cpu))),
            );
        } else {
            for (index, usage) in summary.cpu_cores.iter().enumerate() {
                cpu = cpu.child(
                    h_flex()
                        .gap_2()
                        .h(px(20.0))
                        .child(
                            div()
                                .w(px(24.0))
                                .text_color(cx.theme().muted_foreground)
                                .child(index.to_string()),
                        )
                        .child(div().flex_1().min_w_0().child(core_track(*usage, cx)))
                        .child(div().w(px(44.0)).text_right().child(percent(*usage))),
                );
            }
        }
        body = body.child(cpu);
        let mut slices = vec![MemorySlice { bytes: snapshot.memory, color: cx.theme().chart_1 }];
        let mut categories = h_flex().flex_1().min_w_0().flex_wrap().gap_2().child(legend(
            language.text(Message::MonitorUsed),
            bytes(snapshot.memory),
            cx.theme().chart_1,
            cx,
        ));
        if let Some(cache) = summary.cache {
            slices.push(MemorySlice { bytes: cache, color: cx.theme().chart_2 });
            categories = categories.child(legend(
                language.text(Message::MonitorCache),
                bytes(cache),
                cx.theme().chart_2,
                cx,
            ));
        }
        slices.push(MemorySlice { bytes: summary.free, color: cx.theme().chart_3 });
        categories = categories.child(legend(
            language.text(if summary.cache.is_some() {
                Message::MonitorFree
            } else {
                Message::MonitorAvailable
            }),
            bytes(summary.free),
            cx.theme().chart_3,
            cx,
        ));
        body = body.child(
            card(cx)
                .child(section_heading(
                    IconName::MemoryStick,
                    language.text(Message::MonitorMemory),
                    bytes(snapshot.total_memory),
                    cx,
                ))
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(memory_ring(slices, snapshot.total_memory, cx))
                        .child(categories),
                ),
        );
        let mut network = card(cx).child(section_heading(
            IconName::Network,
            language.text(Message::MonitorNetwork),
            String::new(),
            cx,
        ));
        if let Some(traffic) = &summary.network {
            let maximum = self
                .network_history
                .iter()
                .flat_map(|sample| [sample.upload, sample.download])
                .flatten()
                .fold(1.0_f64, f64::max) as f32;
            let upload = ChartSeries {
                color: cx.theme().chart_1,
                label: language.text(Message::MonitorUpload).into(),
                bytes_per_second: true,
                points: self
                    .network_history
                    .iter()
                    .map(|sample| ChartPoint {
                        sampled: sample.sampled,
                        value: sample.upload.map(|rate| rate as f32),
                    })
                    .collect(),
            };
            let download = ChartSeries {
                color: cx.theme().chart_2,
                label: language.text(Message::MonitorDownload).into(),
                bytes_per_second: true,
                points: self
                    .network_history
                    .iter()
                    .map(|sample| ChartPoint {
                        sampled: sample.sampled,
                        value: sample.download.map(|rate| rate as f32),
                    })
                    .collect(),
            };
            network = network
                .child(trend(&self.network_chart, vec![upload, download], maximum, cx))
                .child(
                    h_flex()
                        .w_full()
                        .text_size(px(10.0))
                        .text_color(cx.theme().muted_foreground)
                        .child(div().flex_1())
                        .child(
                            div()
                                .w(px(68.0))
                                .text_right()
                                .child(language.text(Message::MonitorSpeed)),
                        )
                        .child(
                            div()
                                .w(px(68.0))
                                .text_right()
                                .child(language.text(Message::MonitorTraffic)),
                        ),
                )
                .child(traffic_row(
                    language.text(Message::MonitorUpload),
                    traffic.transmit_rate,
                    traffic.transmitted,
                    cx.theme().chart_1,
                    cx,
                ))
                .child(traffic_row(
                    language.text(Message::MonitorDownload),
                    traffic.receive_rate,
                    traffic.received,
                    cx.theme().chart_2,
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(cx.theme().muted_foreground)
                        .child(traffic.interfaces.join(", ")),
                );
        } else {
            network = network.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::MonitorMetricUnavailable)),
            );
        }
        body = body.child(network);
        if let Probe::Ready(gpus) = &snapshot.gpus {
            for gpu in gpus {
                let vram = gpu
                    .used
                    .zip(gpu.total)
                    .filter(|(_, total)| *total > 0)
                    .map(|(used, total)| 100.0 * used as f32 / total as f32);
                body = body.child(
                    card(cx)
                        .child(section_heading(
                            IconName::Cpu,
                            language.text(Message::MonitorGpu),
                            percent(gpu.usage),
                            cx,
                        ))
                        .child(div().truncate().child(gpu.name.clone()))
                        .child(progress(gpu.usage, cx))
                        .child(card_heading(
                            language.text(Message::MonitorVram),
                            format!("{} / {}", optional_bytes(gpu.used), optional_bytes(gpu.total)),
                            cx,
                        ))
                        .child(progress(vram, cx)),
                );
            }
        } else if let Probe::Failed(error) = &snapshot.gpus {
            body = body.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        body = body.child(self.render_disks(&snapshot.disks, language, cx));
        let mut processes = snapshot.processes.iter().collect::<Vec<_>>();
        processes.sort_by(|a, b| a.compare_usage(b, Ranking::Cpu));
        let mut preview = card(cx)
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_semibold()
                            .child(language.text(Message::MonitorTopFiveProcesses)),
                    )
                    .child(
                        Button::new("monitor-all-processes")
                            .ghost()
                            .small()
                            .label(language.text(Message::MonitorViewAll))
                            .on_click(cx.listener(|_, _, window, cx| {
                                let monitor = cx.entity();
                                // Defer until the monitor listener releases its mutable entity borrow.
                                window.defer(cx, move |window, cx| {
                                    let dialog =
                                        cx.new(|cx| ProcessDialog::new(&monitor, window, cx));
                                    window.open_dialog(cx, move |modal, window, cx| {
                                        let language = crate::gpui_shell::config::ui_language(cx);
                                        let width = (f32::from(window.viewport_size().width)
                                            - 32.0)
                                            .clamp(240.0, 960.0);
                                        modal
                                            .title(language.text(Message::MonitorAllProcesses))
                                            .width(px(width))
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
            )
            .child(table_header(
                language.text(Message::MonitorProcessName),
                language.text(Message::MonitorCpu),
                language.text(Message::MonitorMemory),
                cx,
            ));
        for process in processes.into_iter().take(5) {
            preview = preview.child(process_row(process, Ranking::Cpu, cx));
        }
        if snapshot.processes.is_empty() {
            preview = preview.child(
                div()
                    .py_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::MonitorNoProcesses)),
            );
        }
        body.child(preview).into_any_element()
    }
}

/// Card headers use shared icons, theme roles and a separator matching the sidebar hierarchy.
pub(super) fn section_heading(icon: IconName, title: &str, value: String, cx: &gpui::App) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .pb_2()
        .mb_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(Icon::new(icon).size(px(15.0)).text_color(cx.theme().primary))
        .child(div().flex_1().min_w_0().font_semibold().child(title.to_owned()))
        .child(
            div()
                .min_w_0()
                .max_w(gpui::relative(0.6))
                .truncate()
                .text_size(px(11.0))
                .text_color(cx.theme().muted_foreground)
                .child(value),
        )
}

/// Traffic totals and rates retain units and a matching curve marker in every appearance.
fn traffic_row(
    label: &str,
    rate: Option<f64>,
    total: u64,
    color: gpui::Hsla,
    cx: &gpui::App,
) -> Div {
    h_flex()
        .w_full()
        .py_2()
        .gap_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().w(px(5.0)).h(px(5.0)).rounded_full().bg(color))
        .child(div().flex_1().min_w_0().text_size(px(11.0)).child(label.to_owned()))
        .child(div().w(px(68.0)).text_right().text_size(px(11.0)).child(optional_rate(rate)))
        .child(div().w(px(68.0)).text_right().text_size(px(11.0)).child(bytes(total)))
}
