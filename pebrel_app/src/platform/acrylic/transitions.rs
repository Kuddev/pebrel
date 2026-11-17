//! Keep a stale Acrylic sample out of maximize/restore-to-window animations.
//!
//! Unlike Accent, the controller's composition target remains visible while DWM
//! scales the old window representation. Temporarily clear only that backdrop;
//! the GPUI scene, pixel opacity, focus policy and Windows animation stay intact.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::UI::Composition::{CompositionBrush, ICompositionSupportsSystemBackdrop};
use windows_core::{Error, Result};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ANIMATIONINFO, IsIconic, IsWindowVisible, IsZoomed, KillTimer, SC_MAXIMIZE, SC_RESTORE,
    SPI_GETANIMATION, SetTimer, SystemParametersInfoW, WM_NCDESTROY, WM_SETTINGCHANGE,
    WM_SYSCOMMAND, WM_TIMER, WM_WINDOWPOSCHANGING,
};

// WM_WINDOWPOSCHANGED precedes the compositor animation's completion. Windows
// exposes no per-HWND completion event here. This bounded, nonblocking settling
// interval covers the native transition measured in our acceptance probes; it is
// not a claim to observe DWM completion. Repeated transitions extend the deadline.
const SETTLE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ShowState {
    maximized: bool,
    minimized: bool,
}

impl ShowState {
    fn read(hwnd: HWND) -> Self {
        Self {
            maximized: unsafe { IsZoomed(hwnd) != 0 },
            minimized: unsafe { IsIconic(hwnd) != 0 },
        }
    }

    fn changes_for(self, command: usize) -> bool {
        match command as u32 & 0xfff0 {
            SC_MAXIMIZE => !self.maximized && !self.minimized,
            SC_RESTORE => self.maximized && !self.minimized,
            _ => false,
        }
    }
}

struct State {
    target: ICompositionSupportsSystemBackdrop,
    saved: RefCell<Option<CompositionBrush>>,
    show: Cell<ShowState>,
    deadline: Cell<Option<Instant>>,
    attached: Cell<bool>,
    native_ref: Cell<bool>,
}

/// Owned by one Backdrop, on its HWND's thread; no global registry or worker.
pub(super) struct Transitions {
    hwnd: isize,
    state: Rc<State>,
}

impl Transitions {
    pub(super) fn new(hwnd: isize, target: ICompositionSupportsSystemBackdrop) -> Result<Self> {
        let state = Rc::new(State {
            target,
            saved: RefCell::new(None),
            show: Cell::new(ShowState::read(hwnd as _)),
            deadline: Cell::new(None),
            attached: Cell::new(false),
            native_ref: Cell::new(false),
        });
        let id = Rc::as_ptr(&state) as usize;
        let native = Rc::into_raw(state.clone());
        // SAFETY: the subclass owns a strong reference until successful removal
        // or WM_NCDESTROY. Each invocation also takes an Rc for nested teardown.
        if unsafe { SetWindowSubclass(hwnd as _, Some(window_proc), id, id) } == 0 {
            let error = Error::from_win32();
            unsafe {
                drop(Rc::from_raw(native));
            }
            return Err(error);
        }
        state.native_ref.set(true);
        state.attached.set(true);
        Ok(Self { hwnd, state })
    }
}

impl Drop for Transitions {
    fn drop(&mut self) {
        self.state.detach(self.hwnd as _, Rc::as_ptr(&self.state) as usize, false);
    }
}

impl State {
    fn detach(&self, hwnd: HWND, id: usize, destroying: bool) {
        self.attached.set(false);
        if self.native_ref.get() {
            let mut data = 0;
            let removed = unsafe {
                KillTimer(hwnd, id);
                RemoveWindowSubclass(hwnd, Some(window_proc), id) != 0
                    || destroying
                    || GetWindowSubclass(hwnd, Some(window_proc), id, &mut data) == 0
            };
            if removed && self.native_ref.replace(false) {
                // The caller still holds the owner or callback Rc here.
                unsafe {
                    Rc::decrement_strong_count(self as *const State);
                }
            } else {
                // Keep an inert failed-to-remove callback alive until destruction.
                log::warn!(target: "nebula", "Acrylic transition hook removal failed; retaining it until HWND destruction");
            }
        }
        self.deadline.set(None);
        let saved = self.saved.borrow_mut().take();
        drop(saved);
    }

    fn cancel(&self, hwnd: HWND, id: usize) {
        unsafe {
            KillTimer(hwnd, id);
        }
        self.deadline.set(None);
        self.resume();
    }

