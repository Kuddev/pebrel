//! Open local regular files without following symlinks or blocking on FIFOs/devices.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegularProbe {
    RegularFile,
    NotFound,
    InvalidOrError,
}

pub(crate) fn is_dir_no_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

pub(crate) fn is_dir_and_ancestors_no_symlink(path: &Path) -> bool {
    path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        && path.ancestors().all(is_dir_no_symlink)
}

fn valid_relative_segment(seg: &str) -> bool {
    !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(['/', '\\', '\0'])
}

/// Open a regular file beneath `root` after verifying that `root`, all of its ancestors,
/// every intermediate relative directory, and the leaf file itself are free of symlinks,
/// reparse points, FIFOs, sockets, and device nodes.
///
/// Preserves `io::ErrorKind::NotFound` strictly for genuinely absent relative segments inside
/// an already verified `root`; untrusted roots, symlinked ancestors, and non-regular files
/// return `InvalidInput` or OS link/type errors.
pub(crate) fn open_regular_under_root(root: &Path, rel_segments: &[&str]) -> io::Result<File> {
    if !is_dir_and_ancestors_no_symlink(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "trusted root or ancestor is missing or a symlink",
        ));
    }
    let Some((&leaf, dirs)) = rel_segments.split_last() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "missing relative leaf"));
    };
    if !rel_segments.iter().copied().all(valid_relative_segment) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid relative segment"));
    }

    #[cfg(unix)]
    let file = {
        use std::ffi::{CString, OsStr};
        use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::OpenOptionsExt as _;

        let root_file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(root)
            .map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    io::Error::new(io::ErrorKind::InvalidInput, "trusted root disappeared")
                } else {
                    err
                }
            })?;
        let mut dir_fd: OwnedFd = root_file.into();
        for &seg in dirs {
            let c_seg = CString::new(OsStr::new(seg).as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "interior NUL"))?;
            // SAFETY: `dir_fd` is a valid open directory fd and `c_seg` is NUL-terminated.
            let next_fd = unsafe {
                libc::openat(
                    dir_fd.as_raw_fd(),
                    c_seg.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if next_fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `openat` succeeded and returned a new owned file descriptor.
            dir_fd = unsafe { OwnedFd::from_raw_fd(next_fd) };
        }
        let c_leaf = CString::new(OsStr::new(leaf).as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "interior NUL"))?;
        let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir_fd`, `c_leaf`, and `stat_buf` are valid for the duration of `fstatat`.
        if unsafe {
            libc::fstatat(
                dir_fd.as_raw_fd(),
                c_leaf.as_ptr(),
                &mut stat_buf,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if (stat_buf.st_mode & libc::S_IFMT) != libc::S_IFREG {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
        }
        // SAFETY: `dir_fd` and `c_leaf` are valid; `O_NONBLOCK | O_NOFOLLOW` prevents blocking or link traversal.
        let leaf_fd = unsafe {
            libc::openat(
                dir_fd.as_raw_fd(),
                c_leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if leaf_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a fresh file descriptor transferred into `OwnedFd`.
        File::from(unsafe { OwnedFd::from_raw_fd(leaf_fd) })
    };

    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt as _;
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_TYPE_DISK, GetFileType,
        };

        let mut cur = root.to_path_buf();
        for &seg in dirs {
            cur.push(seg);
            match std::fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_dir() => {},
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "intermediate segment is not a regular directory",
                    ));
                },
                Err(err) => return Err(err),
            }
        }
        let leaf_path = cur.join(leaf);
        match std::fs::symlink_metadata(&leaf_path) {
            Ok(meta) if meta.file_type().is_file() => {},
            Ok(_) => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
            },
            Err(err) => return Err(err),
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&leaf_path)?;
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

pub(crate) fn probe_regular_under_root(root: &Path, rel_segments: &[&str]) -> RegularProbe {
    match open_regular_under_root(root, rel_segments) {
        Ok(_) => RegularProbe::RegularFile,
        Err(err) if err.kind() == io::ErrorKind::NotFound => RegularProbe::NotFound,
        Err(_) => RegularProbe::InvalidOrError,
    }
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
