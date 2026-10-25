use super::*;
use crate::i18n::Message;
use nebula_settings::BackgroundEffects;

impl SettingsPane {
    pub(super) fn set_shader_preset(
        &mut self,
        preset: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(updates) = BackgroundEffects::selection_updates(preset) else { return };
        let previous = self.runtime.background_effects.clone();
        self.shader_custom_open = preset == "wgsl";
        cx.notify();
        // 确认前恢复真实选中值，取消或写盘失败时不留下虚假的启用状态。
        self.sync_select("background_shader_preset", previous.preset(), window, cx);
        if preset == previous.preset() {
            return;
        }
        if preset != "off" && !super::super::wallpaper::shader_available() {
            super::super::wallpaper::show_shader_error(cx);
            return;
        }
        if preset == "wgsl" {
            if previous.wgsl_path.is_none() {
                // 先让用户选择文件，再由显式启用操作确认；不能要求先启用才能看到文件入口。
                self.choose_shader_source(cx);
                return;
            }
            let language = crate::gpui_shell::config::ui_language(cx);
            let pane = cx.weak_entity();
            window.open_dialog(cx, move |dialog, window, _| {
                let pane = pane.clone();
                let expected = previous.clone();
                let updates = updates.clone();
                confirm_dialog(
                    dialog,
                    window,
                    language.text(Message::WallpaperShaderConfirm),
                    SharedString::from(language.text(Message::WallpaperShaderConfirmDescription)),
                    language.text(Message::WallpaperShaderEnable),
                    language.text(Message::WallpaperShaderCancel),
                    ButtonVariant::Primary,
                )
                .on_ok(move |_, window, cx| {
                    let _ = pane.update(cx, |this, cx| {
                        if this.runtime.background_effects != expected {
                            return;
                        }
                        this.persist(&updates, cx);
                        let actual = this.runtime.background_effects.preset();
                        this.sync_select("background_shader_preset", actual, window, cx);
                    });
                    true
                })
            });
        } else {
            self.persist(&updates, cx);
            let actual = self.runtime.background_effects.preset();
            self.sync_select("background_shader_preset", actual, window, cx);
        }
    }

    fn choose_shader_source(&mut self, cx: &mut Context<Self>) {
        if self.shader_picker.is_some() {
            return;
        }
        if !super::super::wallpaper::shader_available() {
            super::super::wallpaper::show_shader_error(cx);
            return;
        }
        let previous = self.runtime.background_effects.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(language.text(Message::WallpaperShaderPrompt).into()),
        });
        self.shader_picker = Some(cx.spawn(async move |entity, cx| {
            let result = picked.await;
            let _ = entity.update(cx, |this, cx| {
                if let Some(task) = this.shader_picker.take() {
                    task.detach();
                }
                if this.runtime.background_effects != previous {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            if let Some(path) = path.to_str() {
                                this.persist(&[("background_wgsl_path", path.to_owned())], cx);
                            } else {
                                super::super::wallpaper::show_shader_error(cx);
                            }
                        }
                    },
                    Ok(Ok(None)) => {},
                    _ => super::super::wallpaper::show_shader_error(cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn shader_source_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let current = self.runtime.background_effects.wgsl_path.clone();
        let pending = self.shader_picker.is_some();
        let available = super::super::wallpaper::shader_available();
        let name = current.as_deref().map(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path)
                .to_owned()
        });
        self.row_with_reset(
            language.text(Message::WallpaperShaderSource),
            language.text(Message::WallpaperShaderSourceDescription),
            current.is_some(),
            |this, window, cx| {
                this.shader_picker.take();
                this.persist(
                    &[
                        ("background_wgsl_path", String::new()),
                        ("background_wgsl_enabled", "false".to_owned()),
                    ],
                    cx,
                );
                let actual = this.runtime.background_effects.preset();
                this.sync_select("background_shader_preset", actual, window, cx);
            },
            h_flex()
                .items_center()
                .flex_wrap()
                .max_w_full()
                .gap_2()
                .child(
                    NebulaButton::new("background-shader-choose")
                        .label(language.text(if pending {
                            Message::WallpaperShaderSelecting
                        } else {
                            Message::WallpaperShaderChoose
                        }))
                        .disabled(pending || !available)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_shader_source(cx))),
                )
                .when(current.is_some() && !self.runtime.background_effects.wgsl, |row| {
                    row.child(
                        NebulaButton::new("background-shader-enable")
                            .label(language.text(Message::WallpaperShaderEnable))
                            .disabled(pending || !available)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_shader_preset("wgsl", window, cx);
                            })),
                    )
                })
                .child(
                    NebulaButton::new("background-shader-reload")
                        .label(language.text(Message::WallpaperShaderReload))
                        .disabled(pending || !available || !self.runtime.background_effects.wgsl)
                        .on_click(
                            cx.listener(|_, _, _, cx| super::super::wallpaper::reload_shader(cx)),
                        ),
                )
                .when_some(name, |row, name| {
                    row.child(
                        div()
                            .max_w(px(180.0))
                            .min_w_0()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(name),
                    )
                }),
            cx,
        )
    }
}
