//! Platform file operations shared by the legacy file tree and GPUI workspace.

#[cfg(windows)]
pub(crate) fn send_to_recycle_bin(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };

    if !path.exists() {
        return Err("\u{8def}\u{5f84}\u{5df2}\u{4e0d}\u{5b58}\u{5728}".to_owned());
    }
    let mut source: Vec<u16> = path.as_os_str().encode_wide().collect();
    source.push(0);
    source.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE as u32,
        pFrom: source.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // Background executor threads do not inherit the UI thread's COM apartment.
    // Keep initialization and the synchronous Shell operation on this thread.
    let initialized = unsafe {
        CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32)
    };
    if initialized < 0 {
        return Err(format!("COM initialization failed (HRESULT {initialized:#010x})"));
    }
    let code = unsafe { SHFileOperationW(&mut operation) };
    // Both S_OK and S_FALSE acquire an initialization reference.
    unsafe { CoUninitialize() };
    if code == 0 && operation.fAnyOperationsAborted == 0 {
        Ok(())
    } else if matches!(code, 32 | 33) {
        Err(std::io::Error::from_raw_os_error(code).to_string())
    } else {
        Err(format!("\u{7cfb}\u{7edf}\u{62d2}\u{7edd}\u{ff08}\u{4ee3}\u{7801} {code}\u{ff09}"))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn recycling_a_locked_file_on_a_worker_returns_an_error_and_keeps_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("locked.db");
        std::fs::write(&path, b"in use").unwrap();
        let locked = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let worker_path = path.clone();
        let result = std::thread::spawn(move || send_to_recycle_bin(&worker_path)).join().unwrap();
        assert!(result.is_err(), "a locked file must report a deletion failure");
        assert!(path.exists());
        drop(locked);
        assert_eq!(std::fs::read(&path).unwrap(), b"in use");
    }
}

#[cfg(not(windows))]
pub(crate) fn send_to_recycle_bin(path: &std::path::Path) -> Result<(), String> {
    let result =
        if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    result.map_err(|error| error.to_string())
}
