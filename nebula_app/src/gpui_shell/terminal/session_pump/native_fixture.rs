//! A real PTY mailbox polled by its owning GPUI test foreground.
//!
//! The pinned TestScheduler rejects native-thread wakes. Opted-in fixtures keep
//! the production EventProxy/mailbox/parser and process_event authority, without
//! registering a foreground task waker on the native PTY reader thread.

use super::*;
use gpui::{App, Global, WeakEntity};

#[derive(Default)]
pub(crate) struct LocalPtyPump {
    receivers: Vec<(WeakEntity<TerminalView>, super::super::event_mailbox::EventReceiver)>,
}

impl Global for LocalPtyPump {}

impl LocalPtyPump {
    pub(super) fn attach(
        rx: super::super::event_mailbox::EventReceiver,
        is_ssh: bool,
        cx: &mut Context<TerminalView>,
    ) -> Result<(), super::super::event_mailbox::EventReceiver> {
        if is_ssh || !cx.has_global::<Self>() {
            return Err(rx);
        }
        let owner = cx.entity().downgrade();
        cx.global_mut::<Self>().receivers.push((owner, rx));
        Ok(())
    }

    pub(crate) fn drain(cx: &mut App) {
        let mut receivers = std::mem::take(&mut cx.global_mut::<Self>().receivers);
        receivers.retain_mut(|(owner, rx)| {
            let mut exited = false;
            let result = owner.update(cx, |view, cx| {
                for _ in 0..128 {
                    let Ok(event) = rx.try_recv() else { break };
                    exited |= matches!(event, TermEvent::Exit);
                    view.process_event(event, cx);
                }
            });
            result.is_ok() && !exited
        });
        cx.global_mut::<Self>().receivers.extend(receivers);
    }
}
