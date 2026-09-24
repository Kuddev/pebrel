use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, ParentElement as _, Styled as _, Window, div};

use crate::gpui_shell::prelude::*;
use crate::gpui_shell::terminal::view::TerminalView;
use crate::ssh_prompt::{Prompt, PromptKind, PromptResponse};

use super::{NebulaWorkspace, workspace_ui_language};

pub(super) fn show(request: Arc<Prompt>, window: &mut Window, cx: &mut App) {
    if !request.is_pending() {
        return;
    }
    let language = crate::gpui_shell::config::ui_language(cx);
    let (title, description, is_secret, allow_save) = match &request.kind {
        PromptKind::HostKey { host, port, fingerprint } => (
            language.pick("验证 SSH 主机", "Verify SSH host"),
            format!("{host}:{port}\n\n{fingerprint}\n\n{}", language.pick(
                "请通过可信渠道核对指纹。仅在确认主机身份后信任并保存。",
                "Verify this fingerprint through a trusted channel before trusting and saving it.",
            )),
            false,
            false,
        ),
        PromptKind::Secret { label, allow_save } => {
            (language.pick("SSH 身份验证", "SSH authentication"), label.clone(), true, *allow_save)
        },
    };
    let input = cx.new(|cx| InputState::new(window, cx).masked(true));
    let remember = Rc::new(Cell::new(false));
    let focus_input = input.clone();
    window.open_dialog(cx, move |dialog, window, _cx| {
        let submitted = request.clone();
        let closed = request.clone();
        let submit_input = input.clone();
        let clear_input = input.clone();
        let save = remember.clone();
        let toggle_save = remember.clone();
        let mut body = div().w_full().flex().flex_col().gap_3();
        if is_secret {
            body = body.child(Input::new(&input).mask_toggle().w_full());
        }
        if allow_save {
            body = body.child(
                Checkbox::new("ssh-remember-secret")
                    .label(language.pick("保存在系统凭据库", "Save in system credential store"))
                    .checked(remember.get())
                    .on_click(move |value, window, _| {
                        toggle_save.set(*value);
                        window.refresh();
                    }),
            );
        }
        confirm_dialog(
            dialog,
            window,
            title,
            description.clone(),
            if is_secret {
                language.pick("继续", "Continue")
            } else {
                language.pick("信任并连接", "Trust and connect")
            },
            language.pick("取消", "Cancel"),
            ButtonVariant::Primary,
        )
        .child(body)
        .on_ok(move |_, window, cx| {
            let response = if is_secret {
                PromptResponse::Secret {
                    value: zeroize::Zeroizing::new(
                        submit_input.read(cx).value().as_bytes().to_vec(),
                    ),
                    save: allow_save && save.get(),
                }
            } else {
                PromptResponse::Trust
            };
            submitted.respond(response);
            submit_input.update(cx, |input, cx| input.set_value("", window, cx));
            true
        })
        .on_close(move |_, window, cx| {
            closed.respond(PromptResponse::Cancel);
            clear_input.update(cx, |input, cx| input.set_value("", window, cx));
        })
    });
    if is_secret {
        focus_input.update(cx, |input, cx| input.focus(window, cx));
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{Context, IntoElement, Modifiers, Render, TestAppContext, point};
    use gpui_component::Root;

    struct DialogProbe;

    impl Render for DialogProbe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
            div().size_full().children(Root::render_dialog_layer(window, cx))
        }
    }

    fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).expect("SSH dialog button is visible");
        cx.simulate_click(
            point(
                bounds.origin.x + bounds.size.width * 0.5,
                bounds.origin.y + bounds.size.height * 0.5,
            ),
            Modifiers::default(),
        );
    }

    #[gpui::test]
    fn ssh_dialog_cancel_does_not_accept_an_unknown_host(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            // Keep the button hit target fixed during simulated mouse down/up.
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| DialogProbe);
            Root::new(view, window, cx)
        });
        let (request, mut response) = Prompt::for_test(PromptKind::HostKey {
            host: "example.test".into(),
            port: 22,
            fingerprint: "SHA256:verify-me".into(),
        });
        cx.update(|window, cx| {
            show(request, window, cx);
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        click(cx, "confirm-dialog-cancel");
        assert_eq!(
            response.try_recv().map(|response| matches!(response, PromptResponse::Cancel)),
            Ok(true)
        );
    }

    #[gpui::test]
    fn ssh_dialog_password_is_delivered_only_on_confirmation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            // Keep the button hit target fixed during simulated mouse down/up.
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| DialogProbe);
            Root::new(view, window, cx)
        });
        let (request, mut response) = Prompt::for_test(PromptKind::Secret {
            label: "Password for example.test".into(),
            allow_save: true,
        });
        cx.update(|window, cx| {
            show(request, window, cx);
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_input("test-secret");
        assert!(response.try_recv().is_err());
        click(cx, "confirm-dialog-ok");
        match response.try_recv() {
            Ok(PromptResponse::Secret { value, save }) => {
                assert_eq!(&*value, b"test-secret");
                assert!(!save);
            },
            _ => panic!("confirm must deliver the entered password"),
        }
    }
}

