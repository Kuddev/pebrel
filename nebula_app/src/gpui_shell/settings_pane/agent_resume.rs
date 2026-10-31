use super::*;
use crate::i18n::Message;
use nebula_settings::AGENT_RESUME_SETTINGS;

impl SettingsPane {
    pub(super) fn save_agent_resume_args(&mut self, index: usize, cx: &mut Context<Self>) {
        let value = self.agents.resume_inputs[index].read(cx).value().to_string();
        let language = crate::gpui_shell::config::ui_language(cx);
        let result = match crate::agent_resume::parse(&value) {
            Some(args) => {
                let value = if args.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&args).expect("string arguments serialize")
                };
                self.try_persist(&[(AGENT_RESUME_SETTINGS[index].1, value)], cx).map_err(|error| {
                    language.format(
                        Message::SettingsAgentsResumeFailed,
                        &[("error", &error.to_string())],
                    )
                })
            },
            None => Err(language.text(Message::SettingsAgentsResumeInvalid).to_owned()),
        };
        self.agents.resume_feedback = Some((index, result));
        cx.notify();
    }

    pub(super) fn agent_resume_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let mut content = v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(language.text(Message::SettingsAgentsResumeTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(language.text(Message::SettingsAgentsResumeDescription)),
            );
        for (index, (source, _)) in AGENT_RESUME_SETTINGS.iter().enumerate() {
            let agent =
                crate::ai_agents::AgentKind::parse(source).expect("registered resume source");
            let feedback =
                self.agents.resume_feedback.as_ref().filter(|(row, _)| *row == index).map(
                    |(_, result)| match result {
                        Ok(()) => language.text(Message::CommonSaved).to_owned(),
                        Err(error) => error.clone(),
                    },
                );
            content = content.child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(div().text_sm().child(agent.display_name()))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(move || format!("agent-resume-input-{index}"))
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        Input::new(&self.agents.resume_inputs[index]).aria_label(
                                            format!(
                                                "{} {}",
                                                agent.display_name(),
                                                language.text(Message::SettingsAgentsResumeTitle)
                                            ),
                                        ),
                                    ),
                            )
                            .child(
                                Button::new(("agent-resume-save", index))
                                    .debug_selector(move || format!("agent-resume-save-{index}"))
                                    .label(language.text(Message::CommonSave))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.save_agent_resume_args(index, cx);
                                    })),
                            ),
                    )
                    .when_some(feedback, |view, text| {
                        view.child(div().text_xs().text_color(muted).child(text))
                    }),
            );
        }
        content
    }
}
