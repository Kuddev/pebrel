use super::*;
use crate::i18n::Message;

impl SettingsPane {
    fn choose_terminal_effect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_effect_picker.is_some()
            || !crate::platform::effect_activity::supported(window)
        {
            return;
        }
        let previous = self.runtime.terminal_effects.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let handle = Window::window_handle(window);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(language.text(Message::TerminalEffectPrompt).into()),
        });
        self.terminal_effect_picker = Some(cx.spawn(async move |entity, cx| {
            let result = picked.await;
            let _ = entity.update(cx, |this, cx| {
                if let Some(task) = this.terminal_effect_picker.take() {
                    task.detach();
                }
                if this.runtime.terminal_effects != previous {
                    cx.notify();
                    return;
                }
                match result {
                    // 原生选择器也可能用空列表表示取消，不能因此停用现有的效果链。
                    Ok(Ok(Some(paths))) if paths.is_empty() => {},
                    Ok(Ok(Some(paths))) if paths.iter().all(|path| path.to_str().is_some()) => {
                        let mut sources = previous.paths.clone();
                        sources.extend(
                            paths
                                .iter()
                                .map(|path| path.to_str().expect("checked UTF-8 path").to_owned()),
                        );
                        if let Err(error) = this.persist_terminal_sources(&sources, true, cx) {
                            let _ = handle.update(cx, |_, window, cx| {
                                show_source_error(&error, window, cx);
                            });
                        }
                    },
                    Ok(Ok(None)) => {},
                    _ => {
                        if let Err(error) = handle.update(cx, |_, window, cx| {
                            crate::gpui_shell::toast::toast(
                                window,
                                cx,
                                crate::gpui_shell::toast::ToastKind::Warning,
                                crate::gpui_shell::config::ui_language(cx)
                                    .text(Message::TerminalEffectPickFailed),
                            );
                        }) {
                            log::debug!("effect picker window released: {error}");
                        }
                    },
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_terminal_effect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::gpui_shell::config::ui_language(cx);
        if self.runtime.terminal_effects.enabled {
            self.persist(&[("terminal_effect_enabled", "false".into())], cx);
            return;
        }
        if !crate::platform::effect_activity::supported(window) {
            return;
        }
        if self.runtime.terminal_effects.paths.is_empty() {
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::gpui_shell::toast::ToastKind::Warning,
                language.text(Message::TerminalEffectMissing),
            );
            return;
        }
        let expected = self.runtime.terminal_effects.clone();
        let entity = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, _| {
            let expected = expected.clone();
            let entity = entity.clone();
            confirm_dialog(
                dialog,
                window,
                language.text(Message::TerminalEffectConfirm),
                SharedString::from(language.text(Message::TerminalEffectConfirmDescription)),
                language.text(Message::TerminalEffectEnable),
                language.text(Message::WallpaperShaderCancel),
                ButtonVariant::Primary,
            )
            .on_ok(move |_, window, cx| {
                if !crate::platform::effect_activity::supported(window) {
                    return true;
                }
                let _ = entity.update(cx, |this, cx| {
                    this.runtime = RuntimeSettings::load();
                    if this.runtime.terminal_effects == expected {
                        this.persist(&[("terminal_effect_enabled", "true".into())], cx);
                    }
                    cx.notify();
                });
                true
            })
        });
    }

    fn persist_terminal_sources(
        &mut self,
        paths: &[String],
        deactivate: bool,
        cx: &mut Context<Self>,
    ) -> std::io::Result<()> {
        let mut updates = nebula_settings::TerminalEffects::source_updates(paths)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        if deactivate || paths.is_empty() {
            updates.push(("terminal_effect_enabled", "false".into()));
        }
        self.try_persist(&updates, cx)
    }

    fn edit_terminal_source(
        &mut self,
        expected: &[String],
        index: usize,
        direction: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_effect_picker.is_some()
            || self.runtime.terminal_effects.paths != expected
            || index >= expected.len()
        {
            return;
        }
        let mut paths = expected.to_vec();
        match direction {
            Some(true) if index > 0 => paths.swap(index, index - 1),
            Some(false) if index + 1 < paths.len() => paths.swap(index, index + 1),
            None => {
                paths.remove(index);
            },
            _ => return,
        }
        if let Err(error) = self.persist_terminal_sources(&paths, false, cx) {
            show_source_error(&error, window, cx);
        }
    }

    pub(super) fn terminal_effect_row(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let config = &self.runtime.terminal_effects;
        let busy = self.terminal_effect_picker.is_some();
        let available = crate::platform::effect_activity::supported(window);
        let sources = config
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let label = std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path);
                let previous = config.paths.clone();
                let next = config.paths.clone();
                let removed = config.paths.clone();
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("{}. {label}", index + 1)),
                    )
                    .child(
                        NebulaButton::new(format!("terminal-effect-up-{index}"))
                            .label(language.text(Message::TerminalEffectMoveUp))
                            .disabled(busy || index == 0)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_terminal_source(&previous, index, Some(true), window, cx)
                            })),
                    )
                    .child(
                        NebulaButton::new(format!("terminal-effect-down-{index}"))
                            .label(language.text(Message::TerminalEffectMoveDown))
                            .disabled(busy || index + 1 == config.paths.len())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_terminal_source(&next, index, Some(false), window, cx)
                            })),
                    )
                    .child(
                        NebulaButton::new(format!("terminal-effect-remove-{index}"))
                            .label(language.text(Message::TerminalEffectRemove))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_terminal_source(&removed, index, None, window, cx)
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let header = self.row_with_reset(
            language.text(Message::TerminalEffectTitle),
            language.text(if available {
                Message::TerminalEffectDescription
            } else {
                Message::TerminalEffectUnsupported
            }),
            config.enabled || !config.paths.is_empty(),
            |this, window, cx| {
                this.terminal_effect_picker.take();
                if let Err(error) = this.persist_terminal_sources(&[], true, cx) {
                    show_source_error(&error, window, cx);
                }
            },
            v_flex().gap_2().min_w_0().child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        NebulaButton::new("terminal-effect-choose")
                            .label(language.text(if busy {
                                Message::TerminalEffectChoosing
                            } else {
                                Message::TerminalEffectChoose
                            }))
                            .disabled(
                                busy || !available
                                    || config.paths.len()
                                        >= nebula_settings::TerminalEffects::PATH_KEYS.len(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_terminal_effect(window, cx)
                            })),
                    )
                    .child(
                        NebulaButton::new("terminal-effect-toggle")
                            .label(language.text(if config.enabled {
                                Message::TerminalEffectDisable
                            } else {
                                Message::TerminalEffectEnable
                            }))
                            .disabled(busy || (!available && !config.enabled))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_terminal_effect(window, cx)
                            })),
                    )
                    .child(
                        NebulaButton::new("terminal-effect-reload")
                            .label(language.text(Message::TerminalEffectReload))
                            .disabled(busy || !available || !config.enabled)
                            .on_click(cx.listener(|_, _, window, cx| {
                                if crate::platform::effect_activity::supported(window) {
                                    super::super::wallpaper::reload_terminal_effects(cx)
                                }
                            })),
                    ),
            ),
            cx,
        );
        // 文件列表需要正文的宽度，不能塞进为单个下拉框预留的控件列。
        v_flex().w_full().child(header).when(!sources.is_empty(), |panel| {
            panel.child(
                v_flex()
                    .w_full()
                    .pl(px(super::design::RAIL_INDENT))
                    .pr_4()
                    .pb(px(self.row_padding_y()))
                    .gap_2()
                    .children(sources),
            )
        })
    }
}

fn show_source_error(error: &std::io::Error, window: &mut Window, cx: &mut App) {
    log::warn!("effect source update failed: {error}");
    let message = if error.kind() == std::io::ErrorKind::InvalidInput {
        Message::TerminalEffectSourcesInvalid
    } else {
        Message::TerminalEffectSaveFailed
    };
    crate::gpui_shell::toast::toast(
        window,
        cx,
        crate::gpui_shell::toast::ToastKind::Warning,
        crate::gpui_shell::config::ui_language(cx).text(message),
    );
}
