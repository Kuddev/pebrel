//! STA-affine composition resources. No background worker or per-frame updates.

use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::UI::Composition::Compositor;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
    RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
};
use windows_core::{HRESULT, Interface, Result};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use super::bindings::{Acrylic, Configuration};
use super::runtime::Runtime;
use super::transitions::Transitions;

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub(super) struct Context {
    compositor: Option<Compositor>,
    queue: Option<DispatcherQueueController>,
    // Drop order: SDK objects/queue, runtime package lease, then our own STA ref.
    _runtime: Runtime,
    _apartment: Apartment,
    _thread: PhantomData<Rc<()>>,
}

impl Context {
    pub(super) fn new() -> Result<Self> {
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
        let apartment = Apartment;
        let runtime = Runtime::load()?;
        let queue = if DispatcherQueue::GetForCurrentThread().is_ok() {
            None
        } else {
            Some(unsafe {
                CreateDispatcherQueueController(DispatcherQueueOptions {
                    dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_NONE,
                })?
            })
        };
        // Delay fallible compositor activation until State owns this context;
        // even a failed attach must drain an owned queue at application shutdown.
        Ok(Self {
            compositor: None,
            queue,
            _runtime: runtime,
            _apartment: apartment,
            _thread: PhantomData,
        })
    }

    pub(super) fn attach(&mut self, hwnd: isize) -> Result<Backdrop> {
        if self.compositor.is_none() {
            self.compositor = Some(Compositor::new()?);
        }
        let compositor = self.compositor.as_ref().unwrap();
        let host = HostBackdrop::new(hwnd)?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        // GPUI owns the topmost target; the material occupies the lower target.
        let target = unsafe { interop.CreateDesktopWindowTarget(HWND(hwnd as _), false)? };
        let mut backdrop =
            Backdrop { transitions: None, controller: None, config: None, target, _host: host };
        let root = compositor.CreateContainerVisual()?;
        let mut relative_size = root.RelativeSizeAdjustment()?;
        relative_size.X = 1.0;
        relative_size.Y = 1.0;
        root.SetRelativeSizeAdjustment(relative_size)?;
        backdrop.target.SetRoot(&root)?;
        backdrop.controller = Some(Acrylic::new()?);
        let controller = backdrop.controller.as_ref().unwrap();
        controller.attach(hwnd, &backdrop.target.cast()?)?;
        backdrop.config = Some(controller.configure()?);
        backdrop.transitions = Some(Transitions::new(hwnd, backdrop.target.cast()?)?);
        Ok(backdrop)
    }

    /// Called outside GPUI's App borrow, AFTER Application::run returns. Never
    /// pump messages in a window-close callback or on_app_quit (re-entrancy).
    pub(super) fn shutdown(mut self) -> bool {
        if let Some(compositor) = self.compositor.take() {
            if let Err(error) = compositor.Close() {
                log::warn!(target: "nebula", "closing Acrylic compositor failed: {error}");
            }
        }
        let Some(queue) = self.queue.as_ref() else { return true };
        let shutdown = match queue.ShutdownQueueAsync() {
            Ok(shutdown) => shutdown,
            Err(error) => {
                log::warn!(target: "nebula", "Acrylic queue shutdown failed: {error}");
                // Keep package resolution and our STA alive for outstanding
                // callbacks until process exit. This is not a live-window leak.
                std::mem::forget(self);
                return false;
            },
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while shutdown.Status().is_ok_and(|status| status.0 == 0) && Instant::now() < deadline {
            let mut message: MSG = unsafe { std::mem::zeroed() };
            // The outer application loop has ended; WM_QUIT needs no repost.
            unsafe {
                while Instant::now() < deadline
                    && PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0
                {
                    if message.message != WM_QUIT {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if shutdown.Status().is_ok_and(|status| status.0 == 1) {
            if let Err(error) = shutdown.GetResults() {
                log::warn!(target: "nebula", "Acrylic queue shutdown result: {error}");
                std::mem::forget(shutdown);
                std::mem::forget(self);
                return false;
            }
            true
        } else {
            log::warn!(target: "nebula", "Acrylic queue did not drain; retaining runtime until process exit");
            std::mem::forget(shutdown);
            std::mem::forget(self);
            false
        }
    }
}

pub(super) struct Backdrop {
    transitions: Option<Transitions>,
    controller: Option<Acrylic>,
    config: Option<Configuration>,
    target: DesktopWindowTarget,
    _host: HostBackdrop,
}

#[cfg(test)]
impl Backdrop {
    pub(super) fn has_live_full_size_root(&self) -> bool {
        self.target
            .Root()
            .and_then(|root| root.RelativeSizeAdjustment())
            .is_ok_and(|size| size.X == 1.0 && size.Y == 1.0)
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        // The HWND callback must stop using its target before the SDK closes it.
        self.transitions.take();
        if let Some(controller) = self.controller.take() {
            controller.close();
        }
        self.config.take();
        if let Err(error) = self.target.Close() {
            log::warn!(target: "nebula", "closing Acrylic desktop target failed: {error}");
        }
    }
}

struct HostBackdrop(isize);
impl HostBackdrop {
    fn new(hwnd: isize) -> Result<Self> {
        Self::set(hwnd, true)?;
        Ok(Self(hwnd))
    }

    fn set(hwnd: isize, enabled: bool) -> Result<()> {
        let value = i32::from(enabled);
        unsafe {
            HRESULT(DwmSetWindowAttribute(
                hwnd as _,
                DWMWA_USE_HOSTBACKDROPBRUSH as u32,
                &value as *const _ as _,
                std::mem::size_of_val(&value) as u32,
            ))
            .ok()
        }
    }
}
impl Drop for HostBackdrop {
    fn drop(&mut self) {
        if unsafe { IsWindow(self.0 as _) } != 0 {
            if let Err(error) = Self::set(self.0, false) {
                log::warn!(target: "nebula", "clearing Acrylic host backdrop failed: {error}");
            }
        }
    }
}
