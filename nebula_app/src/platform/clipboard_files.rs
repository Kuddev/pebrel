//! File-copy clipboard access. Other platforms keep their existing paste behavior.

use std::io;
use std::path::PathBuf;

/// Explorer publishes CF_HDROP separately from text. Release the native handle
/// before decoding the owned payload, and never reinterpret ordinary text paths.
#[cfg(windows)]
pub(crate) fn read() -> io::Result<Option<Vec<PathBuf>>> {
    use crate::clipboard::files::{MAX_PAYLOAD_BYTES, decode};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_HDROP: u32 = 15;
    struct ClipboardLock;
    impl Drop for ClipboardLock {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) } == 0 {
        return Ok(None);
    }
    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let clipboard = ClipboardLock;
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) } == 0 {
        return Ok(None); // The clipboard changed before we acquired its lock.
    }
    let bytes = unsafe {
        let handle = GetClipboardData(CF_HDROP);
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let len = GlobalSize(handle);
        if !(20..=MAX_PAYLOAD_BYTES).contains(&len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File clipboard payload is too large or invalid",
            ));
        }
        let source = GlobalLock(handle) as *const u8;
        if source.is_null() {
            return Err(io::Error::last_os_error());
        }
        let bytes = std::slice::from_raw_parts(source, len).to_vec();
        GlobalUnlock(handle);
        bytes
    };
    drop(clipboard);
    decode(&bytes).map(Some)
}

#[cfg(not(windows))]
pub(crate) fn read() -> io::Result<Option<Vec<PathBuf>>> {
    Ok(None)
}
