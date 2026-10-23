use super::*;
use crate::ai_agents::AgentKind;
use gpui::AppContext as _;
use gpui_component::menu::PopupMenuItem;

impl SettingsPane {
    fn install_ssh_skill(&mut self, agent: AgentKind, cx: &mut Context<Self>) {
        if self.ssh_library.skill_busy {
            return;
        }
        self.ssh_library.skill_busy = true;
        self.ssh_library.skill_result = None;
        let task = cx.background_executor().spawn(async move {
            crate::ai_hook::install_ssh_skill_for(agent).map(|path| (agent, path))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.ssh_library.skill_busy = false;
                this.ssh_library.skill_result = Some(result);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn ssh_skill_controls(&self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        let copied = self.ssh_library.skill_copy.read(cx).is_copied();
        let feedback = self.ssh_library.skill_result.as_ref().map(|result| match result {
            Ok((agent, path)) => language.format(
                Message::HostsSkillInstalled,
                &[("agent", agent.display_name()), ("path", &path.display().to_string())],
            ),
            Err(error) => language.format(Message::HostsSkillFailed, &[("error", error)]),
        });
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        Button::new("ssh-install-skill")
                            .debug_selector(|| "ssh-install-skill".into())
                            .label(language.text(if self.ssh_library.skill_busy {
                                Message::HostsSkillInstalling
                            } else {
                                Message::HostsInstallSkill
                            }))
                            .dropdown_caret(true)
                            .disabled(self.ssh_library.skill_busy)
                            .dropdown_menu(move |mut menu, _, _| {
                                for agent in crate::ai_hook::integrations::AGENTS {
                                    let owner = owner.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(agent.display_name()).on_click(
                                            move |_, _, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    this.install_ssh_skill(agent, cx)
                                                });
                                            },
                                        ),
                                    );
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("ssh-copy-skill")
                            .debug_selector(|| "ssh-copy-skill".into())
                            .icon(if copied { IconName::Check } else { IconName::Copy })
                            .label(language.text(if copied {
                                Message::HostsSkillCopied
                            } else {
                                Message::HostsCopySkill
                            }))
                            .min_w(px(120.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    crate::ai_hook::SSH_SKILL_MD.to_owned(),
                                ));
                                this.ssh_library
                                    .skill_copy
                                    .update(cx, |feedback, cx| feedback.mark_copied(cx));
                            })),
                    ),
            )
            .when_some(feedback, |view, message| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(
                            if self.ssh_library.skill_result.as_ref().is_some_and(Result::is_err) {
                                cx.theme().danger
                            } else {
                                cx.theme().success
                            },
                        )
                        .child(message),
                )
            })
    }
}
