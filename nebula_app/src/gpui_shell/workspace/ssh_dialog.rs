use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, ParentElement as _,
    Styled as _, Window, div,
};

use crate::gpui_shell::prelude::*;
use crate::gpui_shell::terminal::view::TerminalView;
use crate::i18n::Message;
use crate::ssh_prompt::{Prompt, PromptKind, PromptResponse};
use crate::ssh_session::ForwardKind;

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

    struct TransferredPortsProbe {
        terminal: Entity<TerminalView>,
        render_terminal: bool,
    }

    impl Render for TransferredPortsProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            div().size_full().children(self.render_terminal.then(|| self.terminal.clone()))
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

    fn press(cx: &mut gpui::VisualTestContext, key: &str) {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
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
    #[gpui::test]
    fn ports_dialog_rejects_invalid_input_and_cancel_releases_focus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| DialogProbe), window, cx));
        let owner = cx.update(|window, cx| {
            cx.new(|cx| {
                TerminalView::new(
                    42,
                    (80, 24),
                    crate::gpui_shell::terminal::view::TerminalLaunch::Local {
                        cwd: None,
                        shell: Some(nebula_terminal::tty::Shell::new(
                            "pebrel-test-missing-shell-executable".into(),
                            vec![],
                        )),
                        shell_name: None,
                    },
                    window,
                    cx,
                )
            })
        });
        let inputs = cx.update(|window, cx| {
            show_port_forward_dialog(owner.clone(), "fixture@localhost".into(), window, cx)
        });
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        cx.simulate_input("65536");
        assert_eq!(inputs[0].read_with(cx, |input, _| input.value()).as_ref(), "65536");
        click(cx, "confirm-dialog-ok");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(
            cx.debug_bounds("confirm-dialog-ok").is_some(),
            "invalid input keeps the dialog open"
        );
        owner.read_with(cx, |view, _| {
            assert!(view.port_forward_task.is_none());
            assert!(view.port_forwards.is_empty());
        });
        let select_all = match crate::platform::Platform::current() {
            crate::platform::Platform::MacOS => "cmd-a",
            _ => "ctrl-a",
        };
        for (index, selector) in ["ssh-forward-remote", "ssh-forward-local"].into_iter().enumerate()
        {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(f32::from(bounds.size.height) >= 28.0);
            assert!(f32::from(bounds.size.width) >= 100.0);
            click(cx, selector);
            cx.simulate_keystrokes(select_all);
            cx.simulate_input("3000");
            assert_eq!(
                inputs[index].read_with(cx, |input, _| input.value()).as_ref(),
                "3000",
                "clicking {selector} must focus and edit that field"
            );
        }
        click(cx, "confirm-dialog-ok");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(
            cx.debug_bounds("confirm-dialog-ok").is_none(),
            "valid ports close the dialog when its pane is no longer ready"
        );
        owner.read_with(cx, |view, _| assert!(view.port_forward_task.is_none()));
        cx.update(|window, cx| {
            show_port_forward_dialog(owner.clone(), "fixture@localhost".into(), window, cx)
        });
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        click(cx, "confirm-dialog-cancel");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("confirm-dialog-ok").is_none());
    }
    #[gpui::test]
    fn ports_modes_use_real_clicks_and_dynamic_ignores_the_hidden_remote_input(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| DialogProbe), window, cx));
        let owner = cx.update(|window, cx| {
            cx.new(|cx| {
                TerminalView::new(
                    42,
                    (80, 24),
                    crate::gpui_shell::terminal::view::TerminalLaunch::Local {
                        cwd: None,
                        shell: Some(nebula_terminal::tty::Shell::new(
                            "pebrel-test-missing-shell-executable".into(),
                            vec![],
                        )),
                        shell_name: None,
                    },
                    window,
                    cx,
                )
            })
        });
        let inputs = cx.update(|window, cx| {
            show_port_forward_dialog(owner.clone(), "fixture@localhost".into(), window, cx)
        });
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        for selector in
            ["ssh-forward-mode-local", "ssh-forward-mode-remote", "ssh-forward-mode-dynamic"]
        {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(f32::from(bounds.size.height) >= 28.0);
            assert!(f32::from(bounds.size.width) >= 90.0);
        }
        // Focus starts in the remote input. Tab traversal reaches actual mode
        // buttons; Space/Enter must select modes without confirming the dialog.
        press(cx, "shift-tab");
        press(cx, "ctrl-space");
        assert!(cx.debug_bounds("ssh-forward-remote").is_some());
        press(cx, "space");
        assert!(cx.debug_bounds("ssh-forward-remote").is_none());
        assert!(cx.debug_bounds("confirm-dialog-ok").is_some());
        click(cx, "ssh-forward-local");
        press(cx, "shift-tab");
        press(cx, "shift-tab");
        press(cx, "enter");
        assert!(cx.debug_bounds("ssh-forward-remote").is_some());
        assert!(cx.debug_bounds("confirm-dialog-ok").is_some());
        click(cx, "ssh-forward-mode-remote");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("ssh-forward-remote").is_some());
        let original_width = cx.debug_bounds("ssh-forward-local").unwrap().size.width;
        click(cx, "ssh-forward-mode-dynamic");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("ssh-forward-remote").is_none());
        assert!(cx.debug_bounds("ssh-forward-local").unwrap().size.width > original_width);
        click(cx, "ssh-forward-local");
        cx.simulate_input("1080");
        assert_eq!(inputs[1].read_with(cx, |input, _| input.value()).as_ref(), "1080");
        assert!(inputs[0].read_with(cx, |input, _| input.value()).is_empty());
        click(cx, "confirm-dialog-ok");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(
            cx.debug_bounds("confirm-dialog-ok").is_none(),
            "dynamic validates only its visible local port"
        );
        owner.read_with(cx, |view, _| {
            assert!(view.port_forward_task.is_none());
            assert!(view.port_forwards.is_empty());
        });
    }

    #[gpui::test]
    fn pending_ports_task_finishes_after_its_source_window_closes(cx: &mut TestAppContext) {
        cx.update(|cx| gpui_component::init(cx));
        let (_, source) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| DialogProbe), window, cx));
        let mut source = source.clone();
        let owner = source.update(|window, cx| {
            cx.new(|cx| {
                TerminalView::new(
                    42,
                    (80, 24),
                    crate::gpui_shell::terminal::view::TerminalLaunch::Local {
                        cwd: None,
                        shell: Some(nebula_terminal::tty::Shell::new(
                            "pebrel-test-missing-shell-executable".into(),
                            vec![],
                        )),
                        shell_name: None,
                    },
                    window,
                    cx,
                )
            })
        });
        let (result, delivered) = futures::channel::oneshot::channel();
        source.update(|_, cx| {
            start_port_forward_task(
                &owner,
                "fixture@localhost".into(),
                (ForwardKind::Local, 3000, 3001),
                crate::gpui_shell::config::ui_language(cx),
                async move { delivered.await.ok() },
                cx,
            );
        });
        assert!(owner.read_with(&source, |view, _| view.port_forward_task.is_some()));
        // add_window_view draws immediately. Keep ownership in the destination
        // root while deliberately deferring the terminal's first render.
        let holder =
            cx.new(|_| TransferredPortsProbe { terminal: owner.clone(), render_terminal: false });
        let (_, destination) =
            cx.add_window_view(|window, cx| Root::new(holder.clone(), window, cx));
        let mut destination = destination.clone();
        let source_window = source.update(|window, _| {
            let handle = window.window_handle();
            window.remove_window();
            handle
        });
        assert!(!cx.windows().contains(&source_window));
        assert!(cx.update(|cx| source_window.update(cx, |_, _, _| ())).is_err());
        assert!(
            cx.update(|cx| cx.with_window(owner.entity_id(), |window, _| window.window_handle()))
                .is_none()
        );
        result.send(Err("synthetic registration refusal".into())).ok().unwrap();
        destination.run_until_parked();
        owner.read_with(&destination, |view, _| {
            assert!(
                view.port_forward_task.is_none(),
                "entity completion must release pending state before destination draw"
            );
            assert!(view.port_forwards.is_empty());
        });
        destination.update(|window, cx| {
            holder.update(cx, |holder, cx| {
                holder.render_terminal = true;
                cx.notify();
            });
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let destination_window = destination.update(|window, _| window.window_handle());
        assert_eq!(
            cx.update(|cx| cx.with_window(owner.entity_id(), |window, _| window.window_handle())),
            Some(destination_window)
        );
        destination.update(|_, cx| {
            start_port_forward_task(
                &owner,
                "fixture@localhost".into(),
                (ForwardKind::Dynamic, 1080, 0),
                crate::gpui_shell::config::ui_language(cx),
                async { None },
                cx,
            )
        });
        destination.run_until_parked();
        assert!(owner.read_with(&destination, |view, _| view.port_forward_task.is_none()));
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
        let view = self.active_ready_ssh_view(cx)?;
        let pending = view.read(cx).port_forward_task.is_some();
        let language = workspace_ui_language();
        Some(
            Button::new("ssh-ports")
                .icon(IconName::Network)
                .ghost()
                .disabled(pending)
                .tooltip(language.text(if pending {
                    Message::SshPortsPending
                } else {
                    Message::SshPortsButton
                }))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_port_forward_dialog(window, cx);
                })),
        )
    }

    fn open_port_forward_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.active_ready_ssh_view(cx) else { return };
        if view.read(cx).port_forward_task.is_some() {
            return;
        }
        let destination = view.read(cx).ready_ssh_destination().unwrap().to_owned();
        show_port_forward_dialog(view, destination, window, cx);
    }
}

