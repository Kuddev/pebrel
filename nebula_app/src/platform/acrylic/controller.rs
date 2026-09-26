use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use super::composition::{Backdrop, Context};

#[derive(Default)]
struct State {
    windows: HashMap<gpui::WindowId, Backdrop>,
    context: Option<Context>,
    unavailable: bool,
    #[cfg(test)]
    attachments: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Owns final queue drain outside the App's shutdown borrow. Must surround run.
#[derive(Default)]
pub(crate) struct RunGuard(PhantomData<Rc<()>>);

impl Drop for RunGuard {
    fn drop(&mut self) {
        let mut state = STATE.with(|state| std::mem::take(&mut *state.borrow_mut()));
        state.windows.clear();
        if let Some(context) = state.context.take() {
            context.shutdown();
        }
    }
}

pub(crate) fn init(cx: &mut gpui::App) {
    cx.on_window_closed(|_, id| remove(id)).detach();
    cx.on_app_quit(|_| {
        STATE.with(|state| state.borrow_mut().windows.clear());
        async {}
    })
    .detach();
}

pub(crate) fn remove(id: gpui::WindowId) {
    STATE.with(|state| {
        state.borrow_mut().windows.remove(&id);
    });
}

/// Called only for a new HWND or material change, never an opacity/frame update.
pub(crate) fn apply(id: gpui::WindowId, hwnd: isize) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.windows.contains_key(&id) {
            return true;
        }
        if state.unavailable {
            return false;
        }
        if state.context.is_none() {
            match Context::new() {
                Ok(context) => state.context = Some(context),
                Err(error) => {
                    state.unavailable = true;
                    log::info!(target: "nebula", "Desktop Acrylic unavailable; using Accent fallback: {error}");
                    return false;
                },
            }
        }
        match state.context.as_mut().unwrap().attach(hwnd) {
            Ok(backdrop) => {
                state.windows.insert(id, backdrop);
                #[cfg(test)]
                {
                    state.attachments += 1;
                }
                log::debug!(target: "nebula", "Desktop Acrylic attached to HWND {hwnd:#x}");
                true
            },
            Err(error) => {
                log::warn!(target: "nebula", "Desktop Acrylic attach failed; using Accent fallback: {error}");
                false
            },
        }
    })
}

#[cfg(test)]
pub(crate) fn test_snapshot() -> (usize, usize) {
    STATE.with(|state| {
        let state = state.borrow();
        (state.windows.len(), state.attachments)
    })
}
