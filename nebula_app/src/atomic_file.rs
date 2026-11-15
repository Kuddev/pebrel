//! Small cross-platform primitives for durable application-state files.
//!
//! State writers use a sibling temporary file followed by an atomic replace,
//! so a crash cannot leave a half-written JSON document. A best-effort lock
//! prevents two Nebula processes from compacting the same store at once.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

/// Atomically replace `path` with `contents`, syncing the temporary file first.
pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_permissions(path, contents, false)
}

/// CLI 凭据导出使用私有新文件权限；原子替换不会把 0600 放宽为 umask 默认值。
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_permissions(path, contents, true)
}

fn write_with_permissions(path: &Path, contents: &[u8], private: bool) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("state path has no parent"))?;
    std::fs::create_dir_all(parent)?;

    let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("state");
    let temporary =
        parent.join(format!(".{file_name}.nebula-tmp-{}-{sequence}", std::process::id()));

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        if private {
            crate::platform::private_file::restrict_creation(&mut options);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, path)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Try to own a sibling lock file without waiting on the UI thread.
pub(crate) fn try_lock(path: &Path) -> io::Result<Option<FileLock>> {
    let lock_path = lock_path(path);
    // Only an existing lock file means contention. Creating its parent can
    // also return AlreadyExists when an ordinary file occupies that path.
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match create_lock(&lock_path) {
        Ok(lock) => Ok(Some(lock)),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = match std::fs::symlink_metadata(&lock_path) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(err) => return Err(err),
            };
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "state lock is not a regular file",
                ));
            }

            // A crashed process can strand the tiny lock file. State writes
            // finish in milliseconds, so a 30-second-old lock is stale enough
            // to reclaim without racing a healthy writer.
            let stale =
                metadata.modified()?.elapsed().is_ok_and(|age| age >= Duration::from_secs(30));
            if !stale {
                return Ok(None);
            }
            match std::fs::remove_file(&lock_path) {
                Ok(()) => {},
                Err(err) if err.kind() == io::ErrorKind::NotFound => {},
                Err(err) => return Err(err),
            }
            match create_lock(&lock_path) {
                Ok(lock) => Ok(Some(lock)),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                Err(err) => Err(err),
            }
        },
        Err(err) => Err(err),
    }
}

/// Try to own a lock for the full lifetime of a resident process.
///
/// 与短状态写锁不同，这里依赖 OS 句柄锁：进程崩溃时内核立即释放，健康进程
/// 无论运行多久都不会被“超时回收”。锁文件本身可以保留，所有权只看句柄。
pub(crate) fn try_lifetime_lock(path: &Path) -> io::Result<Option<LifetimeFileLock>> {
    let lock_path = lock_path(path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt as _;

        match OpenOptions::new().read(true).write(true).create(true).share_mode(0).open(&lock_path)
        {
            Ok(file) => file,
            Err(error) if matches!(error.raw_os_error(), Some(32) | Some(33)) => return Ok(None),
            Err(error) => return Err(error),
        }
    };

    #[cfg(unix)]
    let mut file = {
        use std::os::fd::AsRawFd as _;

        let file = OpenOptions::new().read(true).write(true).create(true).open(&lock_path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(error);
        }
        file
    };

    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;
    file.sync_data()?;
    Ok(Some(LifetimeFileLock { file }))
}

fn create_lock(path: &Path) -> io::Result<FileLock> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(err) = write!(file, "{}", std::process::id()) {
        // 锁文件一旦创建成功就代表“已占用”；写 PID 失败时必须立刻回收，
        // 否则一次磁盘错误会伪装成持续 30 秒的跨进程竞争。
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(err);
    }
    drop(file);
    Ok(FileLock { path: path.to_owned() })
}

fn lock_path(path: &Path) -> PathBuf {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    path.with_extension(if extension.is_empty() {
        "nebula-lock".to_owned()
    } else {
        format!("{extension}.nebula-lock")
    })
}

pub(crate) struct FileLock {
    path: PathBuf,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) struct LifetimeFileLock {
    file: File,
}

impl Drop for LifetimeFileLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

#[cfg(windows)]
pub(crate) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(not(windows))]
pub(crate) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)?;
    // rename 只保证目录项切换是原子的；同步父目录后，掉电恢复时才不会
    // 出现“文件内容已落盘、文件名更新却丢失”的窗口。
    if let Some(parent) = destination.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{lock_path, try_lifetime_lock, try_lock, write};

    #[test]
    fn lock_parent_that_is_a_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, b"existing state").unwrap();

        assert!(try_lock(&parent.join("state.json")).is_err());
        assert_eq!(std::fs::read(parent).unwrap(), b"existing state");
    }

    #[test]
    fn a_directory_at_the_lock_path_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let lock = lock_path(&path);
        std::fs::create_dir(&lock).unwrap();

        assert!(try_lock(&path).is_err());
        assert!(lock.is_dir());
    }

    #[test]
    fn atomic_write_replaces_and_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write(&path, b"one").unwrap();
        write(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");

        let first = try_lock(&path).unwrap().expect("first lock");
        assert!(try_lock(&path).unwrap().is_none());
        drop(first);
        assert!(try_lock(&path).unwrap().is_some());

        let owner = try_lifetime_lock(&path).unwrap().expect("first lifetime lock");
        assert!(try_lifetime_lock(&path).unwrap().is_none());
        drop(owner);
        assert!(try_lifetime_lock(&path).unwrap().is_some());
    }
}
