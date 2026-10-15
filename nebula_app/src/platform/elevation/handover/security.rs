//! 管理员接收窗口的身份与命名互斥量；窗口 PID 来自内核，身份来自进程令牌。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS,
    TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenSessionId, TokenUser,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

#[derive(Clone, Debug)]
pub(super) struct Identity {
    user: String,
    session: u32,
    pub(super) elevated: bool,
}

impl Identity {
    pub(super) fn current() -> io::Result<Self> {
        Self::for_process(std::process::id())
    }

    fn for_process(pid: u32) -> io::Result<Self> {
        // SAFETY: 原生句柄立即移入 OwnedHandle；令牌缓冲在转换 SID 前保持存活。
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let process = OwnedHandle::from_raw_handle(process);
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = OwnedHandle::from_raw_handle(token);
            let token = token.as_raw_handle();
            let elevation: TOKEN_ELEVATION = token_value(token, TokenElevation)?;
            let session: u32 = token_value(token, TokenSessionId)?;
            let mut length = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
            if length < std::mem::size_of::<TOKEN_USER>() as u32 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut count = 0;
            while *text.add(count) != 0 {
                count += 1;
            }
            let user = String::from_utf16_lossy(std::slice::from_raw_parts(text, count));
            LocalFree(text.cast());
            Ok(Self { user, session, elevated: elevation.TokenIsElevated != 0 })
        }
    }

    fn permits(&self, peer: &Self) -> bool {
        self.elevated && peer.elevated && self.user == peer.user && self.session == peer.session
    }

    pub(super) fn endpoint_name(&self, config_file: Option<&Path>) -> io::Result<String> {
        let mut scope = DefaultHasher::new();
        // 同一程序/数据目录才共享窗口，隔离开发构建、便携目录和显式配置文件。
        std::env::current_exe()?.canonicalize()?.hash(&mut scope);
        std::path::absolute(nebula_settings::settings_dir())?.hash(&mut scope);
        config_file.map(std::path::absolute).transpose()?.hash(&mut scope);
        Ok(format!(
            "Pebrel.elevated-launch.v2.{}.{}.{:016x}",
            self.user,
            self.session,
            scope.finish()
        ))
    }
}

fn token_value<T: Copy>(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<T> {
    // SAFETY: 仅用于上方固定大小的 TOKEN_ELEVATION 和 u32，会核对实际写入长度。
    unsafe {
        let mut value: T = std::mem::zeroed();
        let mut length = 0;
        if GetTokenInformation(
            token,
            class,
            (&mut value as *mut T).cast(),
            std::mem::size_of::<T>() as u32,
            &mut length,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        if length as usize != std::mem::size_of::<T>() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected token information"));
        }
        Ok(value)
    }
}

pub(super) fn acquire_owner(name: &str) -> io::Result<Option<OwnedHandle>> {
    // 命名对象只供管理员/SYSTEM 使用；句柄随接收线程存活，不依赖托盘或真实终端窗口。
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)S:(ML;;NWNR;;;HI)\0".encode_utf16().collect();
    let name: Vec<u16> = format!("Local\\{name}\0").encode_utf16().collect();
    // SAFETY: 描述符由 LocalFree 释放；CreateMutexW 在返回前复制安全属性。
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // 只以命名对象是否已存在选出拥有者，不等待互斥锁，也不跨线程转移锁的持有权。
        windows_sys::Win32::Foundation::SetLastError(0);
        let handle = CreateMutexW(&attributes, 0, name.as_ptr());
        let error = GetLastError();
        LocalFree(descriptor);
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let handle = OwnedHandle::from_raw_handle(handle);
        Ok((error != ERROR_ALREADY_EXISTS).then_some(handle))
    }
}

pub(super) fn verify_window(window: HWND, identity: &Identity) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: Windows 验证 HWND；不把请求中的 PID 或可伪造的 wParam 当成接收端身份。
    if unsafe { GetWindowThreadProcessId(window, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !identity.permits(&Identity::for_process(pid)?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "elevated launch window identity mismatch",
        ));
    }
    Ok(pid)
}

pub(super) fn sender_lifetime(pid: u32) -> io::Result<OwnedHandle> {
    // 请求 PID 仅用于检测启动器退出，不作为发送者认证；管理员窗口保留系统默认 UIPI。
    // SAFETY: 句柄仅用于等待进程退出，立即交给 OwnedHandle 释放。
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_handover_requires_the_same_user_session_and_elevation() {
        let current = Identity::current().unwrap();
        assert_eq!(current.elevated, crate::platform::elevation::is_elevated().unwrap());
        let elevated = Identity { elevated: true, ..current.clone() };
        assert!(elevated.permits(&elevated));
        assert!(!elevated.permits(&Identity { elevated: false, ..elevated.clone() }));
        assert!(
            !elevated
                .permits(&Identity { user: format!("{}-other", current.user), ..elevated.clone() })
        );
        assert!(
            !elevated.permits(&Identity {
                session: current.session.wrapping_add(1),
                ..elevated.clone()
            })
        );
        assert!(!Identity { elevated: false, ..current }.permits(&elevated));
    }
}
