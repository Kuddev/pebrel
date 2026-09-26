use super::composition::Context;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, PM_REMOVE, PeekMessageW, TranslateMessage,
    WS_EX_NOREDIRECTIONBITMAP, WS_OVERLAPPEDWINDOW,
};

struct TestWindow(isize);
impl TestWindow {
    fn new() -> Self {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                windows_core::w!("STATIC").as_ptr(),
                windows_core::w!("Pebrel Acrylic lifecycle test").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        Self(hwnd as isize)
    }
}
impl Drop for TestWindow {
    fn drop(&mut self) {
        assert_ne!(unsafe { DestroyWindow(self.0 as _) }, 0);
    }
}

#[test]
#[ignore = "requires interactive Windows 11 and Windows App Runtime 1.8 >= 8000.946.1701.0"]
fn native_targets_rollback_switch_independently_and_drain_before_sta_exit() {
    // Real COM/WinRT calls protect ABI, failure rollback, two HWND ownership and
    // shutdown. This does not substitute for GPUI focus/edge screenshot checks.
    let mut context = Context::new().expect("required native runtime");
    let first = TestWindow::new();
    let second = TestWindow::new();
    assert!(context.attach(0).is_err());
    for _ in 0..8 {
        let a = context.attach(first.0).expect("first native target");
        let b = context.attach(second.0).expect("independent second target");
        assert!(a.has_live_full_size_root() && b.has_live_full_size_root());
        drop(a);
        assert!(b.has_live_full_size_root());
        // Reattach the same HWND while the other remains alive (material toggle).
        let a = context.attach(first.0).expect("reattach after Close");
        let mut message = unsafe { std::mem::zeroed() };
        unsafe {
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        drop(b);
        assert!(a.has_live_full_size_root());
        drop(a);
    }
    drop(first);
    drop(second);
    assert!(context.shutdown(), "owned queue must actually complete shutdown");
    assert!(windows::System::DispatcherQueue::GetForCurrentThread().is_err());
}

#[test]
#[ignore = "opens a native window; requires interactive Windows 11 and Windows App Runtime 1.8"]
fn native_transition_guard_preserves_moves_restarts_deadlines_and_releases_its_hook() {
    use std::time::{Duration, Instant};
    use windows::UI::Color;
    use windows::UI::Composition::{Compositor, ICompositionSupportsSystemBackdrop};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
    use windows_core::Interface;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PM_NOREMOVE, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetWindowPos, ShowWindow,
        ShowWindowAsync, WM_SYSCOMMAND, WM_TIMER,
    };

    let pump = |millis| {
        let until = Instant::now() + Duration::from_millis(millis);
        while Instant::now() < until {
            let mut message = unsafe { std::mem::zeroed() };
            unsafe {
                while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let context = Context::new().unwrap();
    let window = TestWindow::new();
    let compositor = Compositor::new().unwrap();
    let interop: ICompositorDesktopInterop = compositor.cast().unwrap();
    let target = unsafe { interop.CreateDesktopWindowTarget(HWND(window.0 as _), false).unwrap() };
    let root = compositor.CreateContainerVisual().unwrap();
    target.SetRoot(&root).unwrap();
    let support: ICompositionSupportsSystemBackdrop = target.cast().unwrap();
    let original =
        compositor.CreateColorBrushWithColor(Color { A: 255, R: 40, G: 70, B: 90 }).unwrap();
    support.SetSystemBackdrop(&original).unwrap();
    let guard = super::transitions::Transitions::new(window.0, support.clone()).unwrap();
    unsafe {
        ShowWindow(window.0 as _, SW_SHOWNOACTIVATE);
        SetWindowPos(
            window.0 as _,
            std::ptr::null_mut(),
            100,
            100,
            360,
            260,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    assert_eq!(
        support.SystemBackdrop().unwrap(),
        original.cast().unwrap(),
        "ordinary geometry keeps the material"
    );

    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) };
    assert!(support.SystemBackdrop().is_err(), "maximization removes the stale sample");
    // Make a WM_TIMER from the old transition pending without dispatching it.
    std::thread::sleep(Duration::from_millis(340));
    let mut message = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe { PeekMessageW(&mut message, window.0 as _, WM_TIMER, WM_TIMER, PM_NOREMOVE) },
        0
    );
    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
    pump(70);
    assert!(
        support.SystemBackdrop().is_err(),
        "an old queued timer must not end the new transition"
    );
    pump(320);
    assert_eq!(support.SystemBackdrop().unwrap(), original.cast().unwrap());

    // GPUI's titlebar takes the ShowWindow path, without WM_SYSCOMMAND.
    for action in [SW_MAXIMIZE, SW_RESTORE] {
        unsafe {
            ShowWindowAsync(window.0 as _, action);
        }
        pump(20);
        assert!(support.SystemBackdrop().is_err(), "programmatic transitions also pause");
        pump(340);
        assert_eq!(support.SystemBackdrop().unwrap(), original.cast().unwrap());
    }

    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_MINIMIZE as usize, 0) };
    assert_eq!(
        support.SystemBackdrop().unwrap(),
        original.cast().unwrap(),
        "minimize keeps native material"
    );
    pump(340);
    assert_eq!(support.SystemBackdrop().unwrap(), original.cast().unwrap());
    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
    pump(340);
    assert_eq!(
        support.SystemBackdrop().unwrap(),
        original.cast().unwrap(),
        "restore from taskbar keeps native material"
    );

    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) };
    assert!(support.SystemBackdrop().is_err());
    let newer = compositor.CreateColorBrushWithColor(Color { A: 255, R: 0, G: 0, B: 0 }).unwrap();
    support.SetSystemBackdrop(&newer).unwrap();
    pump(340);
    assert_eq!(
        support.SystemBackdrop().unwrap(),
        newer.cast().unwrap(),
        "a newer SDK policy brush wins"
    );

    for action in [SW_MINIMIZE, SW_RESTORE] {
        unsafe {
            ShowWindowAsync(window.0 as _, action);
        }
        pump(340);
        assert_eq!(
            support.SystemBackdrop().unwrap(),
            newer.cast().unwrap(),
            "maximized taskbar transitions keep native material"
        );
    }

    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
    assert!(support.SystemBackdrop().is_err());
    drop(guard);
    support.SetSystemBackdrop(&newer).unwrap();
    pump(340);
    assert_eq!(
        support.SystemBackdrop().unwrap(),
        newer.cast().unwrap(),
        "dropping the owner cancels pending restoration"
    );

    // Native HWND destruction can precede the Rust owner during reentrant close.
    let guard = super::transitions::Transitions::new(window.0, support.clone()).unwrap();
    unsafe { SendMessageW(window.0 as _, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) };
    assert!(support.SystemBackdrop().is_err());
    drop(window);
    pump(340);
    drop(guard);
    drop(support);
    target.Close().unwrap();
    drop(target);
    drop(interop);
    drop(root);
    drop(original);
    drop(newer);
    compositor.Close().unwrap();
    drop(compositor);
    assert!(context.shutdown());
}
