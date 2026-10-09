//! Read-only process inspection shares sidebar samples and owns only UI subscriptions.
use super::presentation::{optional_bytes, optional_rate, percent};
use super::{Ranking, ResourceMonitor};
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;
use crate::resource_monitor::{Probe, Process, bytes};
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, UniformListScrollHandle, Window, div, px,
};
use std::rc::Rc;

/// A host-pinned process table; closing the dialog drops subscriptions without starting probes.
pub(super) struct ProcessDialog {
    /// Host identity at opening, used to reject samples from another terminal host.
    scope: String,
    /// Modal-owned demand token controls detailed queries in the existing sidebar worker.
    details: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Host label shown independently of search results.
    label: String,
    /// Last accepted process sample; retained during pause, failure or host change.
    processes: Rc<Vec<Process>>,
    /// Search input matches the process name and decimal PID.
    search: Entity<InputState>,
    /// Descending resource ordering, with PID tie-breaking and absent counters last.
    ranking: Ranking,
    /// Whether the user temporarily froze updates in this dialog.
    paused: bool,
    /// True after the sidebar changes host; the dialog remains pinned to its original sample.
    scope_changed: bool,
    /// Sidebar collection state associated with the displayed sample.
    status: Message,
    /// Whether supported GPU telemetry enables VRAM ranking.
    has_gpu: bool,
    /// Shared virtual list viewport, reset when search or ordering changes.
    scroll: UniformListScrollHandle,
    /// Observer and input subscriptions are released with the modal entity.
    _subscriptions: Vec<Subscription>,
}

impl ProcessDialog {
    /// Observe the selected host's existing collector and create a searchable, read-only view.
    /// The existing sidebar worker includes details while the modal demand token is active.
    pub(super) fn new(
        monitor: &Entity<ResourceMonitor>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let language = crate::gpui_shell::config::ui_language(cx);
        let view = monitor.read(cx);
        let scope = view.scope.key.clone();
        let label = view.scope.label.clone();
        let processes = Rc::new(
            view.snapshot
                .as_ref()
                .filter(|s| s.processes_complete)
                .map(|s| s.processes.clone())
                .unwrap_or_default(),
        );
        let has_gpu = view
            .snapshot
            .as_ref()
            .is_some_and(|s| matches!(&s.gpus, Probe::Ready(gpus) if !gpus.is_empty()));
        let status = Message::MonitorLoading;
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(Message::MonitorSearchProcesses))
        });

        let details = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        monitor.update(cx, |view, _| {
            view.details.store(false, std::sync::atomic::Ordering::Relaxed);
            view.details = details.clone();
        });
        // Host changes permanently freeze this modal, so returning to a host cannot mix samples.
        let observer = cx.observe(monitor, |this, monitor, cx| {
            let view = monitor.read(cx);
            if view.scope.key != this.scope || !std::sync::Arc::ptr_eq(&view.details, &this.details)
            {
                this.scope_changed = true;
                this.details.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            if !this.scope_changed && !this.paused {
                this.status = view.collection_status();
                if let Some(snapshot) = &view.snapshot {
                    if snapshot.processes_complete {
                        this.processes = Rc::new(snapshot.processes.clone());
                    } else if matches!(this.status, Message::MonitorLive) {
                        this.status = Message::MonitorLoading;
                    }
                    this.has_gpu = matches!(&snapshot.gpus, Probe::Ready(gpus) if !gpus.is_empty());
                }
            }
            cx.notify();
        });
        let input = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                cx.notify();
            }
        });
        Self {
            scope,
            details,
            label,
            processes,
            search,
            ranking: Ranking::Cpu,
            paused: false,
            scope_changed: false,
            status,
            has_gpu,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![observer, input],
        }
    }
}

