//! Rule-based extra CLI arguments for Agent session resume and fork.

use super::*;
use crate::ai_agents::AgentKind;
use crate::i18n::Message;
use gpui_component::menu::PopupMenuItem;
use nebula_settings::{AgentResumeArgs, AgentResumeRule};

pub(super) const RESUMABLE_AGENTS: [AgentKind; 12] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::Gemini,
    AgentKind::OpenCode,
    AgentKind::Cursor,
    AgentKind::Kimi,
    AgentKind::Pi,
    AgentKind::OhMyPi,
    AgentKind::Copilot,
    AgentKind::Grok,
    AgentKind::Amp,
];

pub(super) struct AgentResumeSettingsUi {
    pub(super) selected_agent: AgentKind,
    pub(super) args_input: Entity<InputState>,
    pub(super) error: Option<Message>,
    pub(super) save_error: Option<String>,
    pub(super) saved_agent: Option<AgentKind>,
    _subscription: Subscription,
}

impl SettingsPane {
    pub(super) fn prepare_agent_resume_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agents.resume_ui.is_some() {
            return;
        }
        let language = crate::gpui_shell::config::ui_language(cx);
        let initial_agent = self
            .runtime
            .agent_resume_args
            .rules()
            .first()
            .and_then(|rule| AgentKind::parse(rule.source))
            .unwrap_or(AgentKind::Codex);
        let initial_args = self
            .runtime
            .agent_resume_args
            .rule(initial_agent.slug())
            .map(|rule| rule.args)
            .unwrap_or_default();
        let args_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(Message::SettingsAgentsResumeArgsPlaceholder))
                .default_value(initial_args)
        });
        let subscription = cx.subscribe_in(
            &args_input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Some(ui) = this.agents.resume_ui.as_mut() {
                        ui.error = None;
                        ui.save_error = None;
                        ui.saved_agent = None;
                    }
                    cx.notify();
                },
                InputEvent::PressEnter { .. } => {
                    this.save_agent_resume_rule(window, cx);
                },
                _ => {},
            },
        );
        self.agents.resume_ui = Some(AgentResumeSettingsUi {
            selected_agent: initial_agent,
            args_input,
            error: None,
            save_error: None,
            saved_agent: None,
            _subscription: subscription,
        });
    }

    pub(super) fn select_agent_resume_rule(
        &mut self,
        agent: AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .runtime
            .agent_resume_args
            .rule(agent.slug())
            .map(|rule| rule.args)
            .unwrap_or_default();
        if let Some(ui) = self.agents.resume_ui.as_mut() {
            ui.selected_agent = agent;
            ui.args_input.update(cx, |input, cx| {
                input.set_value(existing, window, cx);
            });
            ui.error = None;
            ui.save_error = None;
            ui.saved_agent = None;
        }
        cx.notify();
    }

    pub(super) fn save_agent_resume_rule(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.agents.resume_ui.as_ref() else { return };
        let agent = ui.selected_agent;
        let draft = ui.args_input.read(cx).value().trim().to_owned();
        if draft.is_empty() || crate::agent_resume::parse(&draft).is_none() {
            if let Some(ui) = self.agents.resume_ui.as_mut() {
                ui.error = Some(Message::SettingsAgentsResumeArgsInvalid);
                ui.save_error = None;
                ui.saved_agent = None;
            }
            cx.notify();
            return;
        }
        let enabled = self
            .runtime
            .agent_resume_args
            .rule(agent.slug())
            .map(|rule| rule.enabled)
            .unwrap_or(true);
        let Some(updates) = AgentResumeArgs::upsert_updates(agent.slug(), enabled, draft) else {
            return;
        };
        let result = self.try_persist(&updates, cx);
        if let Some(ui) = self.agents.resume_ui.as_mut() {
            ui.error = None;
            ui.save_error = result.as_ref().err().map(ToString::to_string);
            ui.saved_agent = result.is_ok().then_some(agent);
        }
        cx.notify();
    }

    pub(super) fn toggle_agent_resume_rule(
        &mut self,
        agent: AgentKind,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(rule) = self.runtime.agent_resume_args.rule(agent.slug()) else { return };
        let Some(updates) = AgentResumeArgs::upsert_updates(rule.source, enabled, rule.args) else {
            return;
        };
        let result = self.try_persist(&updates, cx);
        if let Some(ui) = self.agents.resume_ui.as_mut() {
            ui.error = None;
            ui.save_error = result.as_ref().err().map(ToString::to_string);
            ui.saved_agent = None;
        }
        cx.notify();
    }

    pub(super) fn remove_agent_resume_rule(
        &mut self,
        agent: AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(updates) = AgentResumeArgs::remove_updates(agent.slug()) else { return };
        let result = self.try_persist(&updates, cx);
        if let Some(ui) = self.agents.resume_ui.as_mut() {
            ui.error = None;
            ui.save_error = result.as_ref().err().map(ToString::to_string);
            ui.saved_agent = None;
            if result.is_ok() && ui.selected_agent == agent {
                ui.args_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
            }
        }
        cx.notify();
    }

    pub(super) fn agent_resume_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let Some(ui) = self.agents.resume_ui.as_ref() else { return div() };
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let border = crate::gpui_shell::theme::settings_hairline(cx);
        let selected_agent = ui.selected_agent;
        let owner = cx.entity().downgrade();
        let picker = Button::new("agent-resume-picker")
            .debug_selector(|| "agent-resume-picker".to_owned())
            .label(selected_agent.display_name())
            .w_full()
            .h(px(34.0))
            .map(providers::ProviderSelectTrigger::new)
            .dropdown_menu(move |mut menu, _, _| {
                for agent in RESUMABLE_AGENTS {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(agent.display_name())
                            .checked(agent == selected_agent)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.select_agent_resume_rule(agent, window, cx);
                                });
                            }),
                    );
                }
                menu
            });

        let draft = ui.args_input.read(cx).value();
        let trimmed = draft.trim();
        let current_saved = self
            .runtime
            .agent_resume_args
            .rule(selected_agent.slug())
            .map(|rule| rule.args)
            .unwrap_or_default();
        let dirty = trimmed != current_saved;
        let draft_valid = !trimmed.is_empty() && crate::agent_resume::parse(trimmed).is_some();

        let status = if let Some(error) = &ui.save_error {
            Some((language.format(Message::SettingsSaveFailed, &[("error", error)]), true))
        } else if let Some(error) = ui.error {
            Some((language.text(error).to_owned(), true))
        } else if !trimmed.is_empty() && !draft_valid {
            Some((language.text(Message::SettingsAgentsResumeArgsInvalid).to_owned(), true))
        } else if let Some(agent) = ui.saved_agent.filter(|_| !dirty) {
            Some((
                language.format(
                    Message::SettingsAgentsResumeArgsSaved,
                    &[("agent", agent.display_name())],
                ),
                false,
            ))
        } else {
            None
        };

        let rules = self.runtime.agent_resume_args.rules();
        let rules_view = if rules.is_empty() {
            div()
                .id("agent-resume-empty")
                .debug_selector(|| "agent-resume-empty".to_owned())
                .w_full()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .px_4()
                .py_3()
                .text_xs()
                .text_color(muted)
                .child(language.text(Message::SettingsAgentsResumeArgsEmpty))
                .into_any_element()
        } else {
            let total = rules.len();
            let mut list = v_flex().w_full().rounded_lg().border_1().border_color(border);
            for (index, rule) in rules.iter().enumerate() {
                list = list.child(self.agent_resume_rule_row(index, total, rule, cx));
            }
            list.into_any_element()
        };

        v_flex()
            .w_full()
            .pt_2()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.text(Message::SettingsAgentsResumeArgsTitle)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(language.text(Message::SettingsAgentsResumeArgsDescription)),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .flex_wrap()
                    .items_end()
                    .child(
                        v_flex()
                            .w(px(190.0))
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(language.text(Message::SettingsAgentsResumeArgsAgent)),
                            )
                            .child(picker),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(240.0))
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(language.text(Message::SettingsAgentsResumeArgsSuffix)),
                            )
                            .child(
                                div()
                                    .id("agent-resume-input")
                                    .debug_selector(|| "agent-resume-input".to_owned())
                                    .w_full()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(Input::new(&ui.args_input).h(px(34.0))),
                            ),
                    )
                    .child(
                        Button::new("agent-resume-save")
                            .debug_selector(|| "agent-resume-save".to_owned())
                            .label(language.text(Message::SettingsAgentsResumeArgsAdd))
                            .h(px(34.0))
                            .disabled(!dirty || !draft_valid)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_agent_resume_rule(window, cx);
                            })),
                    ),
            )
            .when_some(status, |view, (text, is_error)| {
                view.child(
                    div()
                        .id("agent-resume-status")
                        .debug_selector(|| "agent-resume-status".to_owned())
                        .text_xs()
                        .text_color(if is_error { cx.theme().danger } else { muted })
                        .child(text),
                )
            })
            .child(rules_view)
    }

    fn agent_resume_rule_row(
        &self,
        index: usize,
        total: usize,
        rule: &AgentResumeRule,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let agent = AgentKind::parse(rule.source).unwrap_or(AgentKind::Codex);
        let slug = rule.source;
        let enabled = rule.enabled;
        let args = rule.args.clone();
        let border = crate::gpui_shell::theme::settings_hairline(cx);
        let hover = crate::gpui_shell::theme::settings_hover_bg(cx, false);
        let muted = cx.theme().muted_foreground;
        let icon = self.agent_brand_icon(agent, cx);
        let language = crate::gpui_shell::config::ui_language(cx);

        h_flex()
            .id(("agent-resume-rule", index))
            .debug_selector(move || format!("agent-resume-rule-{slug}"))
            .w_full()
            .min_h(px(52.0))
            .px_4()
            .py_2()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(if index + 1 < total { border } else { gpui::transparent_black() })
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_agent_resume_rule(agent, window, cx);
            }))
            .child(
                div()
                    .size(px(28.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon),
            )
            .child(
                div()
                    .w(px(120.0))
                    .flex_shrink_0()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .when(!enabled, |label| label.text_color(muted))
                    .child(agent.display_name()),
            )
            .child(
                div()
                    .id(("agent-resume-args", index))
                    .debug_selector(move || format!("agent-resume-args-{slug}"))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .px_2()
                    .py_1()
                    .rounded(px(5.0))
                    .bg(cx.theme().muted.opacity(if enabled { 0.55 } else { 0.25 }))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .text_color(if enabled { cx.theme().foreground } else { muted })
                    .child(args),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        crate::gpui_shell::widgets::NebulaSwitch::new(format!(
                            "agent-resume-enabled-{slug}"
                        ))
                        .checked(enabled)
                        .on_click(cx.listener(
                            move |this, next: &bool, _, cx| {
                                this.toggle_agent_resume_rule(agent, *next, cx);
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        crate::gpui_shell::widgets::settings_icon_button(
                            Button::new(format!("agent-resume-delete-{slug}"))
                                .debug_selector(move || format!("agent-resume-delete-{slug}"))
                                .icon(IconName::Close)
                                .tooltip(language.text(Message::CommonRemove)),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.remove_agent_resume_rule(agent, window, cx);
                            },
                        )),
                    ),
            )
    }
}
