use std::{path::PathBuf, time::Duration};

use gpui::{
    AnyElement, AppContext as _, Context, FocusHandle, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Window, div, px,
};
use nebula_terminal::term::grid_content::Scrollback;

use super::{NebulaWorkspace, WorkspaceTab};
use crate::display::ToastKind;
use crate::gpui_shell::prelude::*;
use crate::gpui_shell::terminal::view::TerminalView;
use crate::i18n::Message;
use crate::runtime_api::tabs::TabId;
use crate::session::{LaunchSession, TabSession};

#[derive(Default)]
pub(super) struct TabSleepState {
    pending: Option<TabId>,
    timer: Option<gpui::Task<()>>,
    scan_from: usize,
}

impl TabSleepState {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

pub(super) struct SleepingTab {
    pub(super) title: SharedString,
    pub(super) cwd: PathBuf,
    pub(super) focus: FocusHandle,
    launch: LaunchSession,
    grid: (u16, u16),
    history: Option<Scrollback>,
    pane_name: Option<String>,
    pane_name_history: Vec<Option<String>>,
}

impl SleepingTab {
    pub(super) fn session(&self, meta: super::TabMeta) -> TabSession {
        let cwd = self.cwd.to_string_lossy().into_owned();
        TabSession {
            layout: Some(crate::session::LayoutSession::Pane {
                cwd: cwd.clone(),
                agent: None,
                launch: Some(self.launch.clone()),
                custom_name: self.pane_name.clone(),
            }),
            cwd,
            custom_name: meta.custom_name,
            color: meta.color,
            launch: Some(self.launch.clone()),
            active_pane: 0,
        }
    }
}

impl NebulaWorkspace {
    pub(super) fn can_sleep_tab(&self, ix: usize, cx: &gpui::App) -> bool {
        matches!(self.tabs.get(ix), Some(WorkspaceTab::Terminal { panes, .. })
            if matches!(panes.as_slice(), [pane] if pane.view.read(cx).supports_sleep()))
    }

