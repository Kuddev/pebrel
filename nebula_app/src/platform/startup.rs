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
        use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
        // SAFETY: this queries a borrowed monitor handle without retaining it.
        let monitor = unsafe { MonitorFromPoint(POINT { x: point.0, y: point.1 }, MONITOR_DEFAULTTONEAREST) };
        display_scale(monitor as u64).map(|scale| (monitor as u64, scale))
    }
    #[cfg(not(windows))]
    { let _ = point; None }
}

pub(crate) fn display_scale(id: u64) -> Option<f32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        let (mut x, mut y) = (0, 0);
        let monitor = id as *mut core::ffi::c_void;
        // SAFETY: the borrowed monitor and local DPI output pointers stay valid.
        if monitor.is_null() || unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) } < 0 || x == 0 || x != y {
            None
        } else { Some(x as f32 / 96.0) }
    }
    #[cfg(not(windows))]
    { let _ = id; None }
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
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowPos, SWP_NOSIZE, SWP_NOACTIVATE, SWP_NOZORDER};
    let hwnd = hwnd as *mut core::ffi::c_void;
    // SAFETY: both native output structures admit zero initialization.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let mut monitor: MONITORINFO = unsafe { std::mem::zeroed() };
    monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    // SAFETY: the caller owns the live HWND; the local outputs remain valid.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0
        || unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut monitor) } == 0 { return false; }
    let x = desired.0.clamp(monitor.rcWork.left, (monitor.rcWork.right - (rect.right - rect.left)).max(monitor.rcWork.left));
    let y = desired.1.clamp(monitor.rcWork.top, (monitor.rcWork.bottom - (rect.bottom - rect.top)).max(monitor.rcWork.top));
    // SAFETY: only the live window position changes; ownership and size are preserved.
    unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER) != 0 }
}
