//! Current UI-thread system-session ending facts; persistence remains in its existing owner.

use std::cell::Cell;

thread_local! {
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn is_pending() -> bool {
    PENDING.try_with(Cell::get).unwrap_or(false)
}

fn set_pending(pending: bool) {
    // Native callbacks can run during thread teardown; never unwind through Win32.
    let _ = PENDING.try_with(|state| state.set(pending));
}

#[cfg(windows)]
#[path = "session_end/windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Monitor;

#[cfg(not(windows))]
pub(crate) struct Monitor;
#[cfg(not(windows))]
impl Monitor {
    pub(crate) fn install() -> std::io::Result<Self> {
        Ok(Self)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) fn set_pending(value: bool) {
        super::set_pending(value);
    }
}
