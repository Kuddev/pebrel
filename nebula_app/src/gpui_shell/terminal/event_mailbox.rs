//! Coalesce repaint wakeups before enqueueing; retain every semantic event.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::{Stream, channel::mpsc};
use nebula_terminal::event::Event;

#[derive(Clone)]
pub(super) struct EventSender {
    sender: mpsc::UnboundedSender<Event>,
    wake_pending: Arc<AtomicBool>,
    pub(super) native_prompt: NativePromptState,
}

#[derive(Clone, Default)]
pub(super) struct NativePromptState(Arc<Mutex<NativePrompt>>);

/// PTY 接收与用户输入共用的边界；队列消费只唤醒视图，不重新给旧标记盖章。
#[derive(Clone, Copy, Default)]
pub(super) struct NativePrompt {
    pub seen: bool,
    pub input_epoch: u64,
    pub pending: bool,
}

pub(super) struct EventReceiver {
    receiver: mpsc::UnboundedReceiver<Event>,
    wake_pending: Arc<AtomicBool>,
}

pub(super) fn channel() -> (EventSender, EventReceiver) {
    let (sender, receiver) = mpsc::unbounded();
    let wake_pending = Arc::new(AtomicBool::new(false));
    (
        EventSender {
            sender,
            wake_pending: wake_pending.clone(),
            native_prompt: NativePromptState::default(),
        },
        EventReceiver { receiver, wake_pending },
    )
}

/// Editing a CMD input line does not invalidate a prompt already received from
/// the PTY. Submission and unknown control sequences remain conservative.
pub(super) fn preserves_native_prompt(mut bytes: &[u8]) -> bool {
    while let Some((&byte, rest)) = bytes.split_first() {
        if byte != 0x1b {
            if byte.is_ascii_control() && !matches!(byte, 8 | 9 | 127) {
                return false;
            }
            bytes = rest;
            continue;
        }
        let Some(sequence) = bytes.strip_prefix(b"\x1b[") else { return false };
        let Some(end) = sequence.iter().position(|byte| (0x40..=0x7e).contains(byte)) else {
            return false;
        };
        let parameters = &sequence[..end];
        match sequence[end] {
            b'A'..=b'D' | b'H' | b'F' if parameters.is_empty() => {},
            b'~' if matches!(parameters, b"1" | b"2" | b"3" | b"4" | b"5" | b"6") => {},
            b'_' => {
                let Ok(parameters) = std::str::from_utf8(parameters) else { return false };
                let mut fields = parameters.split(';');
                let mut values = [0u16; 6];
                for value in &mut values {
                    let Some(parsed) = fields.next().and_then(|field| field.parse().ok()) else {
                        return false;
                    };
                    *value = parsed;
                }
                if fields.next().is_some() || values[3] > 1 {
                    return false;
                }
                // Vk;Sc;Uc;Kd;Cs;Rc: Enter is a boundary even if Uc is zero.
                if values[0] == 13 || (values[2] < 32 && !matches!(values[2], 0 | 8 | 9 | 27)) {
                    return false;
                }
            },
            _ => return false,
        }
        bytes = &sequence[end + 1..];
    }
    true
}

impl NativePromptState {
    pub(super) fn snapshot(&self) -> NativePrompt {
        *self.0.lock().unwrap()
    }

    pub(super) fn observe_input(&self, input_epoch: u64, preserves_prompt: bool) {
        let mut prompt = self.0.lock().unwrap();
        prompt.input_epoch = input_epoch;
        prompt.pending &= preserves_prompt;
    }

    pub(super) fn consume_native_prompt(&self) {
        self.0.lock().unwrap().pending = false;
    }

    pub(super) fn observe_prompt(&self) {
        let mut prompt = self.0.lock().unwrap();
        prompt.seen = true;
        prompt.pending = true;
    }
}

impl EventSender {
    pub(super) fn send(&self, event: Event) {
        if matches!(&event, Event::UserVar { name, value }
            if name == "pebrel_cmd_prompt" && value == "1")
        {
            self.native_prompt.observe_prompt();
        }
        let wake = matches!(event, Event::Wakeup);
        if wake && self.wake_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.sender.unbounded_send(event).is_err() && wake {
            self.wake_pending.store(false, Ordering::Release);
        }
    }
}

impl EventReceiver {
    fn acknowledge(&self, event: &Event) {
        if matches!(event, Event::Wakeup) {
            // Clear before processing: output arriving during UI work must
            // still enqueue a subsequent repaint, even on an inactive tab.
            self.wake_pending.store(false, Ordering::Release);
        }
    }

    pub(super) fn try_recv(&mut self) -> Result<Event, mpsc::TryRecvError> {
        let event = self.receiver.try_recv()?;
        self.acknowledge(&event);
        Ok(event)
    }
}

impl Stream for EventReceiver {
    type Item = Event;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        let result = Pin::new(&mut self.receiver).poll_next(cx);
        if let Poll::Ready(Some(event)) = &result {
            self.acknowledge(event);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eighty_sessions_keep_all_agent_edges_through_a_repaint_flood() {
        let mut sessions: Vec<_> = (0..80).map(|_| channel()).collect();
        for (id, (sender, _)) in sessions.iter().enumerate() {
            sender.send(Event::CommandStart);
            for _ in 0..1000 {
                sender.send(Event::Wakeup);
            }
            sender.send(Event::AiHookEnvelope(format!("working:{id}").into_bytes()));
            sender.send(Event::AiHookEnvelope(format!("attention:{id}").into_bytes()));
            sender.send(Event::AiHookEnvelope(format!("done:{id}").into_bytes()));
            sender.send(Event::CommandDone { exit_code: Some(0) });
        }
        for (id, (sender, receiver)) in sessions.iter_mut().enumerate() {
            assert!(matches!(receiver.try_recv().unwrap(), Event::CommandStart));
            assert!(matches!(receiver.try_recv().unwrap(), Event::Wakeup));
            for edge in ["working", "attention", "done"] {
                let Event::AiHookEnvelope(bytes) = receiver.try_recv().unwrap() else {
                    panic!("lost agent edge")
                };
                assert_eq!(bytes, format!("{edge}:{id}").into_bytes());
            }
            assert!(matches!(
                receiver.try_recv().unwrap(),
                Event::CommandDone { exit_code: Some(0) }
            ));
            assert!(receiver.try_recv().is_err());
            sender.send(Event::Wakeup);
            assert!(matches!(receiver.try_recv().unwrap(), Event::Wakeup));
        }
    }

    #[test]
    fn sender_clones_share_wakeup_state_but_retain_exit() {
        let (sender, mut receiver) = channel();
        sender.send(Event::Wakeup);
        sender.clone().send(Event::Wakeup);
        sender.send(Event::Exit);
        assert!(matches!(receiver.try_recv().unwrap(), Event::Wakeup));
        assert!(matches!(receiver.try_recv().unwrap(), Event::Exit));
        assert!(receiver.try_recv().is_err());
    }
}