impl Drop for ProcessDialog {
    /// Release detail demand before the dialog's subscriptions are destroyed.
    fn drop(&mut self) {
        self.details.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Render for ProcessDialog {
    /// Render only visible rows while preserving every collected process in search and ranking.
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let query = self.search.read(cx).value().to_lowercase();
        let mut rows = self
            .processes
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.pid.to_string().contains(&query)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| self.processes[*a].compare_usage(&self.processes[*b], self.ranking));
        let count = rows.len();
        let wide = f32::from(window.viewport_size().width) >= 760.0;
        let height = (f32::from(window.viewport_size().height) - 280.0).clamp(100.0, 500.0);
        let mut controls = h_flex().flex_wrap().gap_1();
        for (index, ranking, label) in [
            (0usize, Ranking::Cpu, Message::MonitorCpu),
            (1, Ranking::Memory, Message::MonitorMemory),
            (2, Ranking::Io, Message::MonitorIo),
            (3, Ranking::Vram, Message::MonitorVram),
        ] {
            if ranking == Ranking::Vram && !self.has_gpu {
                continue;
            }
            controls = controls.child(
                Button::new(("monitor-process-sort", index))
                    .ghost()
                    .small()
                    .selected(self.ranking == ranking)
                    .label(language.text(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ranking = ranking;
                        this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                        cx.notify();
                    })),
            );
        }
        let ranking = self.ranking;
        let processes = self.processes.clone();
        let list = gpui::uniform_list(
            "monitor-process-list",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| process_table_row(&processes[rows[index]], wide, ranking, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .w_full()
        .h(px(height))
        .track_scroll(&self.scroll);
        let state = if self.scope_changed {
            Message::MonitorScopeChanged
        } else if self.paused {
            Message::MonitorPaused
        } else {
            self.status
        };
        let extra_label =
            if self.ranking == Ranking::Vram { Message::MonitorVram } else { Message::MonitorIo };
        let mut header = h_flex()
            .w_full()
            .h(px(32.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().min_w_0().child(language.text(Message::MonitorProcessName)))
            .child(table_cell("PID".into(), if wide { 72.0 } else { 48.0 }))
            .child(table_cell(language.text(Message::MonitorCpu).into(), 60.0))
            .child(table_cell(
                language
                    .text(if !wide && matches!(self.ranking, Ranking::Io | Ranking::Vram) {
                        extra_label
                    } else {
                        Message::MonitorMemory
                    })
                    .into(),
                78.0,
            ));
        if wide {
            header = header.child(table_cell(language.text(extra_label).into(), 96.0));
        }
        let mut body = v_flex()
            .max_h(px((f32::from(window.viewport_size().height) - 160.0).max(100.0)))
            .overflow_y_scrollbar()
            .w_full()
            .min_w_0()
            .gap_3()
            .text_size(px(12.0))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().child(self.label.clone()))
                    .child(
                        Button::new("monitor-dialog-pause")
                            .ghost()
                            .small()
                            .disabled(self.scope_changed)
                            .label(language.text(if self.paused {
                                Message::MonitorResume
                            } else {
                                Message::MonitorPause
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.paused = !this.paused;
                                this.details
                                    .store(!this.paused, std::sync::atomic::Ordering::Relaxed);
                                if !this.paused {
                                    this.status = Message::MonitorLoading;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(Input::new(&self.search).w_full())
            .child(controls)
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(state)),
            );
        let mut table = v_flex().min_w(px(400.0)).child(header);
        if count == 0 {
            table = table.child(
                div().h(px(height)).py_3().text_color(cx.theme().muted_foreground).child(
                    language.text(if matches!(state, Message::MonitorLoading) {
                        Message::MonitorLoading
                    } else {
                        Message::MonitorNoProcesses
                    }),
                ),
            );
        } else {
            table = table.child(
                div()
                    .relative()
                    .w_full()
                    .h(px(height))
                    .child(list)
                    .vertical_scrollbar(&self.scroll),
            );
        }
        body = body.child(div().w_full().overflow_x_scrollbar().child(table));
        body = body.child(div().text_size(px(11.0)).text_color(cx.theme().muted_foreground).child(
            language.format(
                Message::MonitorProcessCount,
                &[("shown", &count.to_string()), ("total", &self.processes.len().to_string())],
            ),
        ));
        body = body.child(
            div()
                .text_size(px(11.0))
                .text_color(cx.theme().muted_foreground)
                .child(language.text(Message::MonitorCpuScale)),
        );
        if self.ranking == Ranking::Io {
            body = body.child(
                div()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::MonitorIoCoverage)),
            );
        } else if self.ranking == Ranking::Vram {
            body = body.child(
                div()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(Message::MonitorGpuCoverage)),
            );
        }
        body
    }
}

/// Align live process measurements without exposing mutating process operations.
fn process_table_row(process: &Process, wide: bool, ranking: Ranking, cx: &App) -> gpui::Div {
    let extra = if ranking == Ranking::Vram {
        optional_bytes(process.vram)
    } else {
        optional_rate(process.io_rate)
    };
    let mut row = h_flex()
        .w_full()
        .h(px(36.0))
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().flex_1().min_w_0().truncate().child(process.name.clone()))
        .child(table_cell(process.pid.to_string(), if wide { 72.0 } else { 48.0 }))
        .child(table_cell(percent(process.cpu), 60.0))
        .child(table_cell(
            if !wide && matches!(ranking, Ranking::Io | Ranking::Vram) {
                extra.clone()
            } else {
                bytes(process.memory)
            },
            78.0,
        ));
    if wide {
        row = row.child(table_cell(extra, 96.0));
    }
    row
}

/// Numeric columns remain stable as names shrink and measurements refresh.
fn table_cell(value: String, width: f32) -> gpui::Div {
    div().w(px(width)).flex_shrink_0().text_right().pr_2().child(value)
}
