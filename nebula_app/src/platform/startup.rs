/// Surface a pre-logger failure for a GUI launch; the caller also returns it on stderr.
pub(crate) fn report_error(error: &dyn std::fmt::Display, gui_launch: bool) {
    #[cfg(windows)]
    crate::panic::report_startup_error(error, gui_launch);
    #[cfg(not(windows))]
    let _ = (error, gui_launch);
}

#[cfg(windows)]
mod console;
#[cfg(feature = "gpui-shell")]
pub(crate) mod first_frame;
#[cfg(any(unix, test))]
mod login;

/// Match GPUI's primary-monitor DPI query before a native window is created.
/// Other platforms retain post-creation sizing until their display API exposes scale.
pub(crate) fn primary_display_scale() -> Option<f32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
        use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

        // SAFETY: the monitor is borrowed and both DPI outputs are valid local pointers.
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let (mut x, mut y) = (0, 0);
            if monitor.is_null()
                || GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) < 0
                || x == 0
                || x != y
            {
                return None;
            }
            Some(x as f32 / 96.0)
        }
    }
    #[cfg(not(windows))]
    None
}

/// Prepare process-wide GUI state before worker threads or terminal children exist.
pub(crate) fn prepare_gui_process() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // Portable builds may not be on PATH. Non-PTY children need the same
        // executable fallback that agent_env supplies for each terminal pane.
        if let Ok(executable) = std::env::current_exe() {
            // SAFETY: main calls this while startup is still single-threaded.
            unsafe { std::env::set_var(crate::agent_env::CLI_ENV, executable) };
        }
        if std::env::var_os("NEBULA_DETACHED_LAUNCH").is_some() {
            // Detach before either GUI event loop starts, so a launcher exiting
            // cannot take the window down with its startup console.
            unsafe { windows_sys::Win32::System::Console::FreeConsole() };
        }
        console::prepare_console_for_gui()?;
    }
    Ok(())
}

pub fn prepare_gui() {
    #[cfg(target_os = "macos")]
    {
        crate::macos::locale::set_locale_environment();
        crate::macos::disable_autofill();
        if std::env::current_dir().ok().as_deref() == Some(std::path::Path::new("/")) {
            if let Some(home) = home::home_dir() {
                if let Err(error) = std::env::set_current_dir(home) {
                    eprintln!("Could not use the home directory: {error}");
                }
            }
        }
    }
    super::notifications::prepare();
}

pub(crate) fn start_hidden(settings: &nebula_settings::RuntimeSettings) -> bool {
    super::CAPABILITIES.hide_window_on_close && settings.silent_start && settings.tray
}

/// The installer and Settings manage the same per-user Startup shortcut.
pub(crate) fn launch_at_login() -> bool {
    #[cfg(windows)]
    return startup_shortcut().is_ok_and(|path| path.is_file());
    #[cfg(unix)]
    return login::entry_path().is_ok_and(|path| path.is_file());
}

pub(crate) fn set_launch_at_login(enabled: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        use windows::core::{HSTRING, Interface, w};

        let path = startup_shortcut()?;
        if !enabled {
            return match std::fs::remove_file(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            };
        }
        let executable = std::env::current_exe()?;
        // SAFETY: COM calls stay on this thread; interfaces are dropped before
        // balancing the successful initialization, including on save errors.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(std::io::Error::other)?;
            let result = (|| -> windows::core::Result<()> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(&HSTRING::from(executable.as_os_str()))?;
                link.SetArguments(w!("--gpui"))?;
                if let Some(home) = super::dirs::home_dir() {
                    link.SetWorkingDirectory(&HSTRING::from(home.as_os_str()))?;
                }
                link.cast::<IPersistFile>()?.Save(&HSTRING::from(path.as_os_str()), true)
            })();
            CoUninitialize();
            result.map_err(std::io::Error::other)
        }
    }
    #[cfg(unix)]
    {
        login::set_enabled(enabled)
    }
}

