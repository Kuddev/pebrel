//! Legacy UI adapter. Lifecycle and fallback authority live in ai_hook::lifecycle.

use std::time::Instant;

use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line, Point};

use crate::ai_agents::{AgentKind, AgentStatus, AgentStatusSource};
use crate::ai_hook::{AiHookEvent, AiHookKind};
use crate::display::NebulaPaneState;

use super::WindowContext;

fn project_status(state: &mut NebulaPaneState) {
    let status = state.agent_activity.status();
    state.awaiting_input =
        matches!(status, AgentStatus::Idle | AgentStatus::Done | AgentStatus::Blocked);
    state.needs_attention = status == AgentStatus::Blocked;
    state.finished_unseen = matches!(status, AgentStatus::Done | AgentStatus::Blocked);
    if status == AgentStatus::Working {
        state.command_started.get_or_insert_with(Instant::now);
    }
}

fn clear_foreground(state: &mut NebulaPaneState) {
    state.agent_activity.command_finished();
    state.ai_session = None;
    state.running_program = None;
    state.pending_command_prompt = None;
    state.runtime_submit_barrier = None;
    state.command_started = None;
    project_status(state);
}

impl WindowContext {
    pub fn handle_ai_hook(&mut self, event: &AiHookEvent) -> bool {
        let pane_id = event.pane.unwrap_or_else(|| self.focused_pane_id());
        let Some(idx) = self.pane_index(pane_id) else { return false };
        if let Some(pid) = event.client_pid
            && event.pane == Some(pane_id)
            && crate::process_tree::is_within_tree(pid, self.panes[idx].shell_pid) == Some(false)
        {
            log::warn!("ai_hook: pane={pane_id} rejected foreign client pid={pid}");
            return true;
        }
        if !self.panes[idx].nebula_state.agent_activity.accepts_hook(event) {
            log::debug!("ai_hook: pane={pane_id} ignored hook outside the active lifecycle");
            return true;
        }
        let verdict = crate::ai_hook::accept_for_pane(event, pane_id);
        if !verdict.accepted() {
            log::debug!("ai_hook: pane={pane_id} source={} dropped {verdict:?}", event.source);
            return true;
        }
        let state = &mut self.panes[idx].nebula_state;
        state.agent_activity.apply_hook(event);
        if let Some(id) = event.session_id.as_deref()
            && let Err(error) =
                crate::ai_sessions::record_hook_session(&event.source, id, &state.cwd, None)
        {
            log::warn!("agent session index: could not record {} {id}: {error}", event.source);
        }
        if event.kind == AiHookKind::SessionEnd {
            clear_foreground(state);
        } else {
            state.running_program = Some(event.source.clone());
            if let Some(id) = event.session_id.as_deref() {
                state.ai_session = Some(crate::display::AiSessionIdentity {
                    source: event.source.clone(),
                    session_id: id.to_owned(),
                });
            }
            project_status(state);
        }
        let status = state.agent_activity.status();
        let message = event
            .attention
            .as_ref()
            .map(|context| context.summary_for_pane(pane_id))
            .or_else(|| event.message.clone());
        if matches!(event.kind, AiHookKind::TurnDone | AiHookKind::NeedsAttention)
            && let Some(notification) = crate::notify::Notification::from_ai_hook(
                event,
                message,
                status == AgentStatus::Blocked,
            )
        {
            let mut background_tab = false;
            for (i, tab) in self.tabs.iter_mut().enumerate() {
                let mut ids = Vec::new();
                tab.layout.leaves(&mut ids);
                if ids.contains(&pane_id) {
                    if i != self.active_tab {
                        tab.has_bell = true;
                        background_tab = true;
                    }
                    break;
                }
            }
            if !self.display.window.has_focus() || background_tab {
                crate::notify::deliver(&self.display.window, &notification, Some(pane_id));
            }
        }
        self.dirty = true;
        self.display.window.request_redraw();
        true
    }

    pub fn refresh_agent_screen_states(&mut self) {
        let pane_ids: Vec<_> = self.panes.iter().map(|pane| pane.id).collect();
        for pane_id in pane_ids {
            self.runtime_flush_pending_submit(Some(pane_id));
        }
        for pane in &mut self.panes {
            let state = &mut pane.nebula_state;
            let (prompt_restored, screen) = {
                let term = pane.terminal.lock();
                let lines = term.screen_lines();
                if lines == 0 || term.columns() == 0 {
                    continue;
                }
                let prompt_restored = state.runtime_submit_barrier.is_none()
                    && (state.command_started.is_some() || state.running_program.is_some())
                    && state.pending_command_prompt.as_deref().is_some_and(|expected| {
                        crate::display::nebula_shell_prompt_restored_from_raw_grid(
                            &term,
                            expected,
                            &state.suggest_env,
                        )
                    });
                let screen = state.agent_activity.allows_screen().then(|| {
                    let start = Point::new(Line((lines - lines.min(24)) as i32), Column(0));
                    let end = Point::new(
                        Line(lines as i32 - 1),
                        Column(term.columns().saturating_sub(1)),
                    );
                    term.bounds_to_string(start, end)
                });
                (prompt_restored, screen)
            };
            if prompt_restored {
                if let Some(started) = state.command_started
                    && !state.agent_activity.hook_seen()
                    && started.elapsed() >= crate::notify::COMMAND_NOTIFY_MIN
                    && !self.display.window.has_focus()
                {
                    crate::notify::deliver(
                        &self.display.window,
                        &crate::notify::Notification::CommandDone {
                            duration: started.elapsed(),
                            program: state.running_program.clone(),
                        },
                        Some(pane.id),
                    );
                }
                if let Some(run) = state.active_run.take() {
                    state.last_run =
                        Some(crate::runtime_api::RuntimeRunOutcome::command_done(run, None));
                }
                clear_foreground(state);
                #[cfg(windows)]
                if let Some(hwnd) = self.display.window.native_window_handle_id() {
                    crate::taskbar::apply(hwnd as isize, crate::taskbar::TaskProgress::None);
                }
                continue;
            }
            let Some(screen) = screen else { continue };
            let program = match state.running_program.as_deref() {
                Some(program) if AgentKind::parse(program).is_some() => program.to_owned(),
                Some(program)
                    if !crate::process_tree::is_interactive_shell_command(program)
                        && !crate::process_tree::display_name(program)
                            .eq_ignore_ascii_case("ssh") =>
                {
                    state.agent_activity.observe_screen(None);
                    continue;
                },
                _ => {
                    let Some(agent) = crate::ai_agents::identify(&screen) else {
                        state.agent_activity.observe_screen(None);
                        continue;
                    };
                    let program = agent.slug().to_owned();
                    state.running_program = Some(program.clone());
                    state.agent_activity.identify(AgentStatusSource::Screen);
                    program
                },
            };
            let previous = state.agent_activity.status();
            if !state.agent_activity.observe_screen(crate::ai_agents::detect(&program, &screen)) {
                continue;
            }
            project_status(state);
            if let Some(attention) = crate::ai_hook::lifecycle::screen_notification(
                previous,
                state.agent_activity.status(),
                state.agent_activity.hook_seen(),
            ) && !self.display.window.has_focus()
            {
                crate::notify::deliver(
                    &self.display.window,
                    &crate::notify::Notification::AiTurn { program, message: None, attention },
                    Some(pane.id),
                );
            }
        }
        self.sync_chrome_tabs();
    }
}
