//! 三平台共用一次性密钥输入；仅凭据适配器接收明文，设置只保存提示信息。
use super::*;

impl SettingsPane {
    pub(in crate::gpui_shell::settings_pane) fn prompt_provider_key(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.provider_key_task.is_some() {
            return;
        }
        let Some(index) = self.active_provider_index() else { return };
        let provider = self.provider_store.providers[index].clone();
        let input = cx.new(|cx| InputState::new(window, cx).masked(true));
        let focus = input.clone();
        let pane = cx.entity().downgrade();
        let language = crate::gpui_shell::config::ui_language(cx);
        window.open_dialog(cx, move |dialog, window, _cx| {
            let value = input.clone();
            let clear = input.clone();
            let pane = pane.clone();
            let provider = provider.clone();
            confirm_dialog(
                dialog,
                window,
                "API Key",
                provider.name.clone(),
                language.text(crate::i18n::Message::CommonSave),
                language.text(crate::i18n::Message::CommonCancel),
                ButtonVariant::Primary,
            )
            .child(
                div()
                    // masked 只负责绘制；捕获动作才能阻止快捷键和原生菜单复制明文。
                    .capture_action(|_: &gpui_component::input::Copy, _, cx| cx.stop_propagation())
                    .capture_action(|_: &gpui_component::input::Cut, _, cx| cx.stop_propagation())
                    .child(Input::new(&input)),
            )
            .on_ok(move |_, window, cx| {
                let secret = zeroize::Zeroizing::new(value.read(cx).value().to_string());
                if secret.trim().is_empty() {
                    return false;
                }
                value.update(cx, |input, cx| input.set_value("", window, cx));
                let mut provider = provider.clone();
                let _ = pane.update(cx, |pane, cx| {
                    if pane.provider_key_task.is_some() {
                        return;
                    }
                    let work = cx.background_executor().spawn(async move {
                        crate::ai_providers::store_provider_api_key(&mut provider, &secret)
                            .map(|()| provider)
                    });
                    // 提交后凭据写入必须完成元数据收尾；临时留住实体，关掉设置页也不丢结果。
                    let owner = cx.entity();
                    pane.provider_key_task = Some(cx.spawn(async move |_, cx| {
                        let result = work.await;
                        let _ = owner.update(cx, |pane, cx| {
                            pane.provider_key_task = None;
                            match result {
                                Ok(updated) => {
                                    if let Some(provider) = pane
                                        .provider_store
                                        .providers
                                        .iter_mut()
                                        .find(|provider| provider.id == updated.id)
                                    {
                                        provider.api_key_set = updated.api_key_set;
                                        provider.api_key_hint = updated.api_key_hint;
                                    }
                                    pane.provider_status = Some(
                                        match crate::ai_providers::save(&pane.provider_store) {
                                            Ok(()) => ProviderStatus::ApiKeySaved,
                                            Err(error) => ProviderStatus::Error(error.to_string()),
                                        },
                                    );
                                },
                                Err(error) => {
                                    pane.provider_status =
                                        Some(ProviderStatus::Error(error.to_string()))
                                },
                            }
                            cx.notify();
                        });
                    }));
                    cx.notify();
                });
                true
            })
            .on_close(move |_, window, cx| {
                clear.update(cx, |input, cx| input.set_value("", window, cx))
            })
        });
        focus.update(cx, |input, cx| input.focus(window, cx));
    }
}
