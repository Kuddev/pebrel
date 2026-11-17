//! Scoped native-delivery capture and admission time for routed notification tests.

use std::cell::RefCell;
use std::time::{Duration, Instant};

struct Capture {
    now: Instant,
    messages: Vec<(String, String)>,
}

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

pub(crate) struct Guard;

pub(crate) fn start(now: Instant) -> Guard {
    CAPTURE.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Capture { now, messages: Vec::new() });
    });
    Guard
}

pub(crate) fn time() -> Option<Instant> {
    CAPTURE.with(|slot| slot.borrow().as_ref().map(|capture| capture.now))
}

pub(crate) fn advance(duration: Duration) {
    CAPTURE.with(|slot| slot.borrow_mut().as_mut().unwrap().now += duration);
}

pub(crate) fn record(title: &str, body: &str) -> bool {
    CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(capture) = slot.as_mut() else { return false };
        capture.messages.push((title.into(), body.into()));
        true
    })
}

pub(crate) fn messages() -> Vec<(String, String)> {
    CAPTURE.with(|slot| slot.borrow().as_ref().unwrap().messages.clone())
}

impl Drop for Guard {
    fn drop(&mut self) {
        CAPTURE.with(|slot| *slot.borrow_mut() = None);
    }
}