#[cfg(windows)]
fn startup_shortcut() -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Startup, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    // SAFETY: the known-folder buffer is copied before its COM allocation is freed.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT, None)
            .map_err(std::io::Error::other)?;
        let directory = std::ffi::OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        Ok(std::path::PathBuf::from(directory).join("Pebrel.lnk"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn silent_start_requires_a_tray_and_native_window_hiding() {
        use nebula_settings::{RawSettings, RuntimeSettings};

        for (text, expected) in [
            ("", false),
            ("silent_start=1\ntray=0", false),
            ("silent_start=0\ntray=1", false),
            ("silent_start=1\ntray=1", super::super::CAPABILITIES.hide_window_on_close),
        ] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(text));
            assert_eq!(super::start_hidden(&settings), expected);
        }
    }
}

/// GPUI Windows display ids are the borrowed HMONITOR value at the pinned backend.
pub(crate) fn physical_display(point: (i32, i32)) -> Option<(u64, f32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
        // SAFETY: this queries a borrowed monitor handle without retaining it.
        let monitor =
            unsafe { MonitorFromPoint(POINT { x: point.0, y: point.1 }, MONITOR_DEFAULTTONEAREST) };
        display_scale(monitor as u64).map(|scale| (monitor as u64, scale))
    }
    #[cfg(not(windows))]
    {
        let _ = point;
        None
    }
}

pub(crate) fn display_scale(id: u64) -> Option<f32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        let (mut x, mut y) = (0, 0);
        let monitor = id as *mut core::ffi::c_void;
        // SAFETY: the borrowed monitor and local DPI output pointers stay valid.
        if monitor.is_null()
            || unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) } < 0
            || x == 0
            || x != y
        {
            None
        } else {
            Some(x as f32 / 96.0)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        None
    }
}

pub(crate) fn placement_offset(id: u64) -> (i32, i32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
        // SAFETY: MONITORINFO is a plain output structure; cbSize is set below.
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: the borrowed display handle and initialized output are valid.
        if unsafe { GetMonitorInfoW(id as *mut core::ffi::c_void, &mut info) } != 0 {
            return (info.rcWork.left - info.rcMonitor.left, info.rcWork.top - info.rcMonitor.top);
        }
    }
    let _ = id;
    (0, 0)
}

#[cfg(windows)]
pub(crate) fn place_normal_window(hwnd: isize, desired: (i32, i32)) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    let hwnd = hwnd as *mut core::ffi::c_void;
    // SAFETY: both native output structures admit zero initialization.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let mut monitor: MONITORINFO = unsafe { std::mem::zeroed() };
    monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    // SAFETY: the caller owns the live HWND; the local outputs remain valid.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0
        || unsafe {
            GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut monitor)
        } == 0
    {
        return false;
    }
    let x = desired.0.clamp(
        monitor.rcWork.left,
        (monitor.rcWork.right - (rect.right - rect.left)).max(monitor.rcWork.left),
    );
    let y = desired.1.clamp(
        monitor.rcWork.top,
        (monitor.rcWork.bottom - (rect.bottom - rect.top)).max(monitor.rcWork.top),
    );
    // SAFETY: only the live window position changes; ownership and size are preserved.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        ) != 0
    }
}

#[cfg(feature = "gpui-shell")]
pub(crate) fn normal_position(
    origin: gpui::Point<gpui::Pixels>,
    scale: f32,
    display: Option<u64>,
) -> Option<(i32, i32)> {
    #[cfg(windows)]
    return display.map(|display| {
        let offset = placement_offset(display);
        desktop_position((origin.x.as_f32(), origin.y.as_f32()), scale, offset)
    });
    #[cfg(not(windows))]
    {
        let _ = (origin, scale, display);
        None
    }
}

#[cfg(feature = "gpui-shell")]
pub(crate) fn place_configured_window(window: &gpui::Window, desired: (i32, i32)) -> bool {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return false };
        place_normal_window(handle.hwnd.get(), desired)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, desired);
        true
    }
}

