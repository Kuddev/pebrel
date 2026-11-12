//! Open local regular files without following symlinks or blocking on FIFOs/devices.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path};

pub(crate) fn is_regular_no_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

pub(crate) fn is_dir_no_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

pub(crate) fn open_regular_nonblocking(path: &Path) -> io::Result<File> {
    if !is_regular_no_symlink(path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt as _;
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_TYPE_DISK, GetFileType,
        };
        let file =
            OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
        // SAFETY: `file` owns a live kernel file handle for the duration of this call.
        if unsafe { GetFileType(file.as_raw_handle() as _) } != FILE_TYPE_DISK {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a disk file"));
        }
        file
    };
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "opened handle is not regular"));
    }
    Ok(file)
}

/// Lexical directory equality using host path-component case rules without filesystem I/O.
pub(crate) fn paths_equal_lexically(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        if left.to_str().is_some_and(|s| s.starts_with(r"\\"))
            != right.to_str().is_some_and(|s| s.starts_with(r"\\"))
        {
            return false;
        }
        let mut a = left.components();
        let mut b = right.components();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(ca), Some(cb)) => match (ca, cb) {
                    (Component::Prefix(pa), Component::Prefix(pb)) => {
                        if !pa.as_os_str().eq_ignore_ascii_case(pb.as_os_str()) {
                            return false;
                        }
                    },
                    (Component::RootDir, Component::RootDir) => {},
                    (Component::Normal(na), Component::Normal(nb)) => {
                        let eq = match (na.to_str(), nb.to_str()) {
                            (Some(sa), Some(sb)) => sa.eq_ignore_ascii_case(sb),
                            _ => na == nb,
                        };
                        if !eq {
                            return false;
                        }
                    },
                    _ => return false,
                },
                _ => return false,
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = Component::RootDir;
        left == right
    }
}

#[cfg(test)]
pub(crate) fn create_fifo_for_test(path: &Path) -> Option<io::Result<()>> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;
        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c_path` is a valid NUL-terminated path buffer owned by this frame.
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        Some(if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
