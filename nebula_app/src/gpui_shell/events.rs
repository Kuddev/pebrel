//! Cross-thread shell events wake GPUI directly. A bounded queue replaces the
//! old 120 ms polling timer; no worker thread or idle timer is needed.
use super::GpuiShellEvent;
use futures::{StreamExt, channel::mpsc};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct Sender(Arc<Mutex<mpsc::Sender<GpuiShellEvent>>>);
pub(crate) struct Receiver(mpsc::Receiver<GpuiShellEvent>);

pub(crate) fn channel() -> (Sender, Receiver) {
    let (sender, receiver) = mpsc::channel(256);
    (Sender(Arc::new(Mutex::new(sender))), Receiver(receiver))
}

impl Sender {
    pub(crate) fn send(&self, event: GpuiShellEvent) -> Result<(), ()> {
        let mut sender = self.0.lock().unwrap_or_else(|error| error.into_inner());
        sender.try_send(event).map_err(|error| {
            // A caller must never mistake a full command queue for delivery.
            if let GpuiShellEvent::RuntimeControl(dispatch) = error.into_inner() {
                dispatch.respond(Err(crate::runtime_api::ApiError::new(
                    "runtime_busy",
                    "runtime command queue is unavailable",
                )));
            }
        })
    }
}

impl Receiver {
    pub(crate) fn try_recv(&mut self) -> Result<GpuiShellEvent, mpsc::TryRecvError> {
        self.0.try_recv()
    }

    pub(crate) async fn next_batch(&mut self) -> Option<Vec<GpuiShellEvent>> {
        let first = self.0.next().await?;
        let mut batch = vec![first];
        while batch.len() < 64 {
            let Ok(event) = self.try_recv() else { break };
            batch.push(event);
        }
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::task::{ArcWake, Context, Poll, waker_ref};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct WakeCount(AtomicUsize);
    impl ArcWake for WakeCount {
        fn wake_by_ref(value: &Arc<Self>) {
            value.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn mobile_latency_queued_input_wakes_without_advancing_a_clock() {
        let (sender, mut receiver) = channel();
        let wake = Arc::new(WakeCount::default());
        let waker = waker_ref(&wake);
        let mut cx = Context::from_waker(&waker);
        let mut next = Box::pin(receiver.next_batch());
        assert!(matches!(std::future::Future::poll(next.as_mut(), &mut cx), Poll::Pending));
        sender.send(GpuiShellEvent::TrayFocus(Some(1))).unwrap();
        sender.send(GpuiShellEvent::TrayFocus(Some(2))).unwrap();
        assert!(wake.0.load(Ordering::Relaxed) > 0);
        let Poll::Ready(Some(events)) = std::future::Future::poll(next.as_mut(), &mut cx) else {
            panic!("queued events must not wait for a timer");
        };
        assert!(matches!(
            events.as_slice(),
            [GpuiShellEvent::TrayFocus(Some(1)), GpuiShellEvent::TrayFocus(Some(2))]
        ));
    }

    #[test]
    fn mobile_latency_queue_and_dispatch_batch_are_bounded_and_close_cleanly() {
        let (sender, mut receiver) = channel();
        let mut accepted = 0;
        while sender.send(GpuiShellEvent::MuxAttach).is_ok() {
            accepted += 1;
        }
        assert!(accepted <= 257);
        assert_eq!(futures::executor::block_on(receiver.next_batch()).unwrap().len(), 64);
        drop(sender);
        while futures::executor::block_on(receiver.next_batch()).is_some() {}
    }
}
