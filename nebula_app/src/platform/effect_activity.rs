//! 后处理的窗口活动快照；跨平台动画只由实际绘制续期，不靠失焦轮询探测合成器状态。
use gpui::Window;

#[derive(Clone, Copy)]
pub(crate) struct EffectActivity {
    #[cfg(not(windows))]
    active: bool,
    #[cfg(windows)]
    native: isize,
}

impl EffectActivity {
    pub(crate) fn capture(window: &Window) -> Self {
        Self {
            #[cfg(not(windows))]
            active: window.is_window_active(),
            #[cfg(windows)]
            native: {
                use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
                HasWindowHandle::window_handle(window)
                    .ok()
                    .and_then(|handle| {
                        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                            Some(handle.hwnd.get())
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0)
            },
        }
    }

    pub(crate) fn focused(self) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetForegroundWindow};
            // 非输入桌面的 WM_ACTIVATE 也可能为真，沿用原生前台事实而不是缓存的激活位。
            self.native != 0 && unsafe { GetForegroundWindow() == HWND(self.native as *mut _) }
        }
        #[cfg(not(windows))]
        {
            self.active
        }
    }

    pub(crate) fn hidden(self) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{IsIconic, IsWindowVisible},
            };
            let hwnd = HWND(self.native as *mut _);
            self.native == 0
                || unsafe { !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() }
        }
        #[cfg(not(windows))]
        {
            // Wayland 没有通用同步最小化查询；未获知隐藏不等于一定可见。
            // 控制器只在一次真实绘制后安排一个唤醒，合成器停止帧后不会自行续期。
            false
        }
    }
}

pub(crate) fn supported(window: &Window) -> bool {
    cfg!(feature = "shader-background") && window.supports_postprocess_wgsl()
}