impl NebulaWorkspace {
    fn active_ready_ssh_view(&self, cx: &App) -> Option<Entity<TerminalView>> {
        if self.settings_open {
            return None;
        }
        let view = self.tabs.get(self.active)?.focused_view()?.clone();
        view.read(cx).ready_ssh_destination()?;
        Some(view)
    }

    pub(super) fn render_port_forward_button(&self, cx: &mut Context<Self>) -> Option<Button> {
        self.active_ready_ssh_view(cx)?;
        Some(Button::new("ssh-ports").icon(IconName::Network).ghost().tooltip("Ports").on_click(
            cx.listener(|this, _, window, cx| {
                this.open_port_forward_dialog(window, cx);
            }),
        ))
    }

    fn open_port_forward_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.active_ready_ssh_view(cx) else { return };
        let (destination, forwards) = {
            let view = view.read(cx);
            let Some(destination) = view.ready_ssh_destination() else { return };
            (
                destination.to_owned(),
                view.port_forwards
                    .iter()
                    .map(|forward| (forward.local_port(), forward.remote_port()))
                    .collect::<Vec<_>>(),
            )
        };
        let remote_input = cx.new(|cx| InputState::new(window, cx).placeholder("3000"));
        let local_input = cx.new(|cx| InputState::new(window, cx).placeholder("3000"));
        let focus_input = remote_input.clone();
        let target = view.downgrade();
        let language = workspace_ui_language();

        window.open_dialog(cx, move |dialog, window, _cx| {
            let mut body = v_flex().w_full().gap_3();
            if forwards.is_empty() {
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(_cx.theme().muted_foreground)
                        .child(language.pick("暂无端口转发", "No forwarded ports")),
                );
            } else {
                for (index, (local_port, remote_port)) in forwards.iter().copied().enumerate() {
                    let stop_target = target.clone();
                    body = body.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(format!("127.0.0.1:{local_port} → 127.0.0.1:{remote_port}"))
                            .child(
                                Button::new(("ssh-port-stop", index))
                                    .icon(IconName::Close)
                                    .ghost()
                                    .xsmall()
                                    .tooltip(language.pick("停止转发", "Stop forwarding"))
                                    .on_click(move |_, window, cx| {
                                        if let Some(view) = stop_target.upgrade() {
                                            view.update(cx, |view, cx| {
                                                if index < view.port_forwards.len() {
                                                    view.port_forwards.remove(index);
                                                    cx.notify();
                                                }
                                            });
                                        }
                                        window.close_dialog(cx);
                                    }),
                            ),
                    );
                }
            }
            body = body.child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(language.pick("服务器端口", "Remote port"))
                            .child(Input::new(&remote_input)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(language.pick("本地端口", "Local port"))
                            .child(Input::new(&local_input)),
                    ),
            );

            let submit_target = target.clone();
            let submit_remote = remote_input.clone();
            let submit_local = local_input.clone();
            let submit_destination = destination.clone();
            confirm_dialog(
                dialog,
                window,
                language.pick("SSH 端口转发", "SSH Ports"),
                destination.clone(),
                language.pick("转发", "Forward"),
                language.pick("取消", "Cancel"),
                ButtonVariant::Primary,
            )
            .child(body)
            .on_ok(move |_, window, cx| {
                let remote_port = submit_remote
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0);
                let local_port = submit_local
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0);
                let (Some(remote_port), Some(local_port)) = (remote_port, local_port) else {
                    crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::display::ToastKind::Warning,
                        language.pick("请输入 1–65535 的端口", "Enter ports from 1–65535"),
                    );
                    return false;
                };

                let weak = submit_target.clone();
                let expected_destination = submit_destination.clone();
                let work_destination = submit_destination.clone();
                let window_handle = window.window_handle();
                cx.spawn(async move |cx| {
                    let result = super::remote_files::remote_call(move || async move {
                        crate::ssh_session::open_local_forward(
                            &work_destination,
                            local_port,
                            remote_port,
                        )
                        .await
                    })
                    .await;
                    let _ = window_handle.update(cx, move |_, window, cx| match result {
                        Some(Ok(forward)) => {
                            if let Some(view) = weak.upgrade()
                                && view.read(cx).ready_ssh_destination()
                                    == Some(expected_destination.as_str())
                            {
                                view.update(cx, |view, cx| {
                                    view.port_forwards.push(forward);
                                    cx.notify();
                                });
                                crate::gpui_shell::toast::toast(
                                    window,
                                    cx,
                                    crate::display::ToastKind::Info,
                                    format!(
                                        "127.0.0.1:{local_port} → {expected_destination}:{remote_port}"
                                    ),
                                );
                            }
                        },
                        Some(Err(error)) => crate::gpui_shell::toast::toast(
                            window,
                            cx,
                            crate::display::ToastKind::Warning,
                            error.to_string(),
                        ),
                        None => crate::gpui_shell::toast::toast(
                            window,
                            cx,
                            crate::display::ToastKind::Warning,
                            language.pick(
                                "SSH 网络运行时不可用",
                                "SSH network runtime unavailable",
                            ),
                        ),
                    });
                })
                .detach();
                true
            })
        });
        focus_input.update(cx, |input, cx| input.focus(window, cx));
    }
}