/// Convert the pinned Windows backend's client/workspace point to desktop pixels.
pub(crate) fn desktop_position(origin: (f32, f32), scale: f32, offset: (i32, i32)) -> (i32, i32) {
    ((origin.0 * scale).round() as i32 + offset.0, (origin.1 * scale).round() as i32 + offset.1)
}

/// Convert desktop pixels to the client/workspace point accepted by GPUI placement.
pub(crate) fn workspace_position(
    position: (i32, i32),
    scale: f32,
    offset: (i32, i32),
) -> (f32, f32) {
    ((position.0 - offset.0) as f32 / scale, (position.1 - offset.1) as f32 / scale)
}

#[cfg(test)]
mod geometry_tests {
    #[test]
    fn windows_workspace_desktop_roundtrip_keeps_top_and_left_taskbars_at_fractional_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for offset in [(0, 0), (0, 60), (80, 0)] {
                for position in [(-1700, 120), (197, 102), (0, 0)] {
                    let origin = super::workspace_position(position, scale, offset);
                    assert_eq!(super::desktop_position(origin, scale, offset), position);
                }
            }
        }
    }
}

#[cfg(all(test, windows, feature = "gpui-shell"))]
mod native_geometry_tests {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetClientRect, GetWindowPlacement, GetWindowRect,
        SetWindowPlacement, WINDOWPLACEMENT, WS_OVERLAPPEDWINDOW,
    };

    struct NativeWindow(isize);
    impl Drop for NativeWindow {
        fn drop(&mut self) {
            // SAFETY: this test owns the native window until teardown.
            assert_ne!(unsafe { DestroyWindow(self.0 as _) }, 0);
        }
    }

    #[test]
    fn physical_outer_position_and_backend_normal_placement_roundtrip() {
        // SAFETY: STATIC is a registered class; no pointer escapes the native call.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                windows_core::w!("STATIC").as_ptr(),
                windows_core::w!("Pebrel geometry test").as_ptr(),
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
        let _window = NativeWindow(hwnd as isize);
        // SAFETY: plain native output structures admit zero initialization.
        let (mut rect, mut client): (RECT, RECT) = unsafe { std::mem::zeroed() };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: the owned HWND and initialized output structure are valid.
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        assert_ne!(unsafe { GetMonitorInfoW(monitor, &mut info) }, 0);
        let desired = (info.rcWork.left + 197, info.rcWork.top + 102);
        assert!(super::place_normal_window(hwnd as isize, desired));
        // SAFETY: all output pointers refer to initialized local structures.
        assert_ne!(unsafe { GetWindowRect(hwnd, &mut rect) }, 0);
        assert_ne!(unsafe { GetClientRect(hwnd, &mut client) }, 0);
        assert_eq!((rect.left, rect.top), desired);
        let before = (rect.left, rect.top, rect.right, rect.bottom);
        let border = (
            (rect.right - rect.left - client.right) / 2,
            (rect.bottom - rect.top - client.bottom) / 2,
        );
        let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        assert_ne!(unsafe { GetWindowPlacement(hwnd, &mut placement) }, 0);
        let scale = super::display_scale(monitor as u64).unwrap();
        let native = placement.rcNormalPosition;
        let logical =
            ((native.left + border.0) as f32 / scale, (native.top + border.1) as f32 / scale);
        let offset = super::placement_offset(monitor as u64);
        let saved = super::desktop_position(logical, scale, offset);
        let restored = super::workspace_position(saved, scale, offset);
        let x = (restored.0 * scale).round() as i32 - border.0;
        let y = (restored.1 * scale).round() as i32 - border.1;
        placement.rcNormalPosition = RECT {
            left: x,
            top: y,
            right: x + native.right - native.left,
            bottom: y + native.bottom - native.top,
        };
        assert_ne!(unsafe { SetWindowPlacement(hwnd, &placement) }, 0);
        assert_ne!(unsafe { GetWindowRect(hwnd, &mut rect) }, 0);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), before);
    }
}