    pub(super) fn sync_automatic_tab_sleep(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.window_role != super::windowing::WindowRole::Regular || idle_timeout(cx).is_none() {
            self.tab_sleep.timer = None;
            return;
        }
        if self.tab_sleep.timer.is_some() {
            return;
        }
        let executor = cx.background_executor().clone();
        self.tab_sleep.timer = Some(cx.spawn_in(window, async move |workspace, cx| {
            loop {
                executor.timer(Duration::from_secs(10)).await;
                if workspace
                    .update_in(cx, |this, window, cx| this.sleep_one_idle_tab(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn sleep_one_idle_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_sleep.pending.is_some() || idle_timeout(cx).is_none() {
            return;
        }
        let len = self.tabs.len();
        for _ in 0..len {
            let ix = self.tab_sleep.scan_from % len;
            self.tab_sleep.scan_from = ix + 1;
            if self.can_sleep_tab(ix, cx)
                && self.tabs[ix]
                    .focused_view()
                    .is_some_and(|view| self.is_automatic_sleep_candidate(ix, view.read(cx), cx))
            {
                self.sleep_tab(self.meta(ix).runtime_id, false, window, cx);
                if self.tab_sleep.pending.is_some() {
                    break;
                }
            }
        }
    }

    fn is_automatic_sleep_candidate(&self, ix: usize, view: &TerminalView, cx: &gpui::App) -> bool {
        ix != self.active && idle_timeout(cx).is_some_and(|timeout| view.has_been_idle_for(timeout))
    }

    pub(super) fn sleep_tab(
        &mut self,
        id: TabId,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tab_meta.iter().position(|meta| meta.runtime_id == id) else { return };
        if self.tab_sleep.pending.is_some() || !self.can_sleep_tab(ix, cx) {
            return;
        }
        let Some(view) = self.tabs[ix].focused_view().cloned() else { return };
        let Some(candidate) = view.read(cx).sleep_candidate() else {
            if manual {
                self.sleep_warning(Message::TabSleepBusy, window, cx);
            }
            return;
        };
        self.tab_sleep.pending = Some(id.clone());
        let work = cx.background_executor().spawn(async move {
            let history = candidate.capture();
            (candidate, history)
        });
        let view = view.downgrade();
        cx.spawn_in(window, async move |workspace, cx| {
            let (candidate, history) = work.await;
            let _ = workspace.update_in(cx, move |workspace, window, cx| {
                workspace.tab_sleep.pending = None;
                let Some(ix) = workspace.tab_meta.iter().position(|meta| meta.runtime_id == id)
                else {
                    return;
                };
                let WorkspaceTab::Terminal { panes, .. } = &workspace.tabs[ix] else { return };
                let [pane] = panes.as_slice() else { return };
                let current = pane.view.read(cx);
                let Some(history) = history.filter(|_| {
                    view.upgrade().is_some_and(|view| view.entity_id() == pane.view.entity_id())
                        && (manual || workspace.is_automatic_sleep_candidate(ix, current, cx))
                        && current.matches_sleep_candidate(&candidate)
                }) else {
                    if manual {
                        workspace.sleep_warning(Message::TabSleepBusy, window, cx);
                    }
                    return;
                };
                let truncated = history.was_truncated();
                let saved = SleepingTab {
                    grid: (current.grid_cols() as u16, current.grid_rows() as u16),
                    title: current.tab_label().into(),
                    cwd: candidate.cwd,
                    focus: cx.focus_handle(),
                    launch: candidate.launch,
                    history: Some(history),
                    pane_name: pane.custom_name.clone(),
                    pane_name_history: pane.name_history.clone(),
                };
                let pane_id = pane.id;
                current.shutdown();
                workspace.tabs[ix] = WorkspaceTab::Sleeping(Box::new(saved));
                workspace.runtime_hub.record_pane_closed(workspace.runtime_window_id, pane_id);
                workspace.pane_bounds.borrow_mut().remove(&pane_id);
                workspace.forget_pane_rename(pane_id);
                workspace.remote_browser.forget(pane_id);
                if truncated && manual {
                    workspace.sleep_warning(Message::TabSleepHistoryTruncated, window, cx);
                }
                if ix == workspace.active {
                    workspace.sync_side_panel_to_active(true, cx);
                    if workspace.tab_menu.is_none() {
                        workspace.focus_active(window, cx);
                    }
                }
                let _ = workspace.publish_runtime_snapshot(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn wake_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(WorkspaceTab::Sleeping(saved)) = self.tabs.get_mut(ix) else { return };
        let launch = Self::terminal_launch_from_session(&saved.launch, Some(saved.cwd.clone()));
        let id = self.next_pane_id;
        self.next_pane_id = self.next_pane_id.saturating_add(1);
        let view = cx.new(|cx| {
            TerminalView::start(id, saved.grid, launch, Some(&mut saved.history), window, cx)
        });
        if view.read(cx).session.is_none() {
            self.sleep_warning(Message::TabSleepWakeFailed, window, cx);
            return;
        }
        let pane = super::TerminalPane {
            id,
            custom_name: saved.pane_name.take(),
            name_history: std::mem::take(&mut saved.pane_name_history),
            _subscription: cx.subscribe_in(&view, window, Self::on_terminal_event),
            view,
        };
        self.tabs[ix] = WorkspaceTab::Terminal {
            panes: vec![pane],
            tree: nebula_split::SplitTree::leaf(id),
            focused: id,
            zoomed: false,
            broadcast: false,
        };
        self.focus_active(window, cx);
        self.sync_side_panel_to_active(true, cx);
        let _ = self.publish_runtime_snapshot(window, cx);
        cx.notify();
    }

    pub(super) fn render_sleeping_tab(
        &self,
        saved: &SleepingTab,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        div()
            .track_focus(&saved.focus)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .child(Icon::new(IconName::Pause).text_color(cx.theme().muted_foreground))
            .child(language.text(Message::TabSleepStatus))
            .child(
                Button::new("wake-tab")
                    .outline()
                    .label(language.text(Message::TabSleepWake))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.wake_tab(this.active, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn sleep_warning(&self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::gpui_shell::config::ui_language(cx);
        crate::gpui_shell::toast::toast(window, cx, ToastKind::Warning, language.text(message));
    }
}

fn idle_timeout(cx: &gpui::App) -> Option<Duration> {
    cx.try_global::<crate::gpui_shell::config::Settings>()?.tab_sleep.timeout()
}
