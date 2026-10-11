//! Explicit foreground delivery for real network channels in deterministic GPUI tests.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

type Waiters = Arc<Mutex<Option<Vec<Waker>>>>;

thread_local! {
    static ACTIVE: RefCell<Option<Waiters>> = const { RefCell::new(None) };
}

pub(in crate::gpui_shell::workspace) struct NetworkPump {
    waiters: Waiters,
    _same_thread: PhantomData<Rc<()>>,
}

impl NetworkPump {
    pub(in crate::gpui_shell::workspace) fn new() -> Self {
        let waiters = Arc::new(Mutex::new(Some(Vec::new())));
        ACTIVE.with_borrow_mut(|active| {
            assert!(active.is_none(), "network fixture scopes must not overlap");
            *active = Some(waiters.clone());
        });
        Self { waiters, _same_thread: PhantomData }
    }

    pub(super) fn current() -> Option<Waiters> {
        ACTIVE.with_borrow(Clone::clone)
    }

    pub(in crate::gpui_shell::workspace) fn tick() {
        if let Some(waiters) = Self::current() {
            let pending = waiters.lock().unwrap().as_mut().map(std::mem::take).unwrap_or_default();
            pending.into_iter().for_each(Waker::wake);
        }
    }

    async fn wait<T>(waiters: Waiters, mut receive: impl FnMut() -> Poll<Option<T>>) -> Option<T> {
        std::future::poll_fn(|cx| {
            let mut state = waiters.lock().unwrap();
            let Some(pending) = state.as_mut() else {
                return Poll::Ready(None);
            };
            let result = receive();
            if result.is_pending() && !pending.iter().any(|waker| waker.will_wake(cx.waker())) {
                pending.push(cx.waker().clone());
            }
            result
        })
        .await
    }

    pub(super) async fn oneshot<T>(
        mut receiver: tokio::sync::oneshot::Receiver<T>,
        waiters: Waiters,
    ) -> Option<T> {
        Self::wait(waiters, || match receiver.try_recv() {
            Ok(value) => Poll::Ready(Some(value)),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Poll::Ready(None),
        })
        .await
    }

    pub(super) async fn wake(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
        waiters: Option<Waiters>,
    ) -> Option<()> {
        let Some(waiters) = waiters else { return receiver.recv().await };
        Self::wait(waiters, || match receiver.try_recv() {
            Ok(value) => Poll::Ready(Some(value)),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => Poll::Pending,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => Poll::Ready(None),
        })
        .await
    }
}

impl Drop for NetworkPump {
    fn drop(&mut self) {
        let active = ACTIVE.with_borrow_mut(Option::take);
        assert!(active.as_ref().is_some_and(|active| Arc::ptr_eq(active, &self.waiters)));
        let pending = self.waiters.lock().unwrap_or_else(|error| error.into_inner()).take();
        pending.unwrap_or_default().into_iter().for_each(Waker::wake);
    }
}

#[test]
fn pending_channels_cancel_and_the_next_scope_is_fresh() {
    use std::future::Future as _;
    struct StableWake;
    impl std::task::Wake for StableWake {
        fn wake(self: Arc<Self>) {}
    }
    let scope = NetworkPump::new();
    let waiters = NetworkPump::current().unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
    let mut receive = Box::pin(NetworkPump::oneshot(receiver, waiters.clone()));
    let waker = Waker::from(Arc::new(StableWake));
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    let waiter_count = waiters.lock().unwrap().as_ref().unwrap().len();
    assert_eq!(waiter_count, 1);
    std::thread::spawn(move || sender.send(())).join().unwrap().unwrap();
    NetworkPump::tick();
    assert_eq!(receive.as_mut().poll(&mut cx), Poll::Ready(Some(())));
    let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
    let mut receive = Box::pin(NetworkPump::oneshot(receiver, waiters));
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    drop(scope);
    assert!(NetworkPump::current().is_none());
    assert_eq!(receive.as_mut().poll(&mut cx), Poll::Ready(None));
    drop(receive);
    assert!(sender.is_closed());
    let _next = NetworkPump::new();
    let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
    drop(sender);
    let mut closed = Box::pin(NetworkPump::oneshot(receiver, NetworkPump::current().unwrap()));
    assert_eq!(closed.as_mut().poll(&mut cx), Poll::Ready(None));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    drop(sender);
    let mut closed = Box::pin(NetworkPump::wake(&mut receiver, NetworkPump::current()));
    assert_eq!(closed.as_mut().poll(&mut cx), Poll::Ready(None));
}
