use super::*;
use crate::ai_agents::AgentKind;
use crate::ai_hook::integrations::{self, AgentIntegration};
use crate::i18n::Message;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support"))]
type TestOperation = std::sync::Arc<
    dyn Fn(
            nebula_settings::AgentHook,
            bool,
        )
            -> futures::future::BoxFuture<'static, (Result<(), String>, Vec<AgentIntegration>)>
        + Send
        + Sync,
>;

pub(super) struct AgentSettingsState {
    rows: Option<Vec<AgentIntegration>>,
    loading: bool,
    busy: Option<AgentKind>,
    sequence: u64,
    feedback: Option<(AgentKind, Result<bool, String>)>,
    focus: Vec<FocusHandle>,
    #[cfg(all(test, feature = "gpui-test-support"))]
    test_operation: Option<TestOperation>,
}

impl AgentSettingsState {
    pub(super) fn new(cx: &mut Context<SettingsPane>) -> Self {
        Self {
            rows: None,
            loading: false,
            busy: None,
            sequence: 0,
            feedback: None,
            focus: integrations::AGENTS.iter().map(|_| cx.focus_handle().tab_stop(true)).collect(),
            #[cfg(all(test, feature = "gpui-test-support"))]
            test_operation: None,
        }
    }
}

impl SettingsPane {
    fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        if self.agents.loading || self.agents.busy.is_some() {
            return;
        }
        self.agents.loading = true;
        self.agents.sequence = self.agents.sequence.wrapping_add(1);
        let sequence = self.agents.sequence;
        let task = cx.background_executor().spawn(async { integrations::inspect() });
        cx.spawn(async move |this, cx| {
            let rows = task.await;
            let _ = this.update(cx, |pane, cx| {
                if pane.agents.sequence != sequence {
                    return;
                }
                pane.agents.rows = Some(rows);
                pane.agents.loading = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle_agent_hook(&mut self, agent: AgentKind, enabled: bool, cx: &mut Context<Self>) {
        if self.agents.loading || self.agents.busy.is_some() {
            return;
        }
        let Some(row) =
            self.agents.rows.as_ref().and_then(|rows| rows.iter().find(|row| row.agent == agent))
        else {
            return;
        };
        if !can_toggle(row) {
            return;
        }
        let Some(hook) = row.hook else { return };
        self.agents.busy = Some(agent);
        self.agents.feedback = None;
        self.agents.sequence = self.agents.sequence.wrapping_add(1);
        let sequence = self.agents.sequence;
        #[cfg(all(test, feature = "gpui-test-support"))]
        let test_operation = self.agents.test_operation.clone();
        let task = cx.background_executor().spawn(async move {
            #[cfg(all(test, feature = "gpui-test-support"))]
            if let Some(operation) = test_operation {
                return operation(hook, enabled).await;
            }
            let result = integrations::set_enabled(hook, enabled);
            (result, integrations::inspect())
        });
        // A submitted file operation completes if the pane closes. The weak
        // entity/sequence prevents its result from reviving a destroyed view.
        cx.spawn(async move |this, cx| {
            let (result, rows) = task.await;
            let _ = this.update(cx, |pane, cx| {
                if pane.agents.sequence != sequence {
                    return;
                }
                pane.agents.rows = Some(rows);
                pane.agents.busy = None;
                pane.agents.feedback = Some((agent, result.map(|()| enabled)));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn section_agents(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        if self.agents.rows.is_none() && !self.agents.loading {
            self.refresh_agents(cx);
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let mut list = v_flex()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(crate::gpui_shell::theme::settings_hairline(cx));
        if let Some(rows) = self.agents.rows.as_ref() {
            for (index, row) in rows.iter().enumerate() {
                list = list.child(self.agent_hook_row(index, row, cx));
            }
        }
        v_flex()
            .w_full()
            .max_w(px(720.0))
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(20.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::SettingsAgentsTitle)),
                    )
                    .child(
                        Button::new("agents-refresh")
                            .ghost()
                            .small()
                            .icon(IconName::Redo)
                            .label(language.text(Message::SettingsAgentsRefresh))
                            .disabled(self.agents.loading || self.agents.busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_agents(cx))),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(language.text(Message::SettingsAgentsDescription)),
            )
            .when(!crate::platform::CAPABILITIES.ai_hook_server, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::SettingsAgentsPlatformUnsupported)),
                )
            })
            .when(self.agents.loading, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::SettingsAgentsChecking)),
                )
            })
            .child(list)
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(language.text(Message::SettingsAgentsRestart)),
            )
    }

    fn agent_hook_row(
        &self,
        index: usize,
        row: &AgentIntegration,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let language = crate::gpui_shell::config::ui_language(cx);
        let agent = row.agent;
        let state = &row.inspection;
        let busy = self.agents.busy == Some(agent);
        let disabled = self.agents.loading || self.agents.busy.is_some() || !can_toggle(row);
        let checked = state.enabled;
        let focus = self.agents.focus[index].clone();
        let status = agent_status(row, busy);
        let note = self
            .agents
            .feedback
            .as_ref()
            .filter(|(kind, _)| *kind == agent)
            .map(|(_, result)| match result {
                Ok(true) => language.text(Message::SettingsAgentsEnabled).to_owned(),
                Ok(false) => language.text(Message::SettingsAgentsDisabled).to_owned(),
                Err(error) => language.format(Message::SettingsAgentsFailed, &[("error", error)]),
            })
            .or_else(|| {
                state.error.as_ref().map(|error| {
                    language.format(Message::SettingsAgentsFailed, &[("error", error)])
                })
            })
            .or_else(|| {
                (state.helper_missing
                    && row.executable.is_some()
                    && row.hook.is_some()
                    && crate::platform::CAPABILITIES.ai_hook_server)
                    .then(|| language.text(Message::SettingsAgentsHelperMissing).to_owned())
            })
            .or_else(|| {
                (!state.available && row.executable.is_some() && row.hook.is_some())
                    .then(|| language.text(Message::SettingsAgentsInitialize).to_owned())
            });
        let muted = cx.theme().muted_foreground;
        let hover = crate::gpui_shell::theme::settings_hover_bg(cx, false);
        let border = crate::gpui_shell::theme::settings_hairline(cx);
        let ring = cx.theme().ring;
        let executable = row
            .executable
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| language.text(Message::SettingsAgentsNotDetected).to_owned());
        let path_tooltip = executable.clone();
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(
                div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(agent.display_name()),
            )
            .child(
                div()
                    .id(("agent-cli-path", index))
                    .debug_selector(move || format!("agent-cli-path-{index}"))
                    .w_full()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(path_tooltip.clone())
                            .build(window, cx)
                    })
                    .child(executable),
            )
            .when_some(note, |view, note| {
                view.child(div().text_xs().text_color(muted).child(note))
            });
        use crate::gpui_shell::assets::nav;
        let icon = if let Some(path) = match agent {
            AgentKind::Claude => Some(nav::AGENT_CLAUDE),
            AgentKind::Codex => Some(nav::AGENT_OPENAI),
            AgentKind::OpenCode => Some(nav::AGENT_OPENCODE),
            AgentKind::Cursor => Some(nav::AGENT_CURSOR),
            AgentKind::Kimi => Some(nav::AGENT_KIMI),
            AgentKind::Pi => Some(nav::AGENT_PI),
            AgentKind::OhMyPi => Some(nav::AGENT_OMP),
            AgentKind::Copilot => Some(nav::AGENT_COPILOT),
            AgentKind::Grok => Some(nav::AGENT_GROK),
            _ => None,
        } {
            Icon::default()
                .path(path)
                .size(px(24.0))
                .text_color(match agent {
                    AgentKind::Claude => rgb_hsla(217, 119, 87),
                    AgentKind::OhMyPi => rgb_hsla(147, 98, 239),
                    _ => cx.theme().foreground,
                })
                .into_any_element()
        } else {
            div()
                .font_family(crate::font_install::REQUIRED_FONT_FAMILY)
                .text_size(px(24.0))
                .child(crate::display::program_icon(agent.slug()))
                .into_any_element()
        };
        h_flex()
            .id(("agent-hook-row", index))
            .debug_selector(move || format!("agent-hook-row-{index}"))
            .w_full()
            .min_h(px(64.0))
            .px_4()
            .py_3()
            .gap_3()
            .border_b_1()
            .border_color(if index + 1 < self.agents.rows.as_ref().map_or(0, Vec::len) {
                border
            } else {
                gpui::transparent_black()
            })
            .track_focus(&focus.clone().tab_stop(!disabled))
            .role(gpui::Role::Switch)
            .aria_label(format!("{} hooks — {}", agent.display_name(), language.text(status)))
            .aria_toggled(if checked { gpui::Toggled::True } else { gpui::Toggled::False })
            .when(!disabled, |view| {
                view.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(hover))
                    .focus_visible(move |style| style.border_color(ring))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !disabled {
                    window.focus(&focus, cx);
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !disabled {
                    this.toggle_agent_hook(agent, !checked, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !disabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    this.toggle_agent_hook(agent, !checked, cx);
                }
            }))
            .child(
                div()
                    .size(px(32.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon),
            )
            .child(text)
            .child(
                div()
                    .id(("agent-hook-status", index))
                    .debug_selector(move || format!("agent-hook-status-{index}"))
                    .flex_shrink_0()
                    .max_w(px(160.0))
                    .text_xs()
                    .text_color(muted)
                    .child(language.text(status)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(40.0))
                    .min_h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::gpui_shell::widgets::NebulaSwitch::new(format!(
                            "agent-{}",
                            agent.slug()
                        ))
                        .checked(checked)
                        .disabled(disabled)
                        .on_click(cx.listener(
                            move |this, enabled, _, cx| this.toggle_agent_hook(agent, *enabled, cx),
                        )),
                    ),
            )
    }
}

fn can_toggle(row: &AgentIntegration) -> bool {
    crate::platform::CAPABILITIES.ai_hook_server
        && row.hook.is_some()
        && (row.inspection.installed
            || (row.executable.is_some()
                && row.inspection.available
                && !row.inspection.helper_missing))
}

fn agent_status(row: &AgentIntegration, busy: bool) -> Message {
    let state = &row.inspection;
    if busy {
        Message::SettingsAgentsApplying
    } else if state.installed && state.needs_repair {
        Message::SettingsAgentsNeedsRepair
    } else if state.installed {
        Message::SettingsAgentsInstalled
    } else if state.error.is_some() {
        Message::SettingsAgentsNeedsAttention
    } else if row.hook.is_none() {
        Message::SettingsAgentsUnsupported
    } else if !crate::platform::CAPABILITIES.ai_hook_server {
        Message::SettingsAgentsUnavailable
    } else {
        Message::SettingsAgentsOff
    }
}
