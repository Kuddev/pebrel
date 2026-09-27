//! 系统关机前的最后一笔会话快照。
//!
//! gpui 的 Windows 平台层(`gpui_windows`)只处理 `WM_CLOSE`/`WM_DESTROY`,
//! 系统关机会在宽限期后强杀进程——既有退出链路(身份握手、确认弹窗)整个
//! 走不到,持久化只剩 ≤1 秒前的 1 Hz checkpoint。这里在独立线程上注册一
//! 个不可见顶层窗口接住 `WM_QUERYENDSESSION`/`WM_ENDSESSION`,同步落盘最
//! 近一次会话快照后立即自行退出。
//!
//! wndproc 拿不到 `&mut App`(关机消息由系统直接派发到所属线程),所以保存
//! 用的不是现场重新采快照,而是 [`note_latest`] 在每次持久化保存点镜像的
//! 最新会话——schema 与正常 checkpoint/退出完全一致。代价:关机瞬间仍在
//! 握手(hook 未上报、probe 未落地)的 AI 会话身份直接降级为只恢复目录;
//! 主线程上的有界等待只会卡住唯一能推进握手的一方,故不做。
//!
//! 非 Windows 平台:桌面会话结束走正常窗口 close 事件,已被既有退出链路
//! 覆盖,`install` 为空实现。

use std::sync::Mutex;

/// 最近一次会话快照的镜像;`workspace::session_persistence` 在每个保存点
/// (checkpoint / 关窗 / 退出)更新它。
static LATEST: Mutex<Option<crate::session::Session>> = Mutex::new(None);

/// 持久化层每次确认一份新快照后调用。锁失败(其他线程 panic 临界)时静默
/// 跳过——关机路径拿不到镜像就保持上一次成功落盘的内容,语义仍然安全。
pub(crate) fn note_latest(session: &crate::session::Session) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = Some(session.clone());
    }
}

/// 关机快照的纯转换:内容原样保留,只把收尾标记拨成干净退出——关机由
/// 用户发起,下次启动照常按 `restore_session` 恢复,但不该弹「上次未正常
/// 退出」的提示。空 tab 列表是用户一路关干净的合法状态,同样照写。
#[cfg(any(windows, test))]
fn shutdown_snapshot(mut session: crate::session::Session) -> crate::session::Session {
    session.clean_exit = true;
    session
}

#[cfg(windows)]
fn snapshot_for_shutdown() -> Option<crate::session::Session> {
    LATEST.lock().ok()?.clone().map(shutdown_snapshot)
}

/// 开机时挂接关机监听(由 `gpui_shell::workspace::windowing::initialize`
/// 调用一次;重复调用无副作用)。仅 Windows 实做。
#[inline]
pub(crate) fn install() {
    #[cfg(all(windows, not(test)))]
    imp::install();
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG,
        RegisterClassW, TranslateMessage, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
        WS_OVERLAPPED,
    };

    use super::snapshot_for_shutdown;

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 常驻隐藏顶层窗口的专属线程。关机通知发到进程的每个顶层窗口;消息
    /// 专属窗口(HWND_MESSAGE)在部分 Windows 版本上不参与注销枚举(与
    /// `tray` 同一结论),所以必须是带消息泵的顶层窗口。
    pub(crate) fn install() {
        if INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("session-shutdown-watch".to_owned())
            .spawn(watch_thread);
        if let Err(error) = spawned {
            log::warn!("session shutdown watch thread failed to start: {error}");
        }
    }

    fn watch_thread() {
        let class_name = wide("NebulaShutdownWatchWindow");
        // SAFETY: 类名/实例句柄在调用期间有效;重复注册返回 0 无妨(上方
        // INSTALLED 闸已保证单注册)。窗口无标题栏、零尺寸、永不 ShowWindow,
        // 其余消息全部回落 DefWindowProcW。
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(shutdown_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("Nebula shutdown watch").as_ptr(),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                log::warn!("session shutdown watch: hidden window creation failed");
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    extern "system" fn shutdown_wndproc(
        handle: HWND,
        message: u32,
        wparam: windows_sys::Win32::Foundation::WPARAM,
        lparam: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT {
        match message {
            // 立即同意关机:真正的保存留给 ENDSESSION;QUERY 阶段拖延只会
            // 挤占整个系统的关机预算。
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION if wparam != 0 => save_and_exit(),
            _ => unsafe { DefWindowProcW(handle, message, wparam, lparam) },
        }
    }

    /// 落盘最近一次快照并主动退出:不吃系统宽限期后的强杀,也避开带交互
    /// 确认的正常退出链路。与主线程 checkpoint 并发写冲突时有界重试;PTY
    /// 子进程随系统关机一并回收。
    fn save_and_exit() -> ! {
        if let Some(session) = snapshot_for_shutdown() {
            for attempt in 0..5 {
                match crate::session::try_save(&session) {
                    Ok(()) => break,
                    Err(error) => {
                        log::warn!(
                            "shutdown session save failed (attempt {}): {error}",
                            attempt + 1
                        );
                        if attempt == 4 {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    },
                }
            }
        }
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_snapshot_marks_clean_exit_and_keeps_content() {
        // 纯转换(进程级镜像静态与持久化测试并发,不能直接触碰);线程接线
        // 由真机验收。
        let mut session = crate::session::Session::new(0, Vec::new());
        session.clean_exit = false;
        let fixed = shutdown_snapshot(session);
        assert!(fixed.clean_exit, "关机由用户发起,不算崩溃");
        assert!(fixed.tabs.is_empty(), "空 tab 列表是合法的最终状态,原样保留");

        let mut session = crate::session::Session::new(2, Vec::new());
        session.tabs.push(crate::session::TabSession::single("D:/work".into(), None, None));
        let before = session.clone();
        let fixed = shutdown_snapshot(session);
        assert!(fixed.clean_exit);
        assert_eq!(fixed.tabs, before.tabs);
        assert_eq!(fixed.active_tab, 2);
    }
}
