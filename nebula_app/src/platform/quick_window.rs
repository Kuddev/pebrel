/// 旧壳 `Display::configure_quick_terminal` 的几何合同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QuickTerminalGeometry {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl QuickTerminalGeometry {
    fn animated_y(self, hidden_fraction: f32) -> i32 {
        self.y - (self.height as f32 * hidden_fraction.clamp(0.0, 1.0)).round() as i32
    }
}

/// 以普通工作区 HWND 所在显示器为目标；没有锚点时 Win32 回退主显示器。
pub(crate) fn native_geometry(
    anchor_hwnd: isize,
    remembered: Option<nebula_settings::QuickTerminalSize>,
) -> Option<QuickTerminalGeometry> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
        MonitorFromWindow,
    };

    let anchor = anchor_hwnd as *mut core::ffi::c_void;
    let fallback =
        if anchor.is_null() { MONITOR_DEFAULTTOPRIMARY } else { MONITOR_DEFAULTTONEAREST };
    // SAFETY: anchor 来自当前进程已注册的 GPUI 窗口；失效或为空时 API 按
    // fallback 选择主/最近显示器。失败统一返回 None，不解引用 HWND。
    let monitor = unsafe { MonitorFromWindow(anchor, fallback) };
    if monitor.is_null() {
        return None;
    }
    let zero = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: zero,
        rcWork: zero,
        dwFlags: 0,
    };
    // 旧壳使用 monitor.size/position，即完整屏幕而非扣掉任务栏的 work area。
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let width = (info.rcMonitor.right - info.rcMonitor.left).max(1);
    let monitor_height = (info.rcMonitor.bottom - info.rcMonitor.top).max(1);
    let height = ((monitor_height as f64) * 0.4).round().max(1.0) as i32;
    let scale = if anchor_hwnd != 0 {
        (unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(anchor) }) as f32 / 96.0
    } else {
        1.0
    };
    let scale = scale.max(1.0);
    let saved =
        remembered.map(|size| size.fit(width as f32 / scale, monitor_height as f32 / scale));
    let saved_width = saved.map_or(width, |size| (size.width * scale).round() as i32);
    let saved_height = saved.map_or(height, |size| (size.height * scale).round() as i32);
    Some(QuickTerminalGeometry {
        x: info.rcMonitor.left + (width - saved_width) / 2,
        y: info.rcMonitor.top,
        width: saved_width,
        height: saved_height,
    })
}

/// 创建时一次性设置旧壳的完整几何和 topmost，再把窗口显示在屏幕上缘之外。
/// 后续动画不能再走这个入口，否则每帧重排 z-order/重复 SHOW 会造成明显卡顿。
pub(crate) fn configure_native_window(
    hwnd: isize,
    geometry: QuickTerminalGeometry,
    hidden_fraction: f32,
    show: bool,
) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
    };

    if hwnd == 0 {
        return false;
    }
    let mut flags = SWP_NOACTIVATE;
    if show {
        flags |= SWP_SHOWWINDOW;
    }
    // SAFETY: HWND 由 GPUI 窗口创建回调发布；SetWindowPos 对已失效窗口安全
    // 失败。SWP_NOACTIVATE 保证动画帧本身不反复抢前台，显式热键另行激活。
    unsafe {
        SetWindowPos(
            hwnd as *mut core::ffi::c_void,
            HWND_TOPMOST,
            geometry.x,
            geometry.animated_y(hidden_fraction),
            geometry.width,
            geometry.height,
            flags,
        ) != 0
    }
}

/// 对齐旧壳 `set_quick_terminal_slide`：动画帧只改变外窗 Y，既不改尺寸、
/// 不改 topmost 层级，也不重复 show。PTY、GPUI swapchain 和 DComp surface
/// 因此不会在 90-120ms 动画期间参与 resize。
pub(crate) fn slide_native_window(
    hwnd: isize,
    geometry: QuickTerminalGeometry,
    hidden_fraction: f32,
) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };

    if hwnd == 0 {
        return false;
    }
    // SAFETY: HWND 由 GPUI 窗口注册表持有；失效时 SetWindowPos 安全失败。
    unsafe {
        SetWindowPos(
            hwnd as *mut core::ffi::c_void,
            std::ptr::null_mut(),
            geometry.x,
            geometry.animated_y(hidden_fraction),
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
        ) != 0
    }
}

pub(crate) fn show_native_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};

    if hwnd != 0 {
        // 激活只属于显式热键路径，由 GPUI `activate_window` 单独完成。
        unsafe { ShowWindow(hwnd as *mut core::ffi::c_void, SW_SHOWNOACTIVATE) };
    }
}

pub(crate) fn hide_native_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    if hwnd != 0 {
        // SAFETY: HWND 由 GPUI 创建；窗口已关闭时 ShowWindow 只会安全失败。
        unsafe { ShowWindow(hwnd as *mut core::ffi::c_void, SW_HIDE) };
    }
}

#[cfg(test)]
mod tests {
    use super::QuickTerminalGeometry;

    #[test]
    fn slide_geometry_handles_negative_monitor_origins() {
        let geometry = QuickTerminalGeometry { x: -1920, y: -200, width: 1920, height: 480 };
        assert_eq!(geometry.animated_y(0.0), -200);
        assert_eq!(geometry.animated_y(0.5), -440);
        assert_eq!(geometry.animated_y(1.0), -680);
        assert_eq!(geometry.animated_y(2.0), -680);
    }
}
