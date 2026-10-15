//! Real SSH events and stages consumed by their owning GPUI test foreground.
//!
//! Native threads cannot wake the pinned TestScheduler. Explicitly opted-in
//! fixtures poll both production mailboxes without registering a task waker;
//! the real SSH worker, EventProxy, grid and view event handlers remain intact.

use super::*;
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use gpui::{App, Global, WeakEntity};

type Receivers =
    (super::super::event_mailbox::EventReceiver, UnboundedReceiver<crate::ssh_session::SshStage>);

struct Owner {
    view: WeakEntity<TerminalView>,
    events: Option<super::super::event_mailbox::EventReceiver>,
    stages: Option<UnboundedReceiver<crate::ssh_session::SshStage>>,
}

#[derive(Default)]
pub(crate) struct SshSessionPump {
    owners: Vec<Owner>,
}

impl Global for SshSessionPump {}

impl SshSessionPump {
    pub(super) fn attach(
        events: super::super::event_mailbox::EventReceiver,
        stages: UnboundedReceiver<crate::ssh_session::SshStage>,
        is_ssh: bool,
        cx: &mut Context<TerminalView>,
    ) -> Result<(), Receivers> {
        if !is_ssh || !cx.has_global::<Self>() {
            return Err((events, stages));
        }
        let view = cx.entity().downgrade();
        cx.global_mut::<Self>().owners.push(Owner {
            view,
            events: Some(events),
            stages: Some(stages),
        });
        Ok(())
    }

    pub(crate) fn drain(cx: &mut App) -> (usize, usize) {
        let mut owners = std::mem::take(&mut cx.global_mut::<Self>().owners);
        let (mut events, mut stages) = (0, 0);
        owners.retain_mut(|owner| {
            let view = owner.view.clone();
            let result = view.update(cx, |view, cx| {
                for _ in 0..128 {
                    let Some(receiver) = &mut owner.stages else { break };
                    match receiver.try_recv() {
                        Ok(stage) => {
                            stages += 1;
                            view.apply_ssh_stage(stage, cx);
                        },
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => {
                            owner.stages = None;
                            break;
                        },
                    }
                }
                for _ in 0..128 {
                    let Some(receiver) = &mut owner.events else { break };
                    match receiver.try_recv() {
                        Ok(event) => {
                            events += 1;
                            let exited = matches!(event, TermEvent::Exit);
                            view.process_event(event, cx);
                            if exited {
                                owner.events = None;
                                break;
                            }
                        },
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Closed) => {
                            owner.events = None;
                            break;
                        },
                    }
                }
            });
            result.is_ok() && (owner.events.is_some() || owner.stages.is_some())
        });
        // Events may attach a successor while these owners are being updated.
        cx.global_mut::<Self>().owners.extend(owners);
        (events, stages)
    }
}