    fn pause(&self, hwnd: HWND, id: usize) {
        if !self.attached.get() || unsafe { IsWindowVisible(hwnd) } == 0 {
            return;
        }
        if !animations_enabled() {
            self.cancel(hwnd, id);
            return;
        }
        // Arm before clearing; a failed timer must not leave a live window clear.
        if unsafe { SetTimer(hwnd, id, SETTLE.as_millis() as u32, None) } == 0 {
            log::warn!(target: "nebula", "Acrylic transition timer failed; retaining the normal material");
            self.cancel(hwnd, id);
            return;
        }
        self.deadline.set(Some(Instant::now() + SETTLE));
        let needs_brush = self.saved.borrow().is_none();
        if needs_brush {
            let Ok(brush) = self.target.SystemBackdrop() else { return };
            if !self.attached.get() {
                return;
            }
            match self.target.SetSystemBackdrop(None::<&CompositionBrush>) {
                Ok(()) if self.attached.get() => {
                    let previous = self.saved.replace(Some(brush));
                    drop(previous);
                },
                Ok(()) => {},
                Err(error) => {
                    log::warn!(target: "nebula", "pausing Acrylic transition failed: {error}")
                },
            }
        }
    }

    fn resume(&self) {
        let saved = self.saved.borrow_mut().take();
        if let Some(brush) = saved {
            if !self.attached.get() {
                return;
            }
            // Null WinRT interfaces map to Error::empty (code 0). A real COM
            // error does not prove that the SDK's current material is empty.
            match self.target.SystemBackdrop() {
                Err(error) if error.code().0 == 0 && self.attached.get() => {
                    if let Err(error) = self.target.SetSystemBackdrop(&brush) {
                        log::warn!(target: "nebula", "resuming Acrylic transition failed: {error}");
                    }
                },
                Err(error) => {
                    log::warn!(target: "nebula", "reading Acrylic transition material failed: {error}")
                },
                // A newer SDK brush, including an accessibility fallback, wins.
                Ok(_) => {},
            }
        }
    }

    fn on_timer(&self, hwnd: HWND, id: usize) {
        if !self.attached.get() {
            return;
        }
        unsafe { KillTimer(hwnd, id) };
        let Some(deadline) = self.deadline.get() else { return };
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            // KillTimer does not remove queued WM_TIMER messages. A stale message
            // from an earlier transition must not reveal the backdrop too soon.
            let millis = remaining.as_millis().saturating_add(1).min(u32::MAX as u128) as u32;
            if unsafe { SetTimer(hwnd, id, millis, None) } != 0 {
                return;
            }
            // On resource failure, a usable material is preferable to leaving
            // the window permanently clear. The original artifact can recur.
            log::warn!(target: "nebula", "Acrylic transition timer rearm failed; restoring material");
        }
        self.deadline.set(None);
        self.resume();
    }
}

fn animations_enabled() -> bool {
    let mut animation =
        ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 1 };
    unsafe {
        if SystemParametersInfoW(
            SPI_GETANIMATION,
            animation.cbSize,
            &mut animation as *mut _ as _,
            0,
        ) != 0
            && animation.iMinAnimate == 0
        {
            return false;
        }
    }
    true
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    data: usize,
) -> isize {
    if !matches!(
        message,
        WM_SYSCOMMAND | WM_WINDOWPOSCHANGING | WM_TIMER | WM_NCDESTROY | WM_SETTINGCHANGE
    ) {
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }
    // SAFETY: the installed subclass retains this raw Rc allocation.
    // No mutable borrow or borrowed state crosses DefSubclassProc/COM callbacks.
    let state = unsafe {
        Rc::increment_strong_count(data as *const State);
        Rc::from_raw(data as *const State)
    };
    if message == WM_NCDESTROY {
        state.detach(hwnd, id, true);
    } else if message == WM_TIMER && wparam == id {
        state.on_timer(hwnd, id);
        return 0;
    } else if message == WM_SETTINGCHANGE && state.attached.get() && !animations_enabled() {
        state.cancel(hwnd, id);
    } else if message == WM_WINDOWPOSCHANGING {
        let show = ShowState::read(hwnd);
        let previous = state.show.replace(show);
        // Taskbar minimize/restore retains the native material and animation.
        // Pausing before minimize would add a visible flash before shrinking.
        if !previous.minimized && !show.minimized && previous.maximized != show.maximized {
            state.pause(hwnd, id);
        }
    } else if message == WM_SYSCOMMAND && ShowState::read(hwnd).changes_for(wparam) {
        state.pause(hwnd, id);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
