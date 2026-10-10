//! 原生管道身份验证：PID 来自内核，角色、用户及登录会话来自进程令牌。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_INFORMATION_CLASS,
    TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenSessionId, TokenUser,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

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

    pub(super) fn pipe_name(&self, config_file: Option<&Path>) -> io::Result<String> {
        let mut scope = DefaultHasher::new();
        // 同一程序/数据目录才共享窗口，隔离开发构建、便携目录和显式配置文件。
        std::env::current_exe()?.canonicalize()?.hash(&mut scope);
        std::path::absolute(nebula_settings::settings_dir())?.hash(&mut scope);
        config_file.map(std::path::absolute).transpose()?.hash(&mut scope);
        Ok(format!(
            r"\\.\pipe\Pebrel.elevated-launch.v1.{}.{}.{:016x}",
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

pub(super) fn create_server(name: &str) -> io::Result<NamedPipeServer> {
    // 默认命名管道允许 Everyone 读取；显式管理员 DACL 和高完整性读写限制排除该默认值。
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)S:(ML;;NWNR;;;HI)\0".encode_utf16().collect();
    // SAFETY: 描述符由 LocalFree 释放；CreateNamedPipe 在返回前复制安全属性。
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
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(true)
            .max_instances(1)
            .reject_remote_clients(true)
            .in_buffer_size(4096)
            .out_buffer_size(4096)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(descriptor);
        result
    }
}

pub(super) fn verify_server(pipe: &NamedPipeClient, identity: &Identity) -> io::Result<u32> {
    verify_peer(pipe.as_raw_handle(), identity, false)
}

pub(super) fn verify_client(pipe: &NamedPipeServer, identity: &Identity) -> io::Result<()> {
    verify_peer(pipe.as_raw_handle(), identity, true).map(|_| ())
}

fn verify_peer(pipe: HANDLE, identity: &Identity, server_side: bool) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: pipe 是仍存活的 Tokio 管道句柄，PID 写入有效的局部变量。
    let ok = unsafe {
        if server_side {
            GetNamedPipeClientProcessId(pipe, &mut pid)
        } else {
            GetNamedPipeServerProcessId(pipe, &mut pid)
        }
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if !identity.permits(&Identity::for_process(pid)?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "elevated launch peer identity mismatch",
        ));
    }
    Ok(pid)
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
