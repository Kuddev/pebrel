//! GPUI owns macOS authorization, foreground presentation and native delegate lifetime.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{App, SystemNotification, SystemNotificationAction};

use super::registry::{CAPACITY, Registry};
use super::{ToastAction, ToastActivation};
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SEND: OnceLock<Mutex<mpsc::Sender<Pending>>> = OnceLock::new();

struct Pending {
    title: String,
    body: String,
    activation: Option<ToastActivation>,
    actions: Vec<ToastAction>,
}

pub(super) fn is_active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn init(cx: &mut App) {
    ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    // Register native callbacks only for a real application bundle.
    if objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let mut generation = [0_u8; 16];
    if let Err(error) = getrandom::fill(&mut generation) {
        log::warn!("notify: cannot initialize native notification identity: {error}");
        return;
    }
    let generation = generation.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let (sender, mut receiver) = mpsc::channel::<Pending>(CAPACITY);
    if SEND.set(Mutex::new(sender)).is_err() {
        return;
    }
    let callbacks = Rc::new(RefCell::new(Registry::new(generation)));
    let response_callbacks = callbacks.clone();
    cx.on_system_notification_response(move |response, cx| {
        let callback = {
            let mut callbacks = response_callbacks.borrow_mut();
            callbacks.take(&response.tag)
        };
        let Some(callback) = callback else { return };
        // Release the registry borrow before activation can re-enter application delivery.
        cx.dismiss_system_notification(&callback.tag);
        callback.activate(response.action_id.as_deref());
    });
    cx.spawn(async move |cx| {
        while let Some(pending) = receiver.next().await {
            cx.update(|cx| {
                let actions = pending
                    .actions
                    .iter()
                    .enumerate()
                    .map(|(index, action)| SystemNotificationAction {
                        id: format!("choice-{index}").into(),
                        label: action.label.clone().into(),
                    })
                    .collect();
                let registration = {
                    let mut callbacks = callbacks.borrow_mut();
                    callbacks.insert(pending.activation, pending.actions)
                };
                let Some((tag, expired)) = registration else {
                    log::warn!("notify: native notification identity exhausted");
                    return;
                };
                if let Some(expired) = expired {
                    cx.dismiss_system_notification(&expired);
                }
                cx.show_system_notification(SystemNotification {
                    tag: tag.into(),
                    title: pending.title.into(),
                    body: pending.body.into(),
                    actions,
                });
            });
        }
    })
    .detach();
}

pub(super) fn dispatch(
    title: String,
    body: String,
    activation: Option<ToastActivation>,
    actions: Vec<ToastAction>,
) {
    let Some(sender) = SEND.get() else {
        log::warn!("notify: macOS native notification service is not ready");
        return;
    };
    // No worker, RPC wait or blocking send on terminal/UI threads.
    let Ok(mut sender) = sender.try_lock() else {
        log::warn!("notify: native notification queue busy");
        return;
    };
    if let Err(error) = sender.try_send(Pending { title, body, activation, actions }) {
        log::warn!("notify: native notification queue unavailable: {error}");
    }
}