fn show_port_forward_dialog(
    view: Entity<TerminalView>,
    destination: String,
    window: &mut Window,
    cx: &mut App,
) -> [Entity<InputState>; 2] {
    let forwards = Rc::new(
        view.read(cx)
            .port_forwards
            .iter()
            .map(|forward| {
                (forward.id(), forward.kind(), forward.local_port(), forward.remote_port())
            })
            .collect::<Vec<_>>(),
    );
    let remote_input = cx.new(|cx| InputState::new(window, cx).placeholder("3000"));
    let local_input = cx.new(|cx| InputState::new(window, cx).placeholder("3000"));
    let focus_input = remote_input.clone();
    let inputs = [remote_input.clone(), local_input.clone()];
    let target = view.downgrade();
    let language = workspace_ui_language();
    let kind = Rc::new(Cell::new(ForwardKind::Local));

    window.open_dialog(cx, move |dialog, window, _cx| {
        let submit_target = target.clone();
        let submit_remote = remote_input.clone();
        let submit_local = local_input.clone();
        let submit_destination = destination.clone();
        let content_target = target.clone();
        let content_forwards = forwards.clone();
        let content_remote = remote_input.clone();
        let content_local = local_input.clone();
        let content_destination = destination.clone();
        let submit_kind = kind.clone();
        let content_kind = kind.clone();
        confirm_dialog(
            dialog,
            window,
            language.text(Message::SshPortsTitle),
            destination.clone(),
            language.text(Message::SshPortsForward),
            language.text(Message::CommonCancel),
            ButtonVariant::Primary,
        )
        .content(move |content, _, _cx| {
            let mut body = v_flex().w_full().gap_3();
            if content_forwards.is_empty() {
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(_cx.theme().muted_foreground)
                        .child(language.text(Message::SshPortsEmpty)),
                );
            } else {
                for (index, (id, kind, local_port, remote_port)) in
                    content_forwards.iter().copied().enumerate()
                {
                    let stop_target = content_target.clone();
                    body = body.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(forward_label(language, kind, local_port, remote_port))
                            .child(
                                Button::new(("ssh-port-stop", index))
                                    .icon(IconName::Close)
                                    .ghost()
                                    .xsmall()
                                    .tooltip(language.text(Message::SshPortsStop))
                                    .on_click(move |_, window, cx| {
                                        if let Some(view) = stop_target.upgrade() {
                                            view.update(cx, |view, cx| {
                                                if let Some(position) = view
                                                    .port_forwards
                                                    .iter()
                                                    .position(|forward| forward.id() == id)
                                                {
                                                    view.port_forwards.remove(position);
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
            let mut modes = h_flex().w_full().gap_2();
            for (mode, message, selector) in [
                (ForwardKind::Local, Message::SshPortsModeLocal, "ssh-forward-mode-local"),
                (ForwardKind::Remote, Message::SshPortsModeRemote, "ssh-forward-mode-remote"),
                (ForwardKind::Dynamic, Message::SshPortsModeDynamic, "ssh-forward-mode-dynamic"),
            ] {
                let selected = content_kind.clone();
                let keyboard_selected = content_kind.clone();
                let enter_selected = content_kind.clone();
                modes = modes.child(
                    Button::new(selector)
                        .debug_selector(move || selector.to_owned())
                        .flex_1()
                        .label(language.text(message))
                        .selected(content_kind.get() == mode)
                        .on_key_down(move |event, window, cx| {
                            if event.keystroke.key == "space"
                                && !event.keystroke.modifiers.modified()
                            {
                                keyboard_selected.set(mode);
                                window.refresh();
                                cx.stop_propagation();
                            }
                        })
                        // Enter is a Dialog action; handle it at the focused
                        // mode button so selecting a mode does not submit.
                        .on_action(move |_: &gpui_component::dialog::ConfirmDialog, window, cx| {
                            enter_selected.set(mode);
                            window.refresh();
                            cx.stop_propagation();
                        })
                        .on_click(move |_, window, _| {
                            selected.set(mode);
                            window.refresh();
                        }),
                );
            }
            body = body.child(modes);
            if content_kind.get() == ForwardKind::Dynamic {
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(_cx.theme().muted_foreground)
                        .child(language.text(Message::SshPortsDynamicHelp)),
                );
            }
            body = body.child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .when(content_kind.get() != ForwardKind::Dynamic, |row| {
                        row.child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(language.text(Message::SshPortsRemote))
                                .child(
                                    div()
                                        .debug_selector(|| "ssh-forward-remote".to_owned())
                                        .w_full()
                                        .h_8()
                                        .flex_shrink_0()
                                        .child(Input::new(&content_remote).w_full()),
                                ),
                        )
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(language.text(Message::SshPortsLocal))
                            .child(
                                div()
                                    .debug_selector(|| "ssh-forward-local".to_owned())
                                    .w_full()
                                    .h_8()
                                    .flex_shrink_0()
                                    .child(Input::new(&content_local).w_full()),
                            ),
                    ),
            );
            content
                .child(
                    gpui_component::dialog::DialogDescription::new()
                        .child(content_destination.clone()),
                )
                .child(body)
        })
        .on_ok(move |_, window, cx| {
            let kind = submit_kind.get();
            let remote_port = if kind == ForwardKind::Dynamic {
                Some(0)
            } else {
                submit_remote.read(cx).value().trim().parse::<u16>().ok().filter(|port| *port != 0)
            };
            let local_port =
                submit_local.read(cx).value().trim().parse::<u16>().ok().filter(|port| *port != 0);
            let (Some(remote_port), Some(local_port)) = (remote_port, local_port) else {
                crate::gpui_shell::toast::toast(
                    window,
                    cx,
                    crate::display::ToastKind::Warning,
                    language.text(Message::SshPortsInvalid),
                );
                return false;
            };

            let Some(owner) = submit_target.upgrade() else {
                return true;
            };
            if owner.read(cx).ready_ssh_destination() != Some(submit_destination.as_str()) {
                return true;
            }
            if owner.read(cx).port_forward_task.is_some() {
                return false;
            }
            let Some(forwarder) = owner.read(cx).port_forwarder() else {
                return true;
            };
            start_port_forward_task(
                &owner,
                submit_destination.clone(),
                (kind, local_port, remote_port),
                language,
                super::remote_files::remote_call(move || async move {
                    forwarder.open(kind, local_port, remote_port).await
                }),
                cx,
            );
            true
        })
    });
    focus_input.update(cx, |input, cx| input.focus(window, cx));
    inputs
}

fn start_port_forward_task(
    owner: &Entity<TerminalView>,
    destination: String,
    (kind, local, remote): (ForwardKind, u16, u16),
    language: crate::i18n::UiLanguage,
    result: impl std::future::Future<
        Output = Option<
            Result<crate::ssh_session::PortForward, Box<dyn std::error::Error + Send + Sync>>,
        >,
    > + 'static,
    cx: &mut App,
) {
    let weak = owner.downgrade();
    let task = cx.spawn(async move |cx| {
        let result = result.await;
        cx.update(move |cx| {
            let Some(owner) = weak.upgrade() else { return };
            let notice = owner.update(cx, |view, cx| {
                // This entity may have moved while the transport request was pending.
                view.port_forward_task = None;
                cx.notify();
                if view.ready_ssh_destination() != Some(destination.as_str()) {
                    return None;
                }
                match result {
                    Some(Ok(forward)) if forward.is_active() => {
                        view.port_forwards.push(forward);
                        Some((
                            crate::display::ToastKind::Info,
                            language.format(
                                match kind {
                                    ForwardKind::Local => Message::SshPortsStarted,
                                    ForwardKind::Remote => Message::SshPortsStartedRemote,
                                    ForwardKind::Dynamic => Message::SshPortsStartedDynamic,
                                },
                                &[
                                    ("local", &local.to_string()),
                                    ("host", &destination),
                                    ("remote", &remote.to_string()),
                                ],
                            ),
                        ))
                    },
                    Some(Ok(_)) => None,
                    Some(Err(error)) => {
                        Some((crate::display::ToastKind::Warning, error.to_string()))
                    },
                    None => Some((
                        crate::display::ToastKind::Warning,
                        language.text(Message::SshPortsUnavailable).to_owned(),
                    )),
                }
            });
            if let Some((kind, message)) = notice {
                cx.with_window(owner.entity_id(), |window, cx| {
                    crate::gpui_shell::toast::toast(window, cx, kind, message);
                });
            }
        });
    });
    owner.update(cx, |view, cx| {
        view.port_forward_task = Some(task);
        cx.notify();
    });
}

fn forward_label(
    language: crate::i18n::UiLanguage,
    kind: ForwardKind,
    local: u16,
    remote: u16,
) -> String {
    match kind {
        ForwardKind::Local => format!(
            "{}  127.0.0.1:{local} → 127.0.0.1:{remote}",
            language.text(Message::SshPortsModeLocal)
        ),
        ForwardKind::Remote => format!(
            "{}  127.0.0.1:{remote} → 127.0.0.1:{local}",
            language.text(Message::SshPortsModeRemote)
        ),
        ForwardKind::Dynamic => {
            format!("{}  SOCKS5 127.0.0.1:{local}", language.text(Message::SshPortsModeDynamic))
        },
    }
}
